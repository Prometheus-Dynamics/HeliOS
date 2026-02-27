use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::path::{Path as StdPath, PathBuf};
use tokio::process::Command;
use tokio_util::io::ReaderStream;
use tracing::info;
use utoipa::ToSchema;

use crate::http::AppState;

const DIAGNOSTICS_DIR: &str = "/var/lib/helios/diagnostics";

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DeviceSnapshotsResponse {
    pub snapshots: Vec<DeviceSnapshotResponse>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DeviceSnapshotResponse {
    pub id: String,
    pub label: String,
    pub created_by: String,
    pub created_at: String,
    pub size_bytes: u64,
    pub status: String,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CaptureSnapshotRequest {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub requested_by: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RequestedByQuery {
    #[serde(default)]
    pub requested_by: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SnapshotMeta {
    id: String,
    created_utc: String,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    tag: Option<String>,
}

fn diagnostics_root() -> PathBuf {
    PathBuf::from(DIAGNOSTICS_DIR)
}

fn snapshot_dir(root: &StdPath, id: &str) -> PathBuf {
    root.join(id)
}

fn snapshot_tar(root: &StdPath, id: &str) -> PathBuf {
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

async fn snapshot_response(root: &StdPath, id: &str) -> Option<DeviceSnapshotResponse> {
    let dir = snapshot_dir(root, id);
    let meta = read_snapshot_meta(&dir).await?;
    let created_by_fallback = meta.host.clone().unwrap_or_else(|| "unknown".to_string());
    let created_by = read_created_by(&dir, &created_by_fallback).await;
    let label = meta.tag.clone().unwrap_or_else(|| meta.id.clone());
    let tar = snapshot_tar(root, id);
    let size_bytes = if tokio::fs::try_exists(&tar).await.unwrap_or(false) {
        file_size(&tar).await
    } else {
        // Fallback: sum just the meta size; diagnostics normally produces tarballs.
        file_size(&dir.join("meta.json")).await
    };
    Some(DeviceSnapshotResponse { id: meta.id, label, created_by, created_at: meta.created_utc, size_bytes, status: "ready".to_string() })
}

async fn list_snapshots_impl() -> Vec<DeviceSnapshotResponse> {
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

#[utoipa::path(
    get,
    path = "/device/snapshots",
    tag = "Device",
    responses((status = 200, description = "List diagnostics snapshots", body = DeviceSnapshotsResponse))
)]
pub async fn list_snapshots(State(_state): State<AppState>) -> impl IntoResponse {
    Json(DeviceSnapshotsResponse { snapshots: list_snapshots_impl().await })
}

#[utoipa::path(
    post,
    path = "/device/snapshots",
    tag = "Device",
    request_body = CaptureSnapshotRequest,
    responses((status = 200, description = "Snapshot captured", body = DeviceSnapshotResponse))
)]
pub async fn capture_snapshot(State(_state): State<AppState>, Json(req): Json<CaptureSnapshotRequest>) -> Response {
    let mut cmd = Command::new("helios-diagnostics");
    cmd.arg("--trigger").arg("manual").arg("--tar");
    if let Some(label) = req.label.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        cmd.arg("--tag").arg(label);
    }

    let status = match tokio::time::timeout(std::time::Duration::from_secs(120), cmd.status()).await {
        Ok(Ok(status)) => status,
        Ok(Err(err)) => return (StatusCode::BAD_GATEWAY, format!("failed to run helios-diagnostics: {err}")).into_response(),
        Err(_) => return (StatusCode::GATEWAY_TIMEOUT, "helios-diagnostics timed out").into_response(),
    };
    if !status.success() {
        return (StatusCode::BAD_GATEWAY, format!("helios-diagnostics exited with {status}")).into_response();
    }

    let root = diagnostics_root();
    let latest = tokio::fs::read_to_string(root.join("latest.txt")).await.ok().unwrap_or_default();
    let latest = latest.trim();
    let id = if latest.ends_with(".tar.gz") {
        PathBuf::from(latest).file_name().and_then(|s| s.to_str()).unwrap_or("").trim_end_matches(".tar.gz").to_string()
    } else {
        PathBuf::from(latest).file_name().and_then(|s| s.to_str()).unwrap_or("").to_string()
    };

    if !id.is_empty() {
        let dir = snapshot_dir(&root, &id);
        if tokio::fs::try_exists(&dir).await.unwrap_or(false) {
            let created_by = req.requested_by.as_deref().unwrap_or("ui").trim();
            let _ = tokio::fs::write(dir.join("created_by.txt"), format!("{created_by}\n")).await;
        }
    }

    if let Some(snapshot) = snapshot_response(&root, &id).await {
        return Json(snapshot).into_response();
    }
    (StatusCode::BAD_GATEWAY, "snapshot created but could not be indexed").into_response()
}

#[utoipa::path(
    delete,
    path = "/device/snapshots/{id}",
    tag = "Device",
    params(("id" = String, Path, description = "Snapshot ID")),
    responses((status = 204, description = "Snapshot deleted"))
)]
pub async fn delete_snapshot(State(_state): State<AppState>, Path(id): Path<String>, Query(q): Query<RequestedByQuery>) -> impl IntoResponse {
    if let Some(requested_by) = q.requested_by.as_deref() {
        info!(%id, %requested_by, "snapshot deletion requested");
    }
    let root = diagnostics_root();
    let tar = snapshot_tar(&root, &id);
    let dir = snapshot_dir(&root, &id);
    let _ = tokio::fs::remove_file(tar).await;
    let _ = tokio::fs::remove_dir_all(dir).await;
    StatusCode::NO_CONTENT
}

#[utoipa::path(
    get,
    path = "/device/snapshots/{id}/download",
    operation_id = "device_snapshots_download",
    tag = "Device",
    params(("id" = String, Path, description = "Snapshot ID")),
    responses((status = 200, description = "Snapshot archive"))
)]
pub async fn download_snapshot(State(_state): State<AppState>, Path(id): Path<String>) -> Response {
    let root = diagnostics_root();
    let tar = snapshot_tar(&root, &id);
    if !tokio::fs::try_exists(&tar).await.unwrap_or(false) {
        return (StatusCode::NOT_FOUND, "snapshot archive not found").into_response();
    }

    let file = match tokio::fs::File::open(&tar).await {
        Ok(file) => file,
        Err(err) => return (StatusCode::BAD_GATEWAY, format!("failed to open archive: {err}")).into_response(),
    };
    let stream = ReaderStream::new(file);
    let body = axum::body::Body::from_stream(stream);
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "application/gzip".parse().unwrap());
    headers.insert(header::CONTENT_DISPOSITION, format!("attachment; filename=\"snapshot-{id}.tar.gz\"").parse().unwrap());
    (headers, body).into_response()
}
