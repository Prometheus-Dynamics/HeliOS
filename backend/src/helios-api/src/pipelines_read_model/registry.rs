use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use helios_engine::ipc::NodeRegistrySnapshot;
use lib_runtime_policy::HELIOS_DAEDALUS_RUNTIME_POLICY;
use tokio::fs;
use tokio::process::Command;
use tokio::time::{Duration, Instant};
use tracing::warn;

use crate::http::AppState;
use crate::http::pipelines::{
    DaedalusRegistryFanInPort, DaedalusRegistryNode, DaedalusRegistryPort, DaedalusRegistryResponse, DaedalusRegistryType, DaedalusSyncGroup, build_registry_port_metadata_lookup,
    inject_port_metadata_lookup,
};

use super::{CachedRegistryPayload, PipelinesReadModelState, RegistryCacheEntry};

const REGISTRY_CACHE_MAX_AGE: Duration = Duration::from_secs(30);
const REGISTRY_HELPER_TIMEOUT: Duration = Duration::from_secs(20);

impl PipelinesReadModelState {
    fn schedule_registry_cache_expiry(&self, revision: u64) {
        let registry_cache = Arc::clone(&self.registry_cache);
        tokio::spawn(async move {
            tokio::time::sleep(REGISTRY_CACHE_MAX_AGE).await;
            let mut cache = registry_cache.write().await;
            if cache.as_ref().is_some_and(|entry| entry.revision == revision) {
                *cache = None;
            }
        });
    }

    pub async fn invalidate_registry_cache(&self) {
        *self.registry_cache.write().await = None;
        invalidate_registry_snapshot_file().await;
    }

    pub async fn warm_registry_cache(&self, state: AppState) {
        let _ = self.get_cached_registry_payload(&state).await;
    }

    pub async fn get_cached_registry_response_snapshot(&self, state: &AppState) -> Option<(Bytes, bool, u64)> {
        self.get_cached_registry_payload(state).await.map(|(payload, stale, revision)| (payload.body.clone(), stale, revision))
    }

    pub async fn inject_cached_port_metadata(&self, state: &AppState, graph: &mut serde_json::Value) {
        if let Some((payload, _, _)) = self.get_cached_registry_payload(state).await {
            inject_port_metadata_lookup(graph, payload.port_metadata_lookup.as_ref());
        }
    }

    async fn get_cached_registry_payload(&self, state: &AppState) -> Option<(Arc<CachedRegistryPayload>, bool, u64)> {
        const IPC_TIMEOUT: Duration = Duration::from_secs(6);
        const IPC_RETRY_TIMEOUT: Duration = Duration::from_secs(18);

        if let Some(entry) = self.registry_cache.read().await.clone()
            && entry.fetched_at.elapsed() < REGISTRY_CACHE_MAX_AGE
        {
            self.registry_stats.record_hit();
            return Some((entry.payload, false, entry.revision));
        }

        self.registry_stats.record_miss();
        let _refresh_guard = self.registry_refresh_lock.lock().await;
        if let Some(entry) = self.registry_cache.read().await.clone()
            && entry.fetched_at.elapsed() < REGISTRY_CACHE_MAX_AGE
        {
            self.registry_stats.record_hit();
            return Some((entry.payload, false, entry.revision));
        }

        let mut stale_entry = self.registry_cache.read().await.clone();
        match load_registry_snapshot_from_disk_or_helper().await {
            Ok(snapshot) => match build_cached_registry_payload(snapshot) {
                Ok(payload) => {
                    let revision = self.registry_stats.record_refresh();
                    *self.registry_cache.write().await = Some(RegistryCacheEntry { fetched_at: Instant::now(), revision, payload: payload.clone() });
                    self.schedule_registry_cache_expiry(revision);
                    return Some((payload, false, revision));
                }
                Err(error) => {
                    warn!(error = %error, "failed to encode file-backed node registry response");
                }
            },
            Err(error) => {
                warn!(error = %error, "file-backed node registry fetch failed; falling back to engine IPC");
            }
        }

        match state.engine.get_node_registry_with_timeout(IPC_TIMEOUT).await {
            Ok(snapshot) => match build_cached_registry_payload(snapshot) {
                Ok(payload) => {
                    let revision = self.registry_stats.record_refresh();
                    *self.registry_cache.write().await = Some(RegistryCacheEntry { fetched_at: Instant::now(), revision, payload: payload.clone() });
                    self.schedule_registry_cache_expiry(revision);
                    Some((payload, false, revision))
                }
                Err(error) => {
                    warn!(error = %error, "failed to encode cached node registry response");
                    stale_entry.map(|entry| {
                        self.registry_stats.record_stale_fallback();
                        (entry.payload, true, entry.revision)
                    })
                }
            },
            Err(error) => {
                if stale_entry.is_none() {
                    warn!(
                        error = ?error,
                        timeout_ms = IPC_TIMEOUT.as_millis(),
                        retry_timeout_ms = IPC_RETRY_TIMEOUT.as_millis(),
                        "node registry fetch timed out; retrying with relaxed timeout"
                    );
                    match state.engine.get_node_registry_with_timeout(IPC_RETRY_TIMEOUT).await {
                        Ok(snapshot) => match build_cached_registry_payload(snapshot) {
                            Ok(payload) => {
                                let revision = self.registry_stats.record_refresh();
                                *self.registry_cache.write().await = Some(RegistryCacheEntry { fetched_at: Instant::now(), revision, payload: payload.clone() });
                                self.schedule_registry_cache_expiry(revision);
                                return Some((payload, false, revision));
                            }
                            Err(error) => {
                                warn!(error = %error, "failed to encode cached node registry response");
                            }
                        },
                        Err(retry_error) => {
                            warn!(
                                error = ?retry_error,
                                timeout_ms = IPC_RETRY_TIMEOUT.as_millis(),
                                "node registry fetch failed after retry"
                            );
                        }
                    }
                    stale_entry = self.registry_cache.read().await.clone();
                }
                stale_entry.map(|entry| {
                    self.registry_stats.record_stale_fallback();
                    (entry.payload, true, entry.revision)
                })
            }
        }
    }
}

