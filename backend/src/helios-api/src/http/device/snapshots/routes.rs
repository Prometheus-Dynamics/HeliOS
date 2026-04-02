use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use tokio::process::Command;
use tokio_util::io::ReaderStream;
use tracing::info;

use crate::http::AppState;

use super::{
    storage::{diagnostics_root, latest_snapshot_id, list_snapshots_impl, snapshot_dir, snapshot_response, snapshot_tar},
    types::{CaptureSnapshotRequest, DeviceSnapshotResponse, DeviceSnapshotsResponse, RequestedByQuery},
};

#[utoipa::path(
    get,
    path = "/device/snapshots",
    operation_id = "device_snapshots_list",
    tag = "Device",
    responses((status = 200, description = "List diagnostics snapshots", body = DeviceSnapshotsResponse))
)]
pub async fn list_device_snapshots(State(_state): State<AppState>) -> impl IntoResponse {
    Json(DeviceSnapshotsResponse { snapshots: list_snapshots_impl().await })
}

#[utoipa::path(
    post,
    path = "/device/snapshots",
    operation_id = "device_snapshots_capture",
    tag = "Device",
    request_body = CaptureSnapshotRequest,
    responses((status = 200, description = "Snapshot captured", body = DeviceSnapshotResponse))
)]
pub async fn capture_device_snapshot(State(_state): State<AppState>, Json(req): Json<CaptureSnapshotRequest>) -> Response {
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
    let id = latest_snapshot_id(&latest);

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
    operation_id = "device_snapshots_delete",
    tag = "Device",
    params(("id" = String, Path, description = "Snapshot ID")),
    responses((status = 204, description = "Snapshot deleted"))
)]
pub async fn delete_device_snapshot(State(_state): State<AppState>, Path(id): Path<String>, Query(q): Query<RequestedByQuery>) -> impl IntoResponse {
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
pub async fn download_device_snapshot(State(_state): State<AppState>, Path(id): Path<String>) -> Response {
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
