mod marker_map;
mod postprocess;
mod smoothing;
mod solver_scope;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use lib_cv::modules::aruco::pose::TagPoseCalibration;
use lib_cv::modules::localization::MarkerMap;

use super::config::{LocalizationProfile, LocalizationSolverConfig, LocalizationSourceConfig};
use super::fetch::LocalizationSourceFetcher;
use super::maps::FieldMapDocument;
use super::math::PoseTransform;
use super::solvers::{SolverContext, SolverRegistry};
use super::sources::{fetch_source_samples_with_registry, SourceParserRegistry, SourceSample};
use super::types::{LocalizationSolveResponse, LocalizationSolveTimings, LocalizationSolverOutputs, LocalizationSolverResult, LocalizationSourceSampleStatus};
use marker_map::marker_map_from_field_map;
use postprocess::apply_profile_postprocessing;
#[cfg(test)]
use smoothing::{smooth_localization_pose, LocalizationPoseSmoothingInput};
use solver_scope::filter_solver_output_spaces_for_scope;

#[derive(Debug, Clone)]
pub(super) struct TemporalPoseState {
    translation: nalgebra::Vector3<f64>,
    rotation: nalgebra::UnitQuaternion<f64>,
    updated_at: Instant,
    last_multi_tag_at: Option<Instant>,
    last_tag_ids: Vec<u32>,
    reject_streak: u32,
    first_reject_at: Option<Instant>,
}

pub(super) static SOLVER_TEMPORAL_STATE: OnceLock<Mutex<HashMap<String, TemporalPoseState>>> = OnceLock::new();

pub async fn solve_localization<F: LocalizationSourceFetcher>(
    profile: &LocalizationProfile,
    sources: &[LocalizationSourceConfig],
    rig_poses: &HashMap<String, PoseTransform>,
    field_map: Option<&FieldMapDocument>,
    calibrations: &HashMap<String, TagPoseCalibration>,
    fetcher: &F,
    apply_field_origin: bool,
) -> LocalizationSolveResponse {
    let solver_registry = SolverRegistry::with_defaults();
    let parser_registry = SourceParserRegistry::with_defaults();
    let ctx = LocalizationSolveContext { profile, sources, rig_poses, field_map, calibrations, fetcher, solver_registry: &solver_registry, parser_registry: &parser_registry, apply_field_origin };
    solve_localization_with_registry(ctx).await
}

pub struct LocalizationSolveContext<'a, F> {
    pub profile: &'a LocalizationProfile,
    pub sources: &'a [LocalizationSourceConfig],
    pub rig_poses: &'a HashMap<String, PoseTransform>,
    pub field_map: Option<&'a FieldMapDocument>,
    pub calibrations: &'a HashMap<String, TagPoseCalibration>,
    pub fetcher: &'a F,
    pub solver_registry: &'a SolverRegistry,
    pub parser_registry: &'a SourceParserRegistry,
    pub apply_field_origin: bool,
}

pub async fn solve_localization_with_registry<F: LocalizationSourceFetcher>(ctx: LocalizationSolveContext<'_, F>) -> LocalizationSolveResponse {
    let total_started = Instant::now();
    let marker_map = ctx.field_map.map(marker_map_from_field_map);
    let default_tag_size_m = ctx.profile.tag_size_m.or_else(|| infer_tag_size_from_field_map(ctx.field_map));

    let source_parse_started = Instant::now();
    let mut source_samples = fetch_source_samples_with_registry(ctx.fetcher, ctx.sources, default_tag_size_m, ctx.calibrations, ctx.parser_registry).await;
    let source_parse_ms = source_parse_started.elapsed().as_secs_f64() * 1000.0;
    apply_profile_tag_filter(ctx.profile, &mut source_samples);
    let source_statuses = source_samples
        .iter()
        .map(|sample| LocalizationSourceSampleStatus {
            source_id: sample.source.id.clone(),
            stream_id: sample.source.stream_id.clone(),
            output_key: sample.source.output_key.clone(),
            camera_uid: sample.source.camera_uid.clone(),
            detections: sample.detections.len(),
            poll_ms: sample.poll_ms,
            tag_size: sample.tag_size,
            error: sample.error.clone(),
        })
        .collect();

    let solver_started = Instant::now();
    let mut solver_results = ctx.profile.solvers.iter().map(|solver| solve_for_solver(ctx.solver_registry, solver, &source_samples, ctx.rig_poses, marker_map.as_ref())).collect::<Vec<_>>();
    apply_profile_postprocessing(ctx.profile, &mut solver_results, ctx.rig_poses, ctx.field_map, ctx.apply_field_origin);
    let solver_ms = solver_started.elapsed().as_secs_f64() * 1000.0;

    LocalizationSolveResponse {
        profile_id: ctx.profile.id.clone(),
        solvers: solver_results,
        sources: source_statuses,
        timings: LocalizationSolveTimings { source_fetch_ms: 0.0, source_parse_ms, solver_ms, engine_ms: total_started.elapsed().as_secs_f64() * 1000.0, total_ms: 0.0, cache_hit: false },
    }
}

