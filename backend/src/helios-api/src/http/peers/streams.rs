use axum::{
    Json,
    body::Body,
    extract::{Path, RawQuery, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use reqwest::header::{ACCEPT, RANGE};
use serde::de::DeserializeOwned;
use std::collections::{BTreeSet, HashMap};
use url::Url;
use uuid::Uuid;

use crate::http::AppState;
use crate::http::device::rig::{CameraLayoutCameraResponse, CameraLayoutResponse};
use crate::http::pipelines::{self, PipelineDocument, PipelineSummary};
use crate::http::streams::types::StreamInfo;
use helios_engine::localization::types::LocalizationPipelineSource;

use super::state::snapshot_peers;
use super::types::*;

pub(super) const PEER_JSON_TIMEOUT_MS: u64 = 1800;

pub(crate) fn parse_peer_scoped_ref(value: &str) -> Option<(String, String)> {
    let trimmed = value.trim();
    let raw = trimmed.strip_prefix("peer:")?;
    let mut parts = raw.splitn(2, ':');
    let peer_id = parts.next()?.trim();
    let remote_id = parts.next()?.trim();
    if peer_id.is_empty() || remote_id.is_empty() {
        return None;
    }
    Some((peer_id.to_string(), remote_id.to_string()))
}

pub(crate) fn peer_v1_url(peer: &PeerInfo, path: &str) -> Result<String, String> {
    build_peer_v1_url(peer.api_base_url.as_str(), path)
}

#[utoipa::path(
    get,
    path = "/peers/streams",
    tag = "Peers",
    responses((status = 200, description = "Aggregated remote Helios stream inventory", body = PeerRemoteStreamsResponse))
)]
pub(crate) async fn list_peer_streams(State(state): State<AppState>) -> impl IntoResponse {
    Json(super::state::snapshot_peer_streams(&state).await)
}

#[utoipa::path(
    get,
    path = "/peers/{id}/streams",
    tag = "Peers",
    params(("id" = String, Path, description = "Peer identifier")),
    responses(
        (status = 200, description = "Remote Helios stream inventory for one peer", body = PeerRemoteStreamsResponse),
        (status = 404, description = "Peer not found", body = PeerError),
        (status = 400, description = "Unsupported peer type", body = PeerError)
    )
)]
pub(crate) async fn list_peer_streams_for_peer(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let peers = snapshot_peers(&state).await;
    let Some(peer) = peers.into_iter().find(|peer| peer.id == id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(PeerError {
                error: "peer not found".to_string(),
            }),
        )
            .into_response();
    };
    if !matches!(peer.integration.kind, PeerIntegrationKind::Helios) {
        return (
            StatusCode::BAD_REQUEST,
            Json(PeerError {
                error: "stream inventory is only available for helios peers".to_string(),
            }),
        )
            .into_response();
    }

    match collect_peer_streams_for_peer(&state, &peer).await {
        Ok(mut streams) => {
            streams.sort_by(|a, b| {
                a.display_name
                    .cmp(&b.display_name)
                    .then_with(|| a.remote_stream_id.cmp(&b.remote_stream_id))
            });
            Json(PeerRemoteStreamsResponse {
                streams,
                errors: Vec::new(),
                fetched_at: Utc::now().to_rfc3339(),
            })
                .into_response()
        }
        Err(err) => Json(PeerRemoteStreamsResponse {
            streams: Vec::new(),
            errors: vec![PeerResourceError {
                peer_id: peer.id,
                peer_alias: peer.alias,
                error: err,
            }],
            fetched_at: Utc::now().to_rfc3339(),
        })
        .into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/peers/{id}/streams/{stream_id}/format",
    tag = "Peers",
    params(
        ("id" = String, Path, description = "Peer identifier"),
        ("stream_id" = String, Path, description = "Remote stream id")
    ),
    responses((status = 200, description = "Proxied remote format response"))
)]
pub(crate) async fn proxy_peer_stream_format(
    State(state): State<AppState>,
    Path((id, stream_id)): Path<(String, String)>,
    raw_query: RawQuery,
    headers: HeaderMap,
) -> impl IntoResponse {
    proxy_peer_stream_resource(&state, id, stream_id, "format", raw_query.0, headers).await
}

