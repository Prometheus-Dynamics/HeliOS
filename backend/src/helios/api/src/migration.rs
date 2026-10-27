//! The node id migration. A board's node id was `node-local` (the default); it is now the board's
//! own id (`HELIOS_NODE_ID`, `systemcore-<serial>` or `raze-<serial>`). Camera and other resource
//! ids embed the node id, so the state that names them is rewritten once, at startup:
//!
//! - helios-api's own JSON files under its state directory (camera settings, mounts, calibrations,
//!   identities, pipeline revision history, ...): every string value and object key that holds the
//!   old id as a whole token.
//! - The pipelines in Orion (workloads): the node they are assigned to, their resource bindings and
//!   their config (the graph and the camera bindings), with the same token rule.
//!
//! The original of every file rewritten is kept under `legacy-node-id/` in the state directory, the
//! first time it is rewritten. A marker file (`node-id-migration.json`) records the migration, so a
//! later start does nothing. The marker is written only after the Orion part succeeded; until then
//! the migration retries in the background with a capped backoff.
//!
//! Orion keeps what the old id published: the providers and executors of `node-local`, and the
//! resources those providers published. Nothing publishes under the old id again, so
//! [`spawn_retire`] removes the desired records of the old id, once the new id's own provider is
//! live, and on every start until none are left (the marker does not cover them: a board migrated
//! before this ran still has them). Orion has no client request that retracts observed records, and
//! it prunes them only when it starts, so the old id's observed resources stay in Orion until then;
//! the API hides them meanwhile (`retired_resources` in `orion.rs`).

