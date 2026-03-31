use axum::response::IntoResponse;
use once_cell::sync::Lazy;
use reqwest::header::ACCEPT;

use crate::http::streams::util::camera_id_for_manifest;
use crate::http::{AppState, peers, streams_persist};

use super::{RigPose, UpdateCameraPoseRequest, state::camera_uid_from_keys};

static PEER_RIG_HTTP: Lazy<reqwest::Client> = Lazy::new(|| crate::http::reqwest_client::build_http_client("HeliOS/rig-sync").expect("reqwest client"));

pub(super) fn encode_path_segment(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>().replace('+', "%20")
}

pub(super) async fn forward_peer_camera_pose(state: &AppState, peer_id: &str, remote_camera_uid: &str, req: Option<UpdateCameraPoseRequest>) -> axum::response::Response {
    let peers = peers::snapshot_peers(state).await;
    let Some(peer) = peers.into_iter().find(|peer| peer.id == peer_id) else {
        return (axum::http::StatusCode::NOT_FOUND, axum::Json(crate::http::error::ErrorBody::new("not_found", "peer not found"))).into_response();
    };
    if !matches!(peer.integration.kind, peers::PeerIntegrationKind::Helios) {
        return (axum::http::StatusCode::BAD_REQUEST, axum::Json(crate::http::error::ErrorBody::new("bad_request", "camera pose forwarding is only available for helios peers"))).into_response();
    }

    let encoded_uid = encode_path_segment(remote_camera_uid);
    let path = format!("/device/cameras/{encoded_uid}/pose");
    let url = match peers::peer_v1_url(&peer, &path) {
        Ok(url) => url,
        Err(err) => return (axum::http::StatusCode::BAD_REQUEST, axum::Json(crate::http::error::ErrorBody::new("bad_request", err))).into_response(),
    };

    let response = if let Some(payload) = req {
        PEER_RIG_HTTP.put(url).header(ACCEPT, "application/json").json(&payload).timeout(std::time::Duration::from_millis(1800)).send().await
    } else {
        PEER_RIG_HTTP.delete(url).header(ACCEPT, "application/json").timeout(std::time::Duration::from_millis(1800)).send().await
    };

    let Ok(response) = response else {
        return (axum::http::StatusCode::BAD_GATEWAY, axum::Json(crate::http::error::ErrorBody::new("bad_gateway", "peer request failed"))).into_response();
    };
    if response.status().is_success() {
        return axum::http::StatusCode::NO_CONTENT.into_response();
    }
    if response.status().as_u16() == 404 {
        return (axum::http::StatusCode::NOT_FOUND, axum::Json(crate::http::error::ErrorBody::new("not_found", "peer camera not found"))).into_response();
    }

    let detail = response.text().await.unwrap_or_else(|_| "peer camera pose request failed".to_string());
    (axum::http::StatusCode::BAD_GATEWAY, axum::Json(crate::http::error::ErrorBody::new("bad_gateway", detail))).into_response()
}

pub(super) async fn update_running_stream_pose(state: &AppState, camera_uid: &str, pose: Option<RigPose>) -> Result<bool, String> {
    let streams = state.engine.list_streams().await.map_err(|err| err.to_string())?;
    for stream in streams {
        let fallback = stream.manifest.identity.alias.as_deref().or(stream.manifest.identity.hardware_id.as_deref());
        let stream_uid = camera_uid_from_keys(&stream.manifest.capture.device_keys, fallback).unwrap_or_else(|| stream.stream_id.to_string());
        if stream_uid != camera_uid {
            continue;
        }
        let mut manifest = stream.manifest.clone();
        manifest.pose = pose.clone();
        streams_persist::persist_manifest_checked(&camera_id_for_manifest(&manifest), Some(stream.stream_id), manifest)
            .await
            .map_err(|err| format!("updated live stream pose but failed to persist: {err}"))?;
        return Ok(true);
    }
    Ok(false)
}