#[utoipa::path(
    get,
    path = "/peers/{id}/streams/{stream_id}/preview",
    tag = "Peers",
    params(
        ("id" = String, Path, description = "Peer identifier"),
        ("stream_id" = String, Path, description = "Remote stream id")
    ),
    responses((status = 200, description = "Proxied remote preview response"))
)]
pub(crate) async fn proxy_peer_stream_preview(
    State(state): State<AppState>,
    Path((id, stream_id)): Path<(String, String)>,
    raw_query: RawQuery,
    headers: HeaderMap,
) -> impl IntoResponse {
    proxy_peer_stream_resource(&state, id, stream_id, "preview", raw_query.0, headers).await
}

#[utoipa::path(
    get,
    path = "/peers/{id}/streams/{stream_id}/frame",
    tag = "Peers",
    params(
        ("id" = String, Path, description = "Peer identifier"),
        ("stream_id" = String, Path, description = "Remote stream id")
    ),
    responses((status = 200, description = "Proxied remote frame response"))
)]
pub(crate) async fn proxy_peer_stream_frame(
    State(state): State<AppState>,
    Path((id, stream_id)): Path<(String, String)>,
    raw_query: RawQuery,
    headers: HeaderMap,
) -> impl IntoResponse {
    proxy_peer_stream_resource(&state, id, stream_id, "frame", raw_query.0, headers).await
}

#[utoipa::path(
    post,
    path = "/peers/{id}/pipelines/sync",
    tag = "Peers",
    params(("id" = String, Path, description = "Peer identifier")),
    request_body = PeerPipelineSyncRequest,
    responses(
        (status = 200, description = "Peer pipelines synchronized into local storage", body = PeerPipelineSyncResponse),
        (status = 404, description = "Peer not found", body = PeerError),
        (status = 400, description = "Unsupported peer type", body = PeerError)
    )
)]
pub(crate) async fn sync_peer_pipelines(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<PeerPipelineSyncRequest>,
) -> impl IntoResponse {
    let peers = snapshot_peers(&state).await;
    let Some(peer) = peers.into_iter().find(|peer| peer.id == id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(PeerError {
                error: "peer not found".to_string(),
            }),
        )
            .into_response();
    };
    if !matches!(peer.integration.kind, PeerIntegrationKind::Helios) {
        return (
            StatusCode::BAD_REQUEST,
            Json(PeerError {
                error: "pipeline sync is only available for helios peers".to_string(),
            }),
        )
            .into_response();
    }

    let force = req.force.unwrap_or(false);
    let mut errors = Vec::new();
    let remote_summaries: Vec<PipelineSummary> =
        match fetch_peer_json(&state, &peer, "/pipelines/graphs", 2500).await {
            Ok(value) => value,
            Err(err) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(PeerPipelineSyncResponse {
                        peer_id: peer.id,
                        peer_alias: peer.alias,
                        synced: Vec::new(),
                        errors: vec![err],
                    }),
                )
                    .into_response();
            }
        };

    let requested: BTreeSet<String> = req
        .pipeline_ids
        .iter()
        .filter_map(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect();

    let summaries_by_id: HashMap<String, (Uuid, Option<String>)> = remote_summaries
        .iter()
        .map(|summary| (summary.id.to_string(), (summary.id, summary.name.clone())))
        .collect();
    for requested_id in &requested {
        if !summaries_by_id.contains_key(requested_id) {
            errors.push(format!("remote pipeline not found: {requested_id}"));
        }
    }

    let mut selected: Vec<(Uuid, Option<String>)> = if requested.is_empty() {
        remote_summaries
            .into_iter()
            .map(|summary| (summary.id, summary.name))
            .collect()
    } else {
        requested
            .iter()
            .filter_map(|id| summaries_by_id.get(id).cloned())
            .collect()
    };
    selected.sort_by(|a, b| {
        a.1.as_deref()
            .unwrap_or_default()
            .cmp(b.1.as_deref().unwrap_or_default())
            .then_with(|| a.0.cmp(&b.0))
    });

    let peer_label = peer.alias.clone().unwrap_or_else(|| peer.id.clone());
    let mut synced = Vec::new();
    for (remote_pipeline_id, summary_name) in selected {
        let path = format!("/pipelines/graphs/{remote_pipeline_id}");
        let remote_doc: PipelineDocument =
            match fetch_peer_json(&state, &peer, &path, 4500).await {
                Ok(doc) => doc,
                Err(err) => {
                    errors.push(format!("{}: {err}", remote_pipeline_id));
                    continue;
                }
            };

        let local_pipeline_id = synced_pipeline_local_id(peer.id.as_str(), remote_pipeline_id);
        let remote_name = remote_doc.name.clone().or(summary_name);
        let local_name = remote_name
            .as_ref()
            .map(|name| format!("{peer_label} · {name}"))
            .or_else(|| Some(format!("{peer_label} · {remote_pipeline_id}")));

        match upsert_synced_pipeline(
            &state,
            local_pipeline_id,
            local_name.clone(),
            remote_doc.graph,
            remote_doc.updated_at_ms,
            force,
        )
        .await
        {
            Ok((updated, refresh_failures)) => {
                for failure in refresh_failures {
                    errors.push(failure);
                }
                synced.push(PeerPipelineSyncItem {
                    remote_pipeline_id: remote_pipeline_id.to_string(),
                    local_pipeline_id: local_pipeline_id.to_string(),
                    name: local_name,
                    updated,
                });
            }
            Err(err) => errors.push(format!("{}: {err}", remote_pipeline_id)),
        }
    }

    Json(PeerPipelineSyncResponse {
        peer_id: peer.id,
        peer_alias: peer.alias,
        synced,
        errors,
    })
        .into_response()
}

