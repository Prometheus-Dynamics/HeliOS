use std::path::{Path as StdPath, PathBuf};

use crate::http::device::snapshots::types::{DeviceSnapshotResponse, SnapshotMeta};

const DIAGNOSTICS_DIR: &str = "/var/lib/helios/diagnostics";

pub(super) fn diagnostics_root() -> PathBuf {
    PathBuf::from(DIAGNOSTICS_DIR)
}

pub(super) fn snapshot_dir(root: &StdPath, id: &str) -> PathBuf {
    root.join(id)
}

pub(super) fn snapshot_tar(root: &StdPath, id: &str) -> PathBuf {
    root.join(format!("{id}.tar.gz"))
}

async fn read_snapshot_meta(dir: &StdPath) -> Option<SnapshotMeta> {
    let bytes = tokio::fs::read(dir.join("meta.json")).await.ok()?;
    serde_json::from_slice::<SnapshotMeta>(&bytes).ok()
}

async fn read_created_by(dir: &StdPath, fallback: &str) -> String {
    match tokio::fs::read_to_string(dir.join("created_by.txt")).await {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() { fallback.to_string() } else { trimmed.to_string() }
        }
        Err(_) => fallback.to_string(),
    }
}

async fn file_size(path: &StdPath) -> u64 {
    tokio::fs::metadata(path).await.ok().map(|m| m.len()).unwrap_or(0)
}

pub(super) async fn snapshot_response(root: &StdPath, id: &str) -> Option<DeviceSnapshotResponse> {
    let dir = snapshot_dir(root, id);
    let meta = read_snapshot_meta(&dir).await?;
    let created_by_fallback = meta.host.clone().unwrap_or_else(|| "unknown".to_string());
    let created_by = read_created_by(&dir, &created_by_fallback).await;
    let label = meta.tag.clone().unwrap_or_else(|| meta.id.clone());
    let tar = snapshot_tar(root, id);
    let size_bytes = if tokio::fs::try_exists(&tar).await.unwrap_or(false) { file_size(&tar).await } else { file_size(&dir.join("meta.json")).await };
    Some(DeviceSnapshotResponse { id: meta.id, label, created_by, created_at: meta.created_utc, size_bytes, status: "ready".to_string() })
}

pub(super) async fn list_snapshots_impl() -> Vec<DeviceSnapshotResponse> {
    let root = diagnostics_root();
    let mut out = Vec::new();
    let mut rd = match tokio::fs::read_dir(&root).await {
        Ok(rd) => rd,
        Err(_) => return out,
    };

    while let Ok(Some(ent)) = rd.next_entry().await {
        let path = ent.path();
        if !path.is_dir() {
            continue;
        }
        let name = ent.file_name().to_string_lossy().to_string();
        if name == "latest.txt" || name.starts_with('.') {
            continue;
        }
        if let Some(snapshot) = snapshot_response(&root, &name).await {
            out.push(snapshot);
        }
    }

    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    out
}

pub(super) fn latest_snapshot_id(raw: &str) -> String {
    let latest = raw.trim();
    if latest.ends_with(".tar.gz") {
        PathBuf::from(latest).file_name().and_then(|s| s.to_str()).unwrap_or("").trim_end_matches(".tar.gz").to_string()
    } else {
        PathBuf::from(latest).file_name().and_then(|s| s.to_str()).unwrap_or("").to_string()
    }
}