fn infer_tag_size_from_field_map(field_map: Option<&FieldMapDocument>) -> Option<f64> {
    let map = field_map?;
    let mut buckets: HashMap<i64, (usize, f64)> = HashMap::new();

    for marker in &map.markers {
        let size = marker.size_m;
        if !size.is_finite() || size <= 0.0 {
            continue;
        }
        // Bucket by micrometers so equivalent sizes with minor float noise collapse.
        let key = (size * 1_000_000.0).round() as i64;
        let entry = buckets.entry(key).or_insert((0, size));
        entry.0 += 1;
    }

    buckets.into_iter().max_by_key(|(_key, (count, _size))| *count).map(|(_key, (_count, size))| size)
}

fn apply_profile_tag_filter(profile: &LocalizationProfile, samples: &mut [SourceSample]) {
    if profile.allowed_tag_ids.is_empty() && profile.excluded_tag_ids.is_empty() {
        return;
    }
    let allowed = profile.allowed_tag_ids.iter().copied().collect::<HashSet<_>>();
    let excluded = profile.excluded_tag_ids.iter().copied().collect::<HashSet<_>>();
    for sample in samples {
        sample.detections.retain(|detection| {
            let tag_id = detection.tag_id;
            let allowed_match = allowed.is_empty() || allowed.contains(&tag_id);
            allowed_match && !excluded.contains(&tag_id)
        });
    }
}

fn solve_for_solver(
    registry: &SolverRegistry,
    solver: &LocalizationSolverConfig,
    samples: &[SourceSample],
    rig_poses: &HashMap<String, PoseTransform>,
    marker_map: Option<&MarkerMap>,
) -> LocalizationSolverResult {
    let requested_output_spaces = solver.output_spaces.clone();
    let requested_sources = solver.source_ids.iter().filter(|id| !id.trim().is_empty()).collect::<Vec<_>>();

    let mut scoped_samples = Vec::new();
    for sample in samples {
        if !requested_sources.is_empty() && !requested_sources.contains(&&sample.source.id) {
            continue;
        }
        scoped_samples.push(sample);
    }
    let output_spaces = filter_solver_output_spaces_for_scope(&requested_output_spaces, &scoped_samples);

    let Some(active_solver) = registry.solver_for(solver) else {
        return LocalizationSolverResult {
            id: solver.id.clone(),
            name: solver.name.clone(),
            mode: solver.mode,
            output_spaces: output_spaces.clone(),
            outputs: LocalizationSolverOutputs::default(),
            errors: vec![format!("unsupported solver mode: {:?}", solver.mode)],
        };
    };

    let outcome = active_solver.solve(SolverContext { solver_id: &solver.id, solver_config: solver, output_spaces: &output_spaces, samples: &scoped_samples, rig_poses, marker_map });

    LocalizationSolverResult { id: solver.id.clone(), name: solver.name.clone(), mode: solver.mode, output_spaces, outputs: outcome.outputs, errors: outcome.errors }
}