async fn proxy_peer_stream_resource(
    state: &AppState,
    peer_id: String,
    stream_id: String,
    endpoint: &str,
    raw_query: Option<String>,
    request_headers: HeaderMap,
) -> Response {
    let peers = snapshot_peers(state).await;
    let Some(peer) = peers.into_iter().find(|peer| peer.id == peer_id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(PeerError {
                error: "peer not found".to_string(),
            }),
        )
            .into_response();
    };
    if !matches!(peer.integration.kind, PeerIntegrationKind::Helios) {
        return (
            StatusCode::BAD_REQUEST,
            Json(PeerError {
                error: "stream proxy is only available for helios peers".to_string(),
            }),
        )
            .into_response();
    }

    let mut upstream_url = match peer_v1_url(&peer, format!("/streams/{stream_id}/{endpoint}").as_str()) {
        Ok(url) => url,
        Err(err) => {
            return (StatusCode::BAD_REQUEST, Json(PeerError { error: err })).into_response();
        }
    };
    if let Some(query) = raw_query && !query.trim().is_empty() {
        upstream_url.push('?');
        upstream_url.push_str(query.as_str());
    }

    let mut request = state.services.peers.inner().http_client().get(upstream_url);
    if endpoint != "preview" {
        request = request.timeout(std::time::Duration::from_millis(PEER_JSON_TIMEOUT_MS));
    }
    if let Some(value) = request_headers.get(header::ACCEPT) {
        request = request.header(header::ACCEPT, value.clone());
    } else if endpoint == "preview" {
        request = request.header(
            ACCEPT,
            "multipart/x-mixed-replace, image/jpeg, application/octet-stream, */*;q=0.1",
        );
    } else {
        request = request.header(ACCEPT, "application/json, image/jpeg, */*;q=0.1");
    }
    if let Some(range) = request_headers.get(header::RANGE) {
        request = request.header(RANGE, range.clone());
    }

    let upstream = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(PeerError {
                    error: format!("peer stream request failed: {err}"),
                }),
            )
                .into_response();
        }
    };

    let status = StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let upstream_headers = upstream.headers().clone();
    let mut response = Response::new(Body::from_stream(upstream.bytes_stream()));
    *response.status_mut() = status;

    for name in [
        header::CONTENT_TYPE,
        header::CACHE_CONTROL,
        header::PRAGMA,
        header::CONTENT_LENGTH,
        header::CONTENT_DISPOSITION,
        header::ETAG,
        header::LAST_MODIFIED,
    ] {
        if let Some(value) = upstream_headers.get(&name) {
            response.headers_mut().insert(name, value.clone());
        }
    }
    for name in [
        "x-encoded-fourcc",
        "x-encoded-format",
        "x-encoded-width",
        "x-encoded-height",
    ] {
        if let Some(value) = upstream_headers.get(name)
            && let Ok(header_name) = header::HeaderName::from_bytes(name.as_bytes())
        {
            response.headers_mut().insert(header_name, value.clone());
        }
    }

    response
}