fn build_cached_registry_payload(snapshot: NodeRegistrySnapshot) -> Result<Arc<CachedRegistryPayload>, serde_json::Error> {
    let port_metadata_lookup = Arc::new(build_registry_port_metadata_lookup(&snapshot));
    let body = build_registry_response_body(&snapshot)?;
    Ok(Arc::new(CachedRegistryPayload { port_metadata_lookup, body }))
}

fn build_registry_response_body(snapshot: &NodeRegistrySnapshot) -> Result<Bytes, serde_json::Error> {
    let mut nodes: Vec<DaedalusRegistryNode> = snapshot
        .nodes
        .iter()
        .cloned()
        .map(|node| DaedalusRegistryNode {
            id: node.id,
            label: node.label,
            plugin: node.plugin,
            feature_flags: node.feature_flags,
            sync_groups: node
                .sync_groups
                .into_iter()
                .map(|group| DaedalusSyncGroup { name: group.name, policy: group.policy, ports: group.ports, capacity: group.capacity, backpressure: group.backpressure })
                .collect(),
            inputs: node.inputs,
            outputs: node.outputs,
            input_ports: node
                .input_ports
                .into_iter()
                .map(|port| DaedalusRegistryPort { name: port.name, ty: Some(port.ty.into()), source: port.source, const_value: port.const_value.map(|value| value.into()) })
                .collect(),
            fanin_inputs: node.fanin_inputs.into_iter().map(|port| DaedalusRegistryFanInPort { prefix: port.prefix, start: port.start, ty: port.ty.into() }).collect(),
            output_ports: node
                .output_ports
                .into_iter()
                .map(|port| DaedalusRegistryPort { name: port.name, ty: Some(port.ty.into()), source: port.source, const_value: port.const_value.map(|value| value.into()) })
                .collect(),
            default_compute: node.default_compute,
            metadata: node.metadata.into_iter().map(|(key, value)| (key, value.into())).collect(),
        })
        .collect();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    let mut types: Vec<DaedalusRegistryType> = snapshot.types.iter().cloned().map(|entry| DaedalusRegistryType { rust: entry.rust, ty: entry.ty.into() }).collect();
    types.sort_by(|a, b| a.rust.cmp(&b.rust));

    serde_json::to_vec(&DaedalusRegistryResponse { plugins: snapshot.plugins.clone(), nodes, types }).map(Bytes::from)
}

