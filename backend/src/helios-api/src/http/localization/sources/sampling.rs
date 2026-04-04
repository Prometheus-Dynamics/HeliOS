use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use super::ApiLocalizationSourceFetcher;
use super::profile::fetch_profile_output;
use super::sample_refresh::localization_stream_sample_needs_refresh;
use crate::http::streams::util::{engine_error_body, map_client_error};
use crate::http::{AppState, peers};
use helios_engine::ipc::{EngineErrorCode, EngineEvent};
use helios_engine::localization::types::PipelineOutputSample;

pub(super) async fn sample_output(State(state): State<AppState>, Path((id, output_key)): Path<(Uuid, String)>) -> axum::response::Response {
    match state.engine.get_cached_graph_output_sample_event(id, output_key.clone()).await {
        Ok(EngineEvent::GraphOutputSample { value, .. }) => Json(PipelineOutputSample { data_type: None, value: value.into() }).into_response(),
        Ok(EngineEvent::Nack { code, .. }) if code == EngineErrorCode::NotFound && output_key.eq_ignore_ascii_case(super::super::media_imu::MEDIA_IMU_OUTPUT_KEY) => {
            match super::super::media_imu::fetch_media_imu_sample_for_stream(&state, id, &output_key).await {
                Ok(sample) => Json(sample).into_response(),
                Err(err) => err.into_response(),
            }
        }
        Ok(EngineEvent::Nack { code, reason, .. }) => {
            let status = if code == EngineErrorCode::NotFound { StatusCode::NOT_FOUND } else { StatusCode::BAD_REQUEST };
            (status, Json(engine_error_body(Some(code), reason))).into_response()
        }
        Ok(_) => (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), "unexpected engine response"))).into_response(),
        Err(err) => map_client_error(err),
    }
}

pub(super) async fn sample_peer_output(State(state): State<AppState>, Path((id, output_key)): Path<(String, String)>) -> axum::response::Response {
    let (peer_id, camera) = super::super::peers::sources::parse_peer_stream_id(&id);
    let peers = peers::snapshot_peers(&state).await;
    let Some(peer) = peers.into_iter().find(|p| p.id == peer_id) else {
        return (StatusCode::NOT_FOUND, Json(engine_error_body(Some(EngineErrorCode::NotFound), "peer not found"))).into_response();
    };

    match super::super::peers::sources::fetch_peer_output_sample(&peer, camera.as_deref(), &output_key).await {
        Ok(sample) => Json(sample).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), err))).into_response(),
    }
}

pub(super) async fn sample_profile_output(State(state): State<AppState>, Path((id, output_key)): Path<(String, String)>) -> axum::response::Response {
    let fetcher = ApiLocalizationSourceFetcher::new(state);
    match fetch_profile_output(&fetcher, id.trim(), &output_key).await {
        Ok(value) => Json(PipelineOutputSample { data_type: None, value }).into_response(),
        Err(reason) => {
            let lower = reason.to_ascii_lowercase();
            let not_found = lower.contains("not found") || lower.contains("missing");
            let status = if not_found { StatusCode::NOT_FOUND } else { StatusCode::BAD_REQUEST };
            let code = if not_found { EngineErrorCode::NotFound } else { EngineErrorCode::InvalidInput };
            (status, Json(engine_error_body(Some(code), reason))).into_response()
        }
    }
}

pub(crate) async fn fetch_stream_output(state: &AppState, stream_id: &str, output_key: &str) -> Result<JsonValue, String> {
    let stream_uuid = Uuid::parse_str(stream_id).map_err(|_| "invalid stream id".to_string())?;
    let fresh = localization_stream_sample_needs_refresh(stream_uuid, output_key);
    let event = match state.engine.get_graph_output_sample_event_with_mode(stream_uuid, output_key.to_string(), fresh).await {
        Ok(event) => event,
        Err(err) => return Err(err.to_string()),
    };
    match event {
        EngineEvent::GraphOutputSample { value, .. } => Ok(value.into()),
        EngineEvent::Nack { code, .. } if code == EngineErrorCode::NotFound && output_key.eq_ignore_ascii_case(super::super::media_imu::MEDIA_IMU_OUTPUT_KEY) => {
            super::super::media_imu::fetch_media_imu_sample_for_stream(state, stream_uuid, output_key).await.map(|sample| sample.value).map_err(|err| err.to_string())
        }
        EngineEvent::Nack { reason, .. } => Err(reason),
        _ => Err("unexpected engine response".to_string()),
    }
}

pub(crate) async fn fetch_peer_output(state: &AppState, stream_id: &str, output_key: &str) -> Result<JsonValue, String> {
    let (peer_id, camera) = super::super::peers::sources::parse_peer_stream_id(stream_id.trim_start_matches("peer:"));
    let peers = peers::snapshot_peers(state).await;
    let Some(peer) = peers.into_iter().find(|peer| peer.id == peer_id) else {
        return Err("peer not found".to_string());
    };

    super::super::peers::sources::fetch_peer_output_value(&peer, camera.as_deref(), output_key).await
}