pub(super) async fn collect_peer_stream_inventory(
    state: &AppState,
    peers: &[PeerInfo],
) -> (Vec<PeerRemoteStreamSummary>, Vec<PeerResourceError>) {
    let mut streams = Vec::new();
    let mut errors = Vec::new();
    for peer in peers {
        if !matches!(peer.integration.kind, PeerIntegrationKind::Helios) {
            continue;
        }
        match collect_peer_streams_for_peer(state, peer).await {
            Ok(mut peer_streams) => streams.append(&mut peer_streams),
            Err(err) => errors.push(PeerResourceError {
                peer_id: peer.id.clone(),
                peer_alias: peer.alias.clone(),
                error: err,
            }),
        }
    }

    streams.sort_by(|a, b| {
        let a_peer = a.peer_alias.as_deref().unwrap_or(a.peer_id.as_str());
        let b_peer = b.peer_alias.as_deref().unwrap_or(b.peer_id.as_str());
        a_peer
            .cmp(b_peer)
            .then_with(|| a.display_name.cmp(&b.display_name))
            .then_with(|| a.remote_stream_id.cmp(&b.remote_stream_id))
    });
    errors.sort_by(|a, b| a.peer_id.cmp(&b.peer_id));
    (streams, errors)
}

async fn collect_peer_streams_for_peer(
    state: &AppState,
    peer: &PeerInfo,
) -> Result<Vec<PeerRemoteStreamSummary>, String> {
    let remote_streams: Vec<StreamInfo> =
        fetch_peer_json(state, peer, "/streams", PEER_JSON_TIMEOUT_MS).await?;
    let remote_sources: Vec<LocalizationPipelineSource> =
        fetch_peer_json(state, peer, "/localization/sources", PEER_JSON_TIMEOUT_MS)
            .await
            .unwrap_or_default();
    let remote_layout: Option<CameraLayoutResponse> =
        fetch_peer_json(state, peer, "/device/camera-layout", PEER_JSON_TIMEOUT_MS)
            .await
            .ok();

    let mut outputs_by_stream: HashMap<String, Vec<PeerStreamOutputSummary>> = HashMap::new();
    for source in remote_sources {
        let stream_id = source.stream_id.trim();
        let output_key = source.output_key.trim();
        if stream_id.is_empty() || output_key.is_empty() {
            continue;
        }
        let entry = outputs_by_stream.entry(stream_id.to_string()).or_default();
        if entry.iter().any(|value| value.output_key == output_key) {
            continue;
        }
        entry.push(PeerStreamOutputSummary {
            output_key: output_key.to_string(),
            data_type: source.data_type.clone(),
        });
    }
    for values in outputs_by_stream.values_mut() {
        values.sort_by(|a, b| a.output_key.cmp(&b.output_key));
    }

    let mut cameras_by_stream_id = HashMap::<String, CameraLayoutCameraResponse>::new();
    let mut cameras_by_camera_uid = HashMap::<String, CameraLayoutCameraResponse>::new();
    if let Some(layout) = remote_layout {
        for camera in layout.cameras {
            if let Some(stream_id) = camera
                .stream_id
                .clone()
                .filter(|value| !value.trim().is_empty())
            {
                cameras_by_stream_id
                    .entry(stream_id)
                    .or_insert_with(|| camera.clone());
            }
            if let Some(camera_uid) = camera
                .camera_uid
                .clone()
                .filter(|value| !value.trim().is_empty())
            {
                cameras_by_camera_uid
                    .entry(camera_uid)
                    .or_insert_with(|| camera.clone());
            }
        }
    }

    let mut summaries = Vec::new();
    for stream in remote_streams {
        let remote_stream_id = stream.id.to_string();
        let fallback_camera_uid = stream
            .manifest
            .identity
            .alias
            .as_deref()
            .or(stream.manifest.identity.hardware_id.as_deref());
        let inferred_camera_uid = crate::http::device::rig::camera_uid_from_keys(
            &stream.manifest.capture.device_keys,
            fallback_camera_uid,
        )
        .unwrap_or_else(|| remote_stream_id.clone());
        let matched_camera = cameras_by_stream_id
            .get(&remote_stream_id)
            .cloned()
            .or_else(|| cameras_by_camera_uid.get(&inferred_camera_uid).cloned());
        let remote_camera_uid = matched_camera
            .as_ref()
            .and_then(|camera| camera.camera_uid.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(inferred_camera_uid);
        let scoped_camera_uid = format!("peer:{}:{}", peer.id, remote_camera_uid);
        let stream_alias = stream
            .manifest
            .identity
            .alias
            .clone()
            .filter(|value| !value.trim().is_empty());
        let display_name = matched_camera
            .as_ref()
            .map(|camera| camera.display_name.clone())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| stream_alias.clone())
            .or_else(|| stream.manifest.identity.hardware_id.clone())
            .unwrap_or_else(|| remote_stream_id.clone());
        let pose = matched_camera
            .as_ref()
            .and_then(|camera| camera.pose.clone())
            .or(stream.manifest.pose.clone())
            .map(|pose| PeerRemoteRigPose {
                translation: PeerRemotePoseVector {
                    x: pose.translation.x,
                    y: pose.translation.y,
                    z: pose.translation.z,
                },
                rotation: PeerRemotePoseRotation {
                    roll: pose.rotation.roll,
                    pitch: pose.rotation.pitch,
                    yaw: pose.rotation.yaw,
                },
                updated_at: pose.updated_at,
            });

        let mut outputs = outputs_by_stream.remove(&remote_stream_id).unwrap_or_default();
        outputs.sort_by(|a, b| a.output_key.cmp(&b.output_key));
        let imu_output_keys = outputs
            .iter()
            .filter(|output| {
                let key = output.output_key.to_ascii_lowercase();
                key.contains("imu")
                    || output
                        .data_type
                        .as_ref()
                        .and_then(|value| serde_json::to_string(value).ok())
                        .is_some_and(|text| text.to_ascii_lowercase().contains("imu"))
            })
            .map(|output| output.output_key.clone())
            .collect::<Vec<_>>();

        let base_proxy = format!("/v1/peers/{}/streams/{}", peer.id, remote_stream_id);
        let state = stream.status.as_ref().map(|status| match status.state {
            helios_engine::ipc::StreamState::Running => "running".to_string(),
            helios_engine::ipc::StreamState::Disabled => "disabled".to_string(),
        });
        summaries.push(PeerRemoteStreamSummary {
            peer_id: peer.id.clone(),
            peer_alias: peer.alias.clone(),
            peer_kind: peer.integration.kind.clone(),
            stream_ref: format!("peer:{}:{}", peer.id, remote_stream_id),
            remote_stream_id,
            stream_alias,
            display_name: Some(display_name),
            backend: Some(format!("{:?}", stream.manifest.capture.backend).to_ascii_lowercase()),
            state,
            active_pipeline_id: stream.manifest.active_pipeline_id.map(|value| value.to_string()),
            active_pipeline_output: stream.manifest.active_pipeline_output.clone(),
            camera_uid: Some(scoped_camera_uid),
            pose,
            outputs,
            imu_output_keys,
            proxy_preview_url: format!("{base_proxy}/preview"),
            proxy_frame_url: format!("{base_proxy}/frame"),
            proxy_format_url: format!("{base_proxy}/format"),
        });
    }

    Ok(summaries)
}

