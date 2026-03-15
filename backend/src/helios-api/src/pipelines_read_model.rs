use crate::api_observability::{ApiCacheMetric, CacheMetricCounters};
use crate::http::AppState;
use crate::http::pipelines::{
    DaedalusRegistryFanInPort, DaedalusRegistryNode, DaedalusRegistryPort, DaedalusRegistryResponse, DaedalusRegistryType, DaedalusSyncGroup, PipelineDocument, PipelineSummary, PlannerDiagnostic,
    PlannerDiagnosticSpan, RegistryPortMetadataLookup, build_registry_port_metadata_lookup, inject_port_metadata_lookup, map_io_error, pipeline_dir,
};
use axum::response::Response;
use bytes::Bytes;
use helios_engine::ipc::{EngineErrorCode, EngineEvent, NodeRegistrySnapshot};
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant};
use tracing::warn;
use uuid::Uuid;

#[derive(Clone, Debug)]
struct GraphValidationState {
    diagnostics: Vec<PlannerDiagnostic>,
    updated_at_ms: i64,
}

#[derive(Clone)]
struct GraphListCacheEntry {
    fetched_at: Instant,
    revision: u64,
    payload: Arc<Vec<PipelineSummary>>,
}

#[derive(Clone)]
struct CachedRegistryPayload {
    port_metadata_lookup: Arc<RegistryPortMetadataLookup>,
    body: Bytes,
}

#[derive(Clone)]
struct RegistryCacheEntry {
    fetched_at: Instant,
    revision: u64,
    payload: Arc<CachedRegistryPayload>,
}

const GRAPH_VALIDATION_CACHE_MAX_AGE_MS: i64 = 60_000;
const REGISTRY_CACHE_MAX_AGE: Duration = Duration::from_secs(30);
const REGISTRY_HELPER_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Default)]
pub struct PipelinesReadModelState {
    graph_validation_cache: RwLock<HashMap<Uuid, GraphValidationState>>,
    graph_list_cache: RwLock<Option<GraphListCacheEntry>>,
    graph_list_refresh_lock: tokio::sync::Mutex<()>,
    graph_list_stats: CacheMetricCounters,
    registry_cache: Arc<RwLock<Option<RegistryCacheEntry>>>,
    registry_refresh_lock: tokio::sync::Mutex<()>,
    registry_stats: CacheMetricCounters,
}

fn now_timestamp_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn graph_validation_state_stale(state: &GraphValidationState, graph_updated_at_ms: i64, now_ms: i64) -> bool {
    if graph_updated_at_ms > 0 && state.updated_at_ms < graph_updated_at_ms {
        return true;
    }
    now_ms.saturating_sub(state.updated_at_ms) > GRAPH_VALIDATION_CACHE_MAX_AGE_MS
}

fn map_planner_diagnostics(diagnostics: Vec<helios_engine::ipc::PlannerDiagnostic>) -> Vec<PlannerDiagnostic> {
    diagnostics
        .into_iter()
        .map(|diag| PlannerDiagnostic { code: diag.code, message: diag.message, span: PlannerDiagnosticSpan { pass: diag.span.pass, node: diag.span.node, port: diag.span.port } })
        .collect()
}

impl PipelinesReadModelState {
    fn schedule_registry_cache_expiry(&self, revision: u64) {
        let registry_cache = Arc::clone(&self.registry_cache);
        tokio::spawn(async move {
            tokio::time::sleep(REGISTRY_CACHE_MAX_AGE).await;
            let mut cache = registry_cache.write().await;
            if cache.as_ref().is_some_and(|entry| entry.revision == revision) {
                *cache = None;
                drop(cache);
                trim_process_allocator();
            }
        });
    }

    pub fn graph_list_cache_metrics(&self) -> ApiCacheMetric {
        self.graph_list_stats.snapshot()
    }

    pub fn registry_cache_metrics(&self) -> ApiCacheMetric {
        self.registry_stats.snapshot()
    }

    async fn snapshot_graph_validation(&self) -> HashMap<Uuid, GraphValidationState> {
        self.graph_validation_cache.read().await.clone()
    }

    async fn prune_graph_validation_cache(&self, valid_ids: &HashSet<Uuid>) {
        let mut cache = self.graph_validation_cache.write().await;
        cache.retain(|id, _| valid_ids.contains(id));
    }