use std::{
    collections::BTreeMap,
    io::{self, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use orion::{
    control_plane::{ArtifactRecord, DesiredClusterState, DesiredStateMutation, TypedConfigValue, WorkloadRecord},
    core::{ExecutorId, NodeId, ProviderId, ResourceId},
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{SharedState, config::DEFAULT_NODE_ID, error::ApiResult, host::now_ms};

/// The node id every board had before the board-unique ids.
pub const LEGACY_NODE_ID: &str = DEFAULT_NODE_ID;
pub const MARKER_FILE: &str = "node-id-migration.json";
pub const LEGACY_DIR: &str = "legacy-node-id";
/// The first retry waits this long; each further one doubles it, up to `RETRY_MAX`.
const RETRY_MIN: Duration = Duration::from_millis(500);
const RETRY_MAX: Duration = Duration::from_secs(60);
/// The `graph.inline` config field of a pipeline (its graph document, as JSON text).
const GRAPH_FIELD: &str = "graph.inline";

/// What the marker file records: the ids migrated between, and when.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub from: String,
    pub to: String,
    pub migrated_at_ms: u64,
}

/// Replace every whole-token occurrence of `from` in `text` with `to`, or `None` when there is none.
/// A token is bounded by characters that are not alphanumeric, or by the ends of the text:
/// `camera.node-local.front` and `capture_device_node-local_camera` match, `node-localized` does not.
pub fn rewrite_text(text: &str, from: &str, to: &str) -> Option<String> {
    if from.is_empty() || !text.contains(from) {
        return None;
    }
    let mut out = String::with_capacity(text.len() + to.len());
    let mut last = 0;
    for (start, matched) in text.match_indices(from) {
        let end = start + matched.len();
        let bounded_before = text[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let bounded_after = text[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        if bounded_before && bounded_after {
            out.push_str(&text[last..start]);
            out.push_str(to);
            last = end;
        }
    }
    if last == 0 {
        return None;
    }
    out.push_str(&text[last..]);
    Some(out)
}

/// Rewrite every string value and object key under `value` (nested). Returns whether anything changed.
pub fn rewrite_json(value: &mut serde_json::Value, from: &str, to: &str) -> bool {
    match value {
        serde_json::Value::String(text) => match rewrite_text(text, from, to) {
            Some(new) => {
                *text = new;
                true
            }
            None => false,
        },
        serde_json::Value::Array(items) => items.iter_mut().fold(false, |changed, item| rewrite_json(item, from, to) | changed),
        serde_json::Value::Object(map) => {
            let mut changed = false;
            for (key, mut item) in std::mem::take(map) {
                changed |= rewrite_json(&mut item, from, to);
                let key = match rewrite_text(&key, from, to) {
                    Some(new) => {
                        changed = true;
                        new
                    }
                    None => key,
                };
                map.insert(key, item);
            }
            changed
        }
        _ => false,
    }
}

/// Rewrite one workload record in place: its assigned node, its resource bindings (node and
/// resource ids) and its config (keys and string values, including the graph). Returns whether
/// anything changed.
pub fn rewrite_workload(record: &mut WorkloadRecord, from: &str, to: &str) -> bool {
    let mut changed = false;
    if let Some(node) = &record.assigned_node_id
        && let Some(new) = rewrite_text(node.as_str(), from, to)
    {
        record.assigned_node_id = Some(NodeId::new(new));
        changed = true;
    }
    for binding in &mut record.resource_bindings {
        if let Some(new) = rewrite_text(binding.resource_id.as_str(), from, to) {
            binding.resource_id = ResourceId::new(new);
            changed = true;
        }
        if let Some(new) = rewrite_text(binding.node_id.as_str(), from, to) {
            binding.node_id = NodeId::new(new);
            changed = true;
        }
    }
    if let Some(config) = &mut record.config {
        for (key, mut value) in std::mem::take(&mut config.payload) {
            let key = match rewrite_text(&key, from, to) {
                Some(new) => {
                    changed = true;
                    new
                }
                None => key,
            };
            if let TypedConfigValue::String(text) = &mut value
                && let Some(new) = rewrite_text(text, from, to)
            {
                *text = new;
                changed = true;
            }
            config.payload.insert(key, value);
        }
    }
    changed
}

/// Rewrite the JSON files under `state_dir` (recursively, except `legacy-node-id/` and the marker).
/// The original of each rewritten file is copied to `legacy-node-id/` first, unless it is there
/// already. Returns how many files were rewritten; a second run finds none.
pub fn rewrite_files(state_dir: &Path, from: &str, to: &str) -> io::Result<usize> {
    let mut relatives = Vec::new();
    collect_json(state_dir, Path::new(""), &mut relatives)?;
    let mut rewritten = 0;
    for relative in relatives {
        let path = state_dir.join(&relative);
        let bytes = std::fs::read(&path)?;
        // Not JSON: nothing to rewrite (helios-api could not read it either).
        let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        if !rewrite_json(&mut value, from, to) {
            continue;
        }
        let legacy = state_dir.join(LEGACY_DIR).join(&relative);
        if !legacy.exists() {
            write_atomic(&legacy, &bytes)?;
        }
        let mut out = serde_json::to_vec_pretty(&value).map_err(io::Error::other)?;
        out.push(b'\n');
        write_atomic(&path, &out)?;
        rewritten += 1;
    }
    Ok(rewritten)
}

/// The JSON files under `dir`, as paths relative to `root` (the state directory).
fn collect_json(root: &Path, relative: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    let entries = match std::fs::read_dir(root.join(relative)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let child = relative.join(&name);
        if relative.as_os_str().is_empty() && (name == LEGACY_DIR || name == MARKER_FILE) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            collect_json(root, &child, out)?;
        } else if child.extension().and_then(|e| e.to_str()) == Some("json") {
            out.push(child);
        }
    }
    Ok(())
}

/// Write `bytes` to `path` through a temporary file and a rename, creating the parent folders.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.migrating");
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Whether the marker records the migration from `from` to `to` as done.
pub fn is_done(state_dir: &Path, from: &str, to: &str) -> bool {
    read_marker(state_dir).is_some_and(|marker| marker.from == from && marker.to == to)
}

/// The marker, or `None` when there is none (or it cannot be read: the migration then runs again,
/// which is safe).
pub fn read_marker(state_dir: &Path) -> Option<Marker> {
    let bytes = std::fs::read(state_dir.join(MARKER_FILE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn write_marker(state_dir: &Path, from: &str, to: &str, migrated_at_ms: u64) -> io::Result<()> {
    let marker = Marker { from: from.to_string(), to: to.to_string(), migrated_at_ms };
    let bytes = serde_json::to_vec_pretty(&marker).map_err(io::Error::other)?;
    write_atomic(&state_dir.join(MARKER_FILE), &bytes)
}

/// Rewrite the desired workloads of Orion that name `from` (and their artifacts, whose size follows
/// the graph they hold). Returns how many workloads changed. The records are the desired ones, not
/// the merged view: a re-put must not write observed state back into the desired state.
pub async fn rewrite_orion(state: &SharedState, from: &str, to: &str) -> ApiResult<usize> {
    let _deploys = state.deploys.lock().await;
    let snapshot = state.orion.snapshot().await?;
    let desired = &snapshot.state.desired;
    let mut workloads = Vec::new();
    let mut artifacts: BTreeMap<String, ArtifactRecord> = BTreeMap::new();
    for workload in desired.workloads.values() {
        let mut record = workload.clone();
        if !rewrite_workload(&mut record, from, to) {
            continue;
        }
        if let Some(artifact) = desired.artifacts.get(&record.artifact_id) {
            let mut artifact = artifact.clone();
            let graph = record.config.as_ref().and_then(|config| config.string(GRAPH_FIELD));
            if let (Some(_), Some(graph)) = (artifact.size_bytes, graph) {
                artifact.size_bytes = Some(graph.len() as u64);
            }
            artifacts.insert(artifact.artifact_id.to_string(), artifact);
        }
        workloads.push(record);
    }
    if workloads.is_empty() {
        return Ok(0);
    }
    let count = workloads.len();
    let mut mutations: Vec<DesiredStateMutation> = artifacts.into_values().map(DesiredStateMutation::PutArtifact).collect();
    mutations.extend(workloads.into_iter().map(DesiredStateMutation::PutWorkload));
    state.orion.apply(mutations).await?;
    Ok(count)
}

/// One full pass: the state files, then Orion, then the marker. Safe to repeat.
async fn attempt(state: &SharedState, from: &str, to: &str) -> Result<(usize, usize), String> {
    let files = {
        // The store's lock: no settings write lands between a file's read and its rewrite.
        let _guard = state.store.lock.lock().await;
        rewrite_files(&state.config.state_dir, from, to).map_err(|error| format!("state files: {error}"))?
    };
    let workloads = rewrite_orion(state, from, to).await.map_err(|error| format!("Orion: {error}"))?;
    write_marker(&state.config.state_dir, from, to, now_ms()).map_err(|error| format!("marker: {error}"))?;
    Ok((files, workloads))
}

/// Start the migration when this board's id differs from the legacy one and it has not run yet.
/// The task retries until it has succeeded. `None` when there is nothing to do.
pub fn spawn(state: SharedState) -> Option<tokio::task::JoinHandle<()>> {
    let to = state.config.node_id.clone();
    if to == LEGACY_NODE_ID || is_done(&state.config.state_dir, LEGACY_NODE_ID, &to) {
        return None;
    }
    Some(tokio::spawn(async move {
        let mut delay = RETRY_MIN;
        loop {
            match attempt(&state, LEGACY_NODE_ID, &to).await {
                Ok((files, workloads)) => {
                    info!(from = LEGACY_NODE_ID, %to, files, workloads, "node id migrated");
                    return;
                }
                Err(error) => {
                    warn!(%error, retry_in = ?delay, "node id migration not done yet; retrying");
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(RETRY_MAX);
                }
            }
        }
    }))
}

/// How often the old id's records are looked for again while any are left.
const RETIRE_INTERVAL: Duration = Duration::from_secs(5);

/// The desired records of the old id `from` that this board owns, to remove once its new id `to` is
/// live: the providers and executors that node id published, and the desired resources of those
/// providers. Empty while `to` has no provider yet, and while a peer of the robot (`peers`, its node
/// ids) still answers as `from`: that node's records are its own, never this board's.
pub fn legacy_removals(desired: &DesiredClusterState, from: &str, to: &str, peers: &[String]) -> Vec<DesiredStateMutation> {
    if from == to || !new_id_live(desired, to) || peers.iter().any(|peer| peer == from) {
        return Vec::new();
    }
    let providers: Vec<ProviderId> = desired.providers.values().filter(|provider| provider.node_id.as_str() == from).map(|provider| provider.provider_id.clone()).collect();
    let executors: Vec<ExecutorId> = desired.executors.values().filter(|executor| executor.node_id.as_str() == from).map(|executor| executor.executor_id.clone()).collect();
    let mut removals: Vec<DesiredStateMutation> = desired.resources.values().filter(|resource| providers.contains(&resource.provider_id)).map(|resource| DesiredStateMutation::RemoveResource(resource.resource_id.clone())).collect();
    removals.extend(providers.into_iter().map(DesiredStateMutation::RemoveProvider));
    removals.extend(executors.into_iter().map(DesiredStateMutation::RemoveExecutor));
    removals
}

/// Whether the old id `from` still has a provider or an executor in the desired state.
fn legacy_left(desired: &DesiredClusterState, from: &str) -> bool {
    desired.providers.values().any(|provider| provider.node_id.as_str() == from) || desired.executors.values().any(|executor| executor.node_id.as_str() == from)
}

/// Whether this board's new id has a provider in the desired state (its peripherals have published).
fn new_id_live(desired: &DesiredClusterState, to: &str) -> bool {
    desired.providers.values().any(|provider| provider.node_id.as_str() == to)
}

/// One pass of the cleanup: the old id's records are removed, when the new id is live. `true` when
/// the old id has nothing left, so the task can stop.
async fn retire_once(state: &SharedState, from: &str, to: &str) -> ApiResult<bool> {
    if !legacy_left(&state.orion.snapshot().await?.state.desired, from) {
        return Ok(true);
    }
    // The peers come before the deploy lock: the gateway may probe the network to name them.
    let peers: Vec<String> = crate::peers::robot(state).await?.peers.into_iter().map(|peer| peer.node_id).collect();
    let _deploys = state.deploys.lock().await;
    let snapshot = state.orion.snapshot().await?;
    let desired = &snapshot.state.desired;
    let mutations = legacy_removals(desired, from, to, &peers);
    if !mutations.is_empty() {
        info!(from, %to, removed = mutations.len(), "records of the old node id removed");
        state.orion.apply(mutations).await?;
    }
    Ok(!legacy_left(desired, from))
}

/// Removes the old id's leftovers in Orion (see the module doc), until none are left. Nothing to do
/// when this board never had another id. Safe to repeat: a removed record is not removed again.
pub fn spawn_retire(state: SharedState) -> Option<tokio::task::JoinHandle<()>> {
    let to = state.config.node_id.clone();
    if to == LEGACY_NODE_ID {
        return None;
    }
    Some(tokio::spawn(async move {
        loop {
            match retire_once(&state, LEGACY_NODE_ID, &to).await {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => warn!(%error, "records of the old node id not removed yet; retrying"),
            }
            tokio::time::sleep(RETIRE_INTERVAL).await;
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const OLD: &str = "node-local";
    const NEW: &str = "raze-2c206880";

    #[test]
    fn rewrites_whole_tokens_only() {
        assert_eq!(rewrite_text("camera.node-local.front", OLD, NEW).as_deref(), Some("camera.raze-2c206880.front"));
        assert_eq!(rewrite_text("capture_device_node-local_camera-2c206880", OLD, NEW).as_deref(), Some("capture_device_raze-2c206880_camera-2c206880"));
        assert_eq!(rewrite_text("provider.playback.node-local", OLD, NEW).as_deref(), Some("provider.playback.raze-2c206880"));
        assert_eq!(rewrite_text("node-local", OLD, NEW).as_deref(), Some(NEW));
        assert_eq!(rewrite_text("node-local node-local", OLD, NEW).as_deref(), Some("raze-2c206880 raze-2c206880"));
        // Alphanumerics on either side make it a different word.
        assert_eq!(rewrite_text("node-localized", OLD, NEW), None);
        assert_eq!(rewrite_text("xnode-local", OLD, NEW), None);
        assert_eq!(rewrite_text("node-local2", OLD, NEW), None);
        // A non-alphanumeric character after it is a boundary, as in `camera.node-local.front`.
        assert_eq!(rewrite_text("node-local-2", OLD, NEW).as_deref(), Some("raze-2c206880-2"));
    }

    #[test]
    fn no_op_without_the_old_id() {
        assert_eq!(rewrite_text("camera.raze-1.front", OLD, NEW), None);
        assert_eq!(rewrite_text("", OLD, NEW), None);
        assert_eq!(rewrite_text("node-local", "", NEW), None);
        let mut value = json!({"camera": "camera.raze-1.front", "n": 3});
        assert!(!rewrite_json(&mut value, OLD, NEW));
        assert_eq!(value, json!({"camera": "camera.raze-1.front", "n": 3}));
    }

    #[test]
    fn rewrites_nested_keys_and_values() {
        let mut value = json!({
            "camera.node-local.front": { "exposure": 3, "gain": 1.5, "on": true },
            "mounts": [{ "camera": "camera.node-local.side", "x": 0.25 }, "node-localized"],
            "graph": { "nodes": [{ "resource": "capture_device_node-local_camera-2c206880" }] },
            "none": null
        });
        assert!(rewrite_json(&mut value, OLD, NEW));
        assert_eq!(
            value,
            json!({
                "camera.raze-2c206880.front": { "exposure": 3, "gain": 1.5, "on": true },
                "mounts": [{ "camera": "camera.raze-2c206880.side", "x": 0.25 }, "node-localized"],
                "graph": { "nodes": [{ "resource": "capture_device_raze-2c206880_camera-2c206880" }] },
                "none": null
            })
        );
        // Once rewritten, nothing is left to change.
        assert!(!rewrite_json(&mut value, OLD, NEW));
    }

    #[test]
    fn rewrites_a_workload_record() {
        use orion::{
            control_plane::{WorkloadConfig, WorkloadRecord},
            core::{ArtifactId, WorkloadId},
        };
        let config = WorkloadConfig::new("helios.engine.execution.config.v1")
            .field("graph.inline", TypedConfigValue::String(r#"{"camera":"camera.node-local.front"}"#.into()))
            .field("binding.frame.resource_id", TypedConfigValue::String("capture_device_node-local_camera-2c206880".into()))
            .field("cpu", TypedConfigValue::Int(2));
        let mut record = WorkloadRecord::builder(WorkloadId::new("pipeline.default"), "helios.engine.execution.v1", ArtifactId::new("artifact.pipeline.default"))
            .config(config)
            .bind_resource(ResourceId::new("capture_device_node-local_camera-2c206880"), NodeId::new(OLD))
            .build();
        record.assigned_node_id = Some(NodeId::new(OLD));

        assert!(rewrite_workload(&mut record, OLD, NEW));
        assert_eq!(record.assigned_node_id.as_ref().map(|n| n.as_str()), Some(NEW));
        assert_eq!(record.resource_bindings[0].node_id.as_str(), NEW);
        assert_eq!(record.resource_bindings[0].resource_id.as_str(), "capture_device_raze-2c206880_camera-2c206880");
        let config = record.config.as_ref().expect("config");
        assert_eq!(config.string("graph.inline"), Some(r#"{"camera":"camera.raze-2c206880.front"}"#));
        assert_eq!(config.int("cpu"), Some(2));
        assert!(!rewrite_workload(&mut record, OLD, NEW));
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, text).expect("write");
    }

    #[test]
    fn rewrites_state_files_once_and_keeps_originals() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = dir.path().join("state");
        let settings = r#"{"camera.node-local.front":{"exposure":3}}"#;
        write(&state.join("camera-settings.json"), settings);
        write(&state.join("pipelines").join("default.json"), r#"{"revisions":[{"revision":1,"spec":{"bindings":{"frame":{"resource_id":"capture_device_node-local_camera-2c206880"}}}}]}"#);
        write(&state.join("setup.json"), r#"{"done":true}"#);
        write(&state.join("notes.txt"), "node-local stays: not JSON");
        write(&state.join("corrupt.json"), "{not json node-local");

        assert_eq!(rewrite_files(&state, OLD, NEW).expect("first pass"), 2);

        let rewritten = std::fs::read_to_string(state.join("camera-settings.json")).expect("read");
        assert!(rewritten.contains("camera.raze-2c206880.front") && !rewritten.contains("node-local"));
        assert!(std::fs::read_to_string(state.join("pipelines").join("default.json")).expect("read").contains("capture_device_raze-2c206880_camera-2c206880"));
        // The originals are kept, byte for byte; untouched files get no copy.
        assert_eq!(std::fs::read_to_string(state.join(LEGACY_DIR).join("camera-settings.json")).expect("legacy"), settings);
        assert!(state.join(LEGACY_DIR).join("pipelines").join("default.json").exists());
        assert!(!state.join(LEGACY_DIR).join("setup.json").exists());
        assert_eq!(std::fs::read_to_string(state.join("notes.txt")).expect("read"), "node-local stays: not JSON");

        // A second run rewrites nothing and keeps the first original.
        assert_eq!(rewrite_files(&state, OLD, NEW).expect("second pass"), 0);
        assert_eq!(std::fs::read_to_string(state.join(LEGACY_DIR).join("camera-settings.json")).expect("legacy"), settings);
    }

    #[test]
    fn missing_state_directory_is_nothing_to_do() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(rewrite_files(&dir.path().join("absent"), OLD, NEW).expect("pass"), 0);
    }

    #[test]
    fn marker_records_the_migration_and_is_not_rewritten() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = dir.path();
        write(&state.join("camera-mounts.json"), r#"{"camera.node-local.front":{"x":1}}"#);
        assert!(!is_done(state, OLD, NEW));

        rewrite_files(state, OLD, NEW).expect("pass");
        write_marker(state, OLD, NEW, 42).expect("marker");
        assert!(is_done(state, OLD, NEW));
        assert!(!is_done(state, OLD, "raze-other"), "a marker for another id does not count");
        assert_eq!(read_marker(state), Some(Marker { from: OLD.into(), to: NEW.into(), migrated_at_ms: 42 }));

        // The marker and the legacy copies are not themselves rewritten by a later pass.
        assert_eq!(rewrite_files(state, OLD, NEW).expect("again"), 0);
        assert!(state.join(LEGACY_DIR).join("camera-mounts.json").exists());
    }

    #[test]
    fn a_corrupt_marker_means_not_done() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(&dir.path().join(MARKER_FILE), "{");
        assert!(!is_done(dir.path(), OLD, NEW));
    }

    /// The old board's provider, executor and camera; the new id's provider and camera; a peer's.
    fn robot_state() -> DesiredClusterState {
        use orion::control_plane::{ExecutorRecord, ProviderRecord, ResourceRecord};
        let mut desired = DesiredClusterState::default();
        desired.put_provider(ProviderRecord::builder(ProviderId::new("provider.peripherals.node-local"), NodeId::new(OLD)).build());
        desired.put_provider(ProviderRecord::builder(ProviderId::new("provider.peripherals.raze-2c206880"), NodeId::new(NEW)).build());
        desired.put_provider(ProviderRecord::builder(ProviderId::new("provider.peripherals.systemcore-3c99"), NodeId::new("systemcore-3c99")).build());
        desired.put_executor(ExecutorRecord::builder(ExecutorId::new("executor.peripherals.node-local"), NodeId::new(OLD)).build());
        desired.put_executor(ExecutorRecord::builder(ExecutorId::new("executor.peripherals.raze-2c206880"), NodeId::new(NEW)).build());
        for (resource, provider) in [
            ("capture_device_node-local_camera-2c206880", "provider.peripherals.node-local"),
            ("capture_device_raze-2c206880_camera-2c206880", "provider.peripherals.raze-2c206880"),
            ("capture_device_systemcore-3c99_camera-3c99", "provider.peripherals.systemcore-3c99"),
        ] {
            desired.put_resource(ResourceRecord::builder(ResourceId::new(resource), "camera.device", ProviderId::new(provider)).build());
        }
        desired
    }

    /// Applies the mutations to the desired state as Orion does (plain writes, for the test).
    fn apply_plain(desired: &mut DesiredClusterState, mutations: Vec<DesiredStateMutation>) {
        for mutation in mutations {
            match mutation {
                DesiredStateMutation::RemoveResource(id) => drop(desired.remove_resource(&id)),
                DesiredStateMutation::RemoveProvider(id) => drop(desired.remove_provider(&id)),
                DesiredStateMutation::RemoveExecutor(id) => drop(desired.remove_executor(&id)),
                other => panic!("unexpected cleanup mutation: {other:?}"),
            }
        }
    }

    #[test]
    fn the_old_ids_records_go_once_the_new_id_is_live() {
        let mut desired = robot_state();
        let mutations = legacy_removals(&desired, LEGACY_NODE_ID, NEW, &[]);
        assert_eq!(mutations.len(), 3, "the old camera, provider and executor");
        assert!(mutations.iter().any(|m| matches!(m, DesiredStateMutation::RemoveResource(id) if id.as_str() == "capture_device_node-local_camera-2c206880")));
        assert!(mutations.iter().any(|m| matches!(m, DesiredStateMutation::RemoveProvider(id) if id.as_str() == "provider.peripherals.node-local")));
        assert!(mutations.iter().any(|m| matches!(m, DesiredStateMutation::RemoveExecutor(id) if id.as_str() == "executor.peripherals.node-local")));

        apply_plain(&mut desired, mutations);
        assert!(legacy_removals(&desired, LEGACY_NODE_ID, NEW, &[]).is_empty(), "a second pass finds nothing");
        assert!(!legacy_left(&desired, LEGACY_NODE_ID));
        assert!(desired.resources.contains_key(&ResourceId::new("capture_device_raze-2c206880_camera-2c206880")), "the new camera stays");
        assert!(desired.resources.contains_key(&ResourceId::new("capture_device_systemcore-3c99_camera-3c99")), "a peer's camera stays");
        assert!(desired.providers.contains_key(&ProviderId::new("provider.peripherals.systemcore-3c99")), "a peer's provider stays");
    }

    #[test]
    fn nothing_is_removed_before_the_new_id_is_live() {
        let mut desired = robot_state();
        desired.providers.remove(&ProviderId::new("provider.peripherals.raze-2c206880"));
        assert!(legacy_removals(&desired, LEGACY_NODE_ID, NEW, &[]).is_empty());
        assert!(legacy_left(&desired, LEGACY_NODE_ID), "the old records wait for the new ones");
    }

    #[test]
    fn a_peer_still_on_the_old_id_keeps_its_records() {
        let desired = robot_state();
        assert!(legacy_removals(&desired, LEGACY_NODE_ID, NEW, &[LEGACY_NODE_ID.to_string()]).is_empty(), "another board on the old id is not this board's");
        assert!(legacy_removals(&desired, LEGACY_NODE_ID, NEW, &["systemcore-3c99".to_string()]).len() == 3, "other peers do not block it");
    }

    #[test]
    fn the_same_or_no_old_id_removes_nothing() {
        let desired = robot_state();
        assert!(legacy_removals(&desired, NEW, NEW, &[]).is_empty());
        assert_eq!(legacy_removals(&desired, "systemcore-3c99", NEW, &[]).len(), 2, "its provider and camera: whichever id is the old one");
    }
}
