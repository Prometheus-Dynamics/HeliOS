//! "Run on this recording": a copy of a pipeline, with its camera bindings moved to a player. The
//! live pipeline is untouched; the copy is a managed pipeline of its own, removed when its player
//! closes.

use std::collections::BTreeSet;

use serde::Serialize;

use super::{context::RunContext, is_player, request::PlayerView};
use crate::{
    SharedState,
    error::{ApiError, ApiResult},
    routes::{
        cameras,
        pipelines::{self, Pipeline, PipelineSpec},
    },
};

/// The copy's pipeline id: the source pipeline's, with the player's token.
pub fn run_pipeline_id(source: &str, player: &PlayerView) -> String {
    let token = player.resource_id.rsplit('.').next().unwrap_or_default();
    format!("{source}-playback-{token}")
}

/// A run: the copy as a pipeline, and what its calibration and mount came from.
#[derive(Debug, Serialize)]
pub struct RunReport {
    pub pipeline: Pipeline,
    pub context: RunContext,
}

/// Runs pipeline `source` on `player` as a copy (replacing an earlier run of the same pair).
pub async fn run(state: &SharedState, player: &PlayerView, source: &str) -> ApiResult<RunReport> {
    let history = state.store.history(source).await?;
    let revision = history.latest().ok_or_else(|| ApiError::not_found(format!("no pipeline {source}")))?;
    let mut spec: PipelineSpec = serde_json::from_value(revision.spec.clone()).map_err(|error| ApiError::internal(format!("pipeline {source}: {error}")))?;

    let view = state.orion.view().await?;
    let cameras: BTreeSet<String> = cameras::camera_records(&view).map(|record| record.resource_id.to_string()).collect();
    let live = spec.bindings.values().map(|binding| binding.resource_id.clone()).find(|resource| cameras.contains(resource) && !is_player(resource)).unwrap_or_default();
    let mut rebound = 0;
    for binding in spec.bindings.values_mut().filter(|binding| cameras.contains(&binding.resource_id)) {
        binding.resource_id = player.resource_id.clone();
        binding.camera = None;
        rebound += 1;
    }
    if rebound == 0 {
        return Err(ApiError::unprocessable(format!("pipeline {source} has no camera binding to run on the recording")));
    }

    let facts = state.playback.facts(&player.id).await.ok_or_else(|| ApiError::not_found(format!("no playback {}", player.id)))?;
    let context = super::context::take(&state.store, &player.resource_id, &facts, &live).await?;
    spec.name = format!("{} on {}", spec.name, player.name.trim_start_matches("Playback: "));
    spec.enabled = true;
    let id = run_pipeline_id(source, player);
    let pipeline = pipelines::deploy(state, &id, spec).await?;
    Ok(RunReport { pipeline, context })
}

/// The pipelines whose bindings use `player`'s resource (its runs).
pub async fn runs_of(state: &SharedState, player: &PlayerView) -> ApiResult<Vec<String>> {
    let view = state.orion.view().await?;
    Ok(pipelines::pipelines_using(&view, &player.resource_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::request::{PlaybackSource, SourceKind};

    #[test]
    fn a_run_is_named_after_its_pipeline_and_player() {
        let player = PlayerView {
            id: "playback-1a2b3c4d".into(),
            resource_id: "camera.playback.1a2b3c4d".into(),
            name: "Playback: rec-1".into(),
            source: PlaybackSource::Recording { recording: "rec-1".into() },
            kind: SourceKind::Recording,
            raw: false,
            note: None,
            frames_endpoint: String::new(),
            speed: 1.0,
            looping: false,
            fps: None,
            created_at_ms: 1,
            status: None,
            status_error: None,
        };
        assert_eq!(run_pipeline_id("apriltag", &player), "apriltag-playback-1a2b3c4d");
    }
}