    pub async fn set_graph_validation_state(&self, graph_id: Uuid, diagnostics: Vec<PlannerDiagnostic>) {
        let mut cache = self.graph_validation_cache.write().await;
        cache.insert(graph_id, GraphValidationState { diagnostics, updated_at_ms: now_timestamp_ms() });
    }

    pub async fn invalidate_registry_cache(&self) {
        *self.registry_cache.write().await = None;
        invalidate_registry_snapshot_file().await;
    }

    pub async fn set_graph_validation_error(&self, graph_id: Uuid, code: Option<EngineErrorCode>, message: String) {
        let fallback = code.map(|value| format!("{value:?}")).unwrap_or_else(|| "validation_error".to_string());
        let diag = PlannerDiagnostic { code: fallback, message, span: PlannerDiagnosticSpan { pass: "engine".to_string(), node: None, port: None } };
        self.set_graph_validation_state(graph_id, vec![diag]).await;
    }

    pub async fn clear_graph_validation_state(&self, graph_id: Uuid) {
        let mut cache = self.graph_validation_cache.write().await;
        cache.remove(&graph_id);
    }

    pub async fn invalidate_graph_list_cache(&self) {
        *self.graph_list_cache.write().await = None;
    }

    pub async fn get_cached_graph_summaries_snapshot(&self, state: &AppState) -> Result<(Arc<Vec<PipelineSummary>>, u64), Box<Response>> {
        const FRESH_FOR: Duration = Duration::from_secs(2);

        if let Some(entry) = self.graph_list_cache.read().await.clone()
            && entry.fetched_at.elapsed() < FRESH_FOR
        {
            self.graph_list_stats.record_hit();
            return Ok((entry.payload, entry.revision));
        }

        self.graph_list_stats.record_miss();
        let _refresh_guard = self.graph_list_refresh_lock.lock().await;
        if let Some(entry) = self.graph_list_cache.read().await.clone()
            && entry.fetched_at.elapsed() < FRESH_FOR
        {
            self.graph_list_stats.record_hit();
            return Ok((entry.payload, entry.revision));
        }

        let stale = self.graph_list_cache.read().await.clone();
        match self.load_graph_summaries(state).await {
            Ok(summaries) => {
                let payload = Arc::new(summaries);
                let revision = self.graph_list_stats.record_refresh();
                *self.graph_list_cache.write().await = Some(GraphListCacheEntry { fetched_at: Instant::now(), revision, payload: payload.clone() });
                Ok((payload, revision))
            }
            Err(resp) => match stale {
                Some(entry) => {
                    self.graph_list_stats.record_stale_fallback();
                    Ok((entry.payload, entry.revision))
                }
                None => Err(resp),
            },
        }
    }

    async fn load_graph_summaries(&self, state: &AppState) -> Result<Vec<PipelineSummary>, Box<Response>> {
        let dir = pipeline_dir()?;
        let mut loaded_docs: Vec<(PipelineDocument, i64)> = Vec::new();
        let mut existing_ids = HashSet::new();
        let validation_snapshot = self.snapshot_graph_validation().await;
        let mut entries = match fs::read_dir(dir).await {
            Ok(entries) => entries,
            Err(err) => return Err(Box::new(map_io_error(err, "failed to read pipeline directory"))),
        };

        loop {
            let entry = match entries.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(err) => return Err(Box::new(map_io_error(err, "failed to read pipeline entry"))),
            };

            let meta = match entry.metadata().await {
                Ok(meta) if meta.is_file() => meta,
                Ok(_) => continue,
                Err(err) => return Err(Box::new(map_io_error(err, "failed to stat pipeline file"))),
            };

            let data = match fs::read_to_string(entry.path()).await {
                Ok(data) => data,
                Err(err) => return Err(Box::new(map_io_error(err, "failed to read pipeline file"))),
            };
            if let Ok(doc) = serde_json::from_str::<PipelineDocument>(&data) {
                let doc_id = doc.id;
                let mut updated_at_ms = doc.updated_at_ms.max(0);
                if updated_at_ms == 0
                    && let Ok(modified) = meta.modified()
                    && let Ok(ts) = modified.duration_since(std::time::UNIX_EPOCH)
                {
                    updated_at_ms = ts.as_millis() as i64;
                }
                loaded_docs.push((doc, updated_at_ms));
                existing_ids.insert(doc_id);
            }
        }