async fn fetch_peer_json<T: DeserializeOwned>(
    state: &AppState,
    peer: &PeerInfo,
    path: &str,
    timeout_ms: u64,
) -> Result<T, String> {
    let url = peer_v1_url(peer, path)?;
    let resp = state
        .services
        .peers
        .inner()
        .http_client()
        .get(url)
        .header(ACCEPT, "application/json")
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .send()
        .await
        .map_err(|err| format!("peer request failed: {err}"))?;
    if !resp.status().is_success() {
        return Err(format!("peer returned {}", resp.status()));
    }
    resp.json::<T>()
        .await
        .map_err(|err| format!("invalid json: {err}"))
}

fn build_peer_v1_url(base: &str, path: &str) -> Result<String, String> {
    let base = base.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("peer api_base_url missing".to_string());
    }
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    let candidate = if base.ends_with("/v1") {
        format!("{base}{path}")
    } else {
        format!("{base}/v1{path}")
    };
    Url::parse(&candidate)
        .map(|url| url.to_string())
        .map_err(|err| format!("invalid peer url: {err}"))
}

fn synced_pipeline_local_id(peer_id: &str, remote_pipeline_id: Uuid) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("helios:peer-pipeline:{peer_id}:{remote_pipeline_id}").as_bytes(),
    )
}

