use futures::future::join_all;
use serde_json;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::sync::RwLock;

use super::super::sources::ApiLocalizationSourceFetcher;
use crate::http::AppState;
use helios_engine::ipc::{EngineEvent, LocalizationSolveRequest, LocalizationSolveSourceValue, StreamCalibration, StreamSummary};
use helios_engine::localization::config::LocalizationSourceConfig;
use helios_engine::localization::fetch::LocalizationSourceFetcher;
use helios_engine::localization::maps::FieldMapDocument;
use helios_engine::localization::math::{PoseTransform, transform_to_pose};
use helios_engine::localization::types::LocalizationSolveResponse;

#[derive(Clone)]
struct CachedLocalizationSolveResponse {
    signature: Vec<u8>,
    response: LocalizationSolveResponse,
}

#[derive(Default)]
pub(crate) struct LocalizationSolveCacheState {
    entries: RwLock<HashMap<String, CachedLocalizationSolveResponse>>,
    hits: AtomicU64,
    misses: AtomicU64,
    inserts: AtomicU64,
}

impl LocalizationSolveCacheState {
    pub(crate) async fn get_matching(&self, key: &str, signature: &[u8]) -> Option<LocalizationSolveResponse> {
        let response = self.entries.read().await.get(key).filter(|entry| entry.signature == signature).map(|entry| entry.response.clone());
        if response.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
        }
        response
    }

    pub(crate) async fn insert(&self, key: String, signature: Vec<u8>, response: LocalizationSolveResponse) {
        self.entries.write().await.insert(key, CachedLocalizationSolveResponse { signature, response });
        self.inserts.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) async fn snapshot(&self) -> crate::api_observability::LocalizationSolveCacheSnapshot {
        crate::api_observability::LocalizationSolveCacheSnapshot {
            entries: self.entries.read().await.len() as u64,
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            inserts: self.inserts.load(Ordering::Relaxed),
        }
    }
}

fn localization_solve_cache_key(profile_id: &str, apply_field_origin: bool) -> String {
    format!("{}|{}", profile_id.trim(), if apply_field_origin { "field" } else { "raw" })
}

fn localization_solve_signature(request: &LocalizationSolveRequest) -> Result<Vec<u8>, String> {
    serde_json::to_vec(request).map_err(|err| format!("failed to encode localization solve signature: {err}"))
}

pub(crate) async fn solve_via_engine(
    state: &AppState,
    profile: &helios_engine::localization::config::LocalizationProfile,
    sources: Vec<LocalizationSourceConfig>,
    rig_poses: &HashMap<String, PoseTransform>,
    field_map: Option<&FieldMapDocument>,
    stream_summaries: &[StreamSummary],
    fetcher: &ApiLocalizationSourceFetcher,
    apply_field_origin: bool,
) -> Result<LocalizationSolveResponse, String> {
    let request_started = Instant::now();
    let (request, source_fetch_ms) = build_localization_solve_request(state, profile, sources, rig_poses, field_map, stream_summaries, fetcher, apply_field_origin).await?;
    let cache_key = localization_solve_cache_key(&request.profile.id, request.apply_field_origin);
    let signature = localization_solve_signature(&request)?;
    if let Some(mut response) = state.services.runtime.localization_solve_cache().get_matching(&cache_key, &signature).await {
        response.timings.cache_hit = true;
        response.timings.source_fetch_ms = source_fetch_ms;
        response.timings.total_ms = request_started.elapsed().as_secs_f64() * 1000.0;
        return Ok(response);
    }
    match state.engine.solve_localization_event(request).await {
        Ok(EngineEvent::LocalizationSolved { response, .. }) => {
            let mut response: LocalizationSolveResponse = serde_json::from_value(response.into()).map_err(|err| format!("invalid localization solve response: {err}"))?;
            response.timings.cache_hit = false;
            response.timings.source_fetch_ms = source_fetch_ms;
            response.timings.total_ms = request_started.elapsed().as_secs_f64() * 1000.0;
            state.services.runtime.localization_solve_cache().insert(cache_key, signature, response.clone()).await;
            Ok(response)
        }
        Ok(EngineEvent::Nack { reason, .. }) => Err(reason),
        Ok(other) => Err(format!("unexpected engine response: {other:?}")),
        Err(err) => Err(err.to_string()),
    }
}

async fn build_localization_solve_request(
    state: &AppState,
    profile: &helios_engine::localization::config::LocalizationProfile,
    sources: Vec<LocalizationSourceConfig>,
    rig_poses: &HashMap<String, PoseTransform>,
    field_map: Option<&FieldMapDocument>,
    stream_summaries: &[StreamSummary],
    fetcher: &ApiLocalizationSourceFetcher,
    apply_field_origin: bool,
) -> Result<(LocalizationSolveRequest, f64), String> {
    let source_fetch_started = Instant::now();
    let source_values = fetch_localization_source_values(fetcher, &sources).await;
    let source_fetch_ms = source_fetch_started.elapsed().as_secs_f64() * 1000.0;
    let calibrations = load_stream_calibrations_from_streams(stream_summaries).into_iter().collect::<BTreeMap<_, _>>();
    let rig_poses = rig_poses.iter().map(|(camera_uid, pose)| (camera_uid.clone(), transform_to_pose(pose))).collect::<BTreeMap<_, _>>();
    let field_map = strip_overlay_from_field_map(field_map);

    let request = LocalizationSolveRequest { profile: profile.clone(), sources, rig_poses, field_map, calibrations, source_values, apply_field_origin };
    let _ = state;
    Ok((request, source_fetch_ms))
}

pub(crate) async fn fetch_localization_source_values(fetcher: &ApiLocalizationSourceFetcher, sources: &[LocalizationSourceConfig]) -> Vec<LocalizationSolveSourceValue> {
    join_all(sources.iter().map(|source| async move {
        match LocalizationSourceFetcher::fetch_source_value(fetcher, source).await {
            Ok(value) => LocalizationSolveSourceValue { source_id: source.id.clone(), value: Some(value.into()), error: None },
            Err(error) => LocalizationSolveSourceValue { source_id: source.id.clone(), value: None, error: Some(error) },
        }
    }))
    .await
}

pub(crate) fn load_stream_calibrations_from_streams(streams: &[StreamSummary]) -> HashMap<String, StreamCalibration> {
    let mut out = HashMap::new();
    for stream in streams {
        let Some(calib) = stream.manifest.calibration.as_ref() else {
            continue;
        };
        out.insert(stream.stream_id.to_string(), calib.clone());
    }
    out
}

pub(super) fn strip_overlay_from_field_map(field_map: Option<&FieldMapDocument>) -> Option<FieldMapDocument> {
    let mut field_map = field_map.cloned()?;
    field_map.overlay = None;
    Some(field_map)
}

pub(super) fn trim_localization_solve_response_to_field_poses(response: &mut LocalizationSolveResponse) {
    for solver in &mut response.solvers {
        solver.outputs.tag_in_camera = None;
        solver.outputs.camera_in_tag = None;
        solver.outputs.tag_in_robot = None;
        solver.outputs.robot_in_tag = None;
    }
}