fn registry_snapshot_path() -> PathBuf {
    HELIOS_DAEDALUS_RUNTIME_POLICY.resolve().registry_snapshot_path
}

async fn invalidate_registry_snapshot_file() {
    let _ = fs::remove_file(registry_snapshot_path()).await;
}

pub(super) fn registry_generator_binary() -> PathBuf {
    HELIOS_DAEDALUS_RUNTIME_POLICY.resolve().registry_generator_binary
}

fn registry_plugin_dirs() -> Vec<PathBuf> {
    HELIOS_DAEDALUS_RUNTIME_POLICY.resolve().plugin_search_dirs
}

pub(crate) async fn load_registry_snapshot_from_disk_or_helper() -> Result<NodeRegistrySnapshot, String> {
    let path = registry_snapshot_path();
    if registry_snapshot_needs_refresh(&path).await? {
        refresh_registry_snapshot_via_helper(&path).await?;
    }
    let payload = fs::read(&path).await.map_err(|err| format!("read snapshot failed ({}): {err}", path.display()))?;
    serde_json::from_slice::<NodeRegistrySnapshot>(&payload).map_err(|err| format!("decode snapshot failed ({}): {err}", path.display()))
}

async fn registry_snapshot_needs_refresh(path: &Path) -> Result<bool, String> {
    let snapshot_meta = match fs::metadata(path).await {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(err) => return Err(format!("stat snapshot failed ({}): {err}", path.display())),
    };
    let snapshot_mtime = snapshot_meta.modified().map_err(|err| format!("read snapshot mtime failed ({}): {err}", path.display()))?;
    let newest_source = newest_registry_source_mtime().await?;
    Ok(newest_source.is_some_and(|mtime| snapshot_mtime < mtime))
}

async fn newest_registry_source_mtime() -> Result<Option<std::time::SystemTime>, String> {
    let mut newest: Option<std::time::SystemTime> = None;
    let engine_bin = registry_generator_binary();
    if let Some(mtime) = path_modified(&engine_bin).await? {
        newest = Some(match newest {
            Some(current) => current.max(mtime),
            None => mtime,
        });
    }
    for dir in registry_plugin_dirs() {
        let Some(dir_newest) = newest_plugin_dir_mtime(&dir).await? else {
            continue;
        };
        newest = Some(match newest {
            Some(current) => current.max(dir_newest),
            None => dir_newest,
        });
    }
    Ok(newest)
}

async fn newest_plugin_dir_mtime(dir: &Path) -> Result<Option<std::time::SystemTime>, String> {
    let mut newest: Option<std::time::SystemTime> = None;
    let mut entries = match fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("read plugin dir failed ({}): {err}", dir.display())),
    };

    while let Some(entry) = entries.next_entry().await.map_err(|err| format!("scan plugin dir failed ({}): {err}", dir.display()))? {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !(name.ends_with(".so") || name.ends_with(".so.disabled")) {
            continue;
        }
        let meta = entry.metadata().await.map_err(|err| format!("stat plugin file failed ({}): {err}", path.display()))?;
        let modified = meta.modified().map_err(|err| format!("read plugin mtime failed ({}): {err}", path.display()))?;
        newest = Some(match newest {
            Some(current) => current.max(modified),
            None => modified,
        });
    }

    Ok(newest)
}

async fn path_modified(path: &Path) -> Result<Option<std::time::SystemTime>, String> {
    match fs::metadata(path).await {
        Ok(meta) => meta.modified().map(Some).map_err(|err| format!("read mtime failed ({}): {err}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("stat path failed ({}): {err}", path.display())),
    }
}

async fn refresh_registry_snapshot_via_helper(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.map_err(|err| format!("create snapshot dir failed ({}): {err}", parent.display()))?;
    }

    let engine_bin = registry_generator_binary();
    let output = tokio::time::timeout(REGISTRY_HELPER_TIMEOUT, Command::new(&engine_bin).arg("dump-node-registry").arg("--output").arg(path).output())
        .await
        .map_err(|_| format!("registry helper timed out after {}s", REGISTRY_HELPER_TIMEOUT.as_secs()))?
        .map_err(|err| format!("spawn registry helper failed ({}): {err}", engine_bin.display()))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("exit status {}", output.status)
    };
    Err(format!("registry helper failed ({}): {detail}", engine_bin.display()))
}