        let now_ms = now_timestamp_ms();
        let mut refresh_docs = Vec::new();
        let summaries: Vec<PipelineSummary> = loaded_docs
            .into_iter()
            .map(|(doc, updated_at_ms)| {
                let needs_refresh = match validation_snapshot.get(&doc.id) {
                    Some(state) => graph_validation_state_stale(state, updated_at_ms, now_ms),
                    None => true,
                };
                if needs_refresh {
                    refresh_docs.push((doc.id, doc.graph.clone()));
                }
                let issue_count = validation_snapshot.get(&doc.id).map(|state| state.diagnostics.len()).unwrap_or(0);
                PipelineSummary { id: doc.id, name: doc.name, updated_at_ms, issue_count }
            })
            .collect();

        self.prune_graph_validation_cache(&existing_ids).await;
        if !refresh_docs.is_empty() {
            let state = state.clone();
            let pipelines = state.services.pipelines.clone();
            tokio::spawn(async move {
                for (graph_id, graph) in refresh_docs {
                    pipelines.refresh_graph_validation(&state, graph_id, &graph).await;
                }
                pipelines.invalidate_graph_list_cache().await;
            });
        }

        Ok(summaries)
    }

    pub async fn refresh_graph_validation(&self, state: &AppState, graph_id: Uuid, graph: &JsonValue) {
        match state.engine.validate_graph_event(graph.clone(), Vec::new(), true).await {
            Ok(EngineEvent::GraphValidation { report, .. }) => {
                let diagnostics = map_planner_diagnostics(report.diagnostics);
                self.set_graph_validation_state(graph_id, diagnostics).await;
            }
            Ok(EngineEvent::Nack { code, reason, .. }) => {
                self.set_graph_validation_error(graph_id, Some(code), reason).await;
            }
            Ok(_) => {
                self.set_graph_validation_error(graph_id, Some(EngineErrorCode::Internal), "unexpected engine response".to_string()).await;
            }
            Err(err) => {
                warn!(error = %err, graph_id = %graph_id, "graph validation failed");
                self.set_graph_validation_error(graph_id, Some(EngineErrorCode::Internal), format!("validation failed: {err}")).await;
            }
        }
    }

    pub async fn warm_registry_cache(&self, state: AppState) {
        let _ = self.get_cached_registry_payload(&state).await;
    }

    pub async fn get_cached_registry_response_snapshot(&self, state: &AppState) -> Option<(Bytes, bool, u64)> {
        self.get_cached_registry_payload(state).await.map(|(payload, stale, revision)| (payload.body.clone(), stale, revision))
    }

    pub async fn inject_cached_port_metadata(&self, state: &AppState, graph: &mut JsonValue) {
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

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn trim_process_allocator() {
    unsafe extern "C" {
        fn malloc_trim(pad: usize) -> i32;
    }

    // The registry cache is a large, bursty heap object graph. Trim immediately after eviction so
    // the low-memory profile actually returns pages to the OS instead of just freeing Rust objects
    // back into glibc arenas.
    unsafe {
        let _ = malloc_trim(0);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn trim_process_allocator() {}

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
    std::env::var("HELIOS_NODE_REGISTRY_SNAPSHOT_PATH").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/var/lib/helios/state/node-registry.snapshot.json"))
}

async fn invalidate_registry_snapshot_file() {
    let _ = fs::remove_file(registry_snapshot_path()).await;
}

fn registry_generator_binary() -> PathBuf {
    std::env::var("HELIOS_ENGINE_BIN").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/usr/bin/helios-engine"))
}

fn registry_plugin_dirs() -> Vec<PathBuf> {
    if let Ok(single) = std::env::var("HELIOS_DAEDALUS_PLUGIN_DIR") {
        let trimmed = single.trim();
        if !trimmed.is_empty() {
            return vec![PathBuf::from(trimmed)];
        }
    }
    if let Ok(list) = std::env::var("HELIOS_DAEDALUS_PLUGIN_DIRS") {
        let dirs: Vec<_> = std::env::split_paths(&list).collect();
        if !dirs.is_empty() {
            return dirs;
        }
    }
    vec![PathBuf::from("/var/lib/helios/plugins/daedalus"), PathBuf::from("/usr/lib/helios/plugins/daedalus")]
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