async fn upsert_synced_pipeline(
    state: &AppState,
    local_pipeline_id: Uuid,
    local_name: Option<String>,
    graph: serde_json::Value,
    remote_updated_at_ms: i64,
    force: bool,
) -> Result<(bool, Vec<String>), String> {
    let dir = crate::http::storage::ensure_subdir_async("pipelines")
        .await
        .map_err(|err| err.to_string())?;
    let path = dir.join(format!("{local_pipeline_id}.json"));
    if let Ok(data) = tokio::fs::read(&path).await
        && let Ok(existing) = serde_json::from_slice::<PipelineDocument>(&data)
    {
        if !force && remote_updated_at_ms > 0 && existing.updated_at_ms >= remote_updated_at_ms {
            return Ok((false, Vec::new()));
        }
        if !force && existing.graph == graph && existing.name == local_name {
            return Ok((false, Vec::new()));
        }
    }

    let updated_at_ms = if remote_updated_at_ms > 0 {
        remote_updated_at_ms
    } else {
        Utc::now().timestamp_millis()
    };
    let doc = PipelineDocument {
        id: local_pipeline_id,
        name: local_name,
        graph,
        updated_at_ms,
    };
    let data = serde_json::to_vec_pretty(&doc)
        .map_err(|err| format!("failed to serialize pipeline: {err}"))?;
    tokio::fs::write(&path, data)
        .await
        .map_err(|err| format!("failed to persist pipeline: {err}"))?;

    pipelines::refresh_graph_validation(state, local_pipeline_id, &doc.graph).await;
    let refresh_failures = pipelines::refresh_pipeline_consumers(state, local_pipeline_id, &doc.graph)
        .await
        .into_iter()
        .map(|failure| format!("{}: {}", failure.stream_id, failure.error))
        .collect::<Vec<_>>();
    Ok((true, refresh_failures))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_peer_scoped_ref_requires_both_segments() {
        assert_eq!(
            parse_peer_scoped_ref("peer:robot:camera"),
            Some(("robot".to_string(), "camera".to_string()))
        );
        assert_eq!(parse_peer_scoped_ref("peer:robot"), None);
        assert_eq!(parse_peer_scoped_ref("robot:camera"), None);
    }

    #[test]
    fn build_peer_v1_url_appends_v1_once() {
        assert_eq!(
            build_peer_v1_url("http://robot.local", "/streams").unwrap(),
            "http://robot.local/v1/streams"
        );
        assert_eq!(
            build_peer_v1_url("http://robot.local/v1", "/streams").unwrap(),
            "http://robot.local/v1/streams"
        );
    }

    #[test]
    fn build_peer_v1_url_rejects_empty_base() {
        assert_eq!(
            build_peer_v1_url("", "/streams").unwrap_err(),
            "peer api_base_url missing"
        );
    }
}
