use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use super::ApiLocalizationSourceFetcher;
use super::profile::fetch_profile_output;
use crate::http::streams::util::{engine_error_body, map_client_error};
use crate::http::{AppState, peers};
use helios_engine::ipc::{EngineErrorCode, EngineEvent};
use helios_engine::localization::types::PipelineOutputSample;

const LOCALIZATION_STREAM_SAMPLE_REFRESH_MS: u64 = 1_000;

#[derive(Default)]
struct LocalizationStreamSampleRefreshState {
    refreshed_at_ms: BTreeMap<String, u64>,
}

impl LocalizationStreamSampleRefreshState {
    fn needs_refresh(&mut self, stream_id: Uuid, output_key: &str, now_ms: u64) -> bool {
        let key = localization_stream_sample_key(stream_id, output_key);
        self.refreshed_at_ms.retain(|_, refreshed_at_ms| now_ms.saturating_sub(*refreshed_at_ms) <= LOCALIZATION_STREAM_SAMPLE_REFRESH_MS.saturating_mul(8));

        let needs_refresh = self.refreshed_at_ms.get(&key).copied().map(|refreshed_at_ms| now_ms.saturating_sub(refreshed_at_ms) >= LOCALIZATION_STREAM_SAMPLE_REFRESH_MS).unwrap_or(true);
        if needs_refresh {
            self.refreshed_at_ms.insert(key, now_ms);
        }
        needs_refresh
    }
}

fn localization_stream_sample_refreshes() -> &'static Mutex<LocalizationStreamSampleRefreshState> {
    static STATE: OnceLock<Mutex<LocalizationStreamSampleRefreshState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(LocalizationStreamSampleRefreshState::default()))
}

fn localization_stream_sample_key(stream_id: Uuid, output_key: &str) -> String {
    format!("{stream_id}:{}", output_key.trim().to_ascii_lowercase())
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|dur| dur.as_millis() as u64).unwrap_or(0)
}

fn localization_stream_sample_needs_refresh(stream_id: Uuid, output_key: &str) -> bool {
    localization_stream_sample_refreshes().lock().expect("localization stream sample refresh mutex poisoned").needs_refresh(stream_id, output_key, now_ms())
}

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

#[cfg(test)]
mod tests {
    use super::{LOCALIZATION_STREAM_SAMPLE_REFRESH_MS, LocalizationStreamSampleRefreshState, localization_stream_sample_key};
    use uuid::Uuid;

    #[test]
    fn repeated_sampling_is_throttled_per_stream_output_key() {
        let mut state = LocalizationStreamSampleRefreshState::default();
        let stream_id = Uuid::nil();

        assert!(state.needs_refresh(stream_id, "tag_poses", 1_000));
        assert!(!state.needs_refresh(stream_id, "TAG_POSES", 1_500));
        assert!(state.needs_refresh(stream_id, "tag_poses", 2_000));
        assert!(state.needs_refresh(stream_id, "other_output", 2_000));
    }

    #[test]
    fn repeated_sampling_prunes_stale_entries() {
        let mut state = LocalizationStreamSampleRefreshState::default();
        let old_stream = Uuid::nil();
        let fresh_stream = Uuid::from_u128(1);

        assert!(state.needs_refresh(old_stream, "tag_poses", 0));
        assert!(state.needs_refresh(fresh_stream, "tag_poses", LOCALIZATION_STREAM_SAMPLE_REFRESH_MS * 9));

        assert!(!state.refreshed_at_ms.contains_key(&localization_stream_sample_key(old_stream, "tag_poses")));
        assert!(state.refreshed_at_ms.contains_key(&localization_stream_sample_key(fresh_stream, "tag_poses")));
    }
}
