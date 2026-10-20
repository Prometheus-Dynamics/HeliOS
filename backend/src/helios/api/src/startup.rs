//! The first-boot startup file (`/etc/helios/startup.toml`, staged by gaia). Its
//! `[default_pipeline]` is the pipeline helios-api creates on the first camera when the board has
//! no pipelines yet, with the embedded golden graph it names. It is created once: the decision is
//! kept in the API's state (`store.rs`), so a pipeline the user deletes stays deleted.

use std::{collections::BTreeMap, path::Path, time::Duration};

use anyhow::Context;
use serde::Deserialize;
use tracing::{info, warn};

use crate::{
    SharedState,
    error::{ApiError, ApiResult},
    host::now_ms,
    routes::{
        cameras,
        pipelines::{self, Binding, PipelineSpec},
    },
};

/// The id of the pipeline the default creates.
pub const DEFAULT_PIPELINE_ID: &str = "default";
/// The binding input the camera feeds: the golden graphs' host bridge `frame` port.
const FRAME_INPUT: &str = "frame";
/// How often the default pipeline is checked for (a camera can appear after the API starts).
const CHECK_INTERVAL: Duration = Duration::from_secs(5);

/// The golden graphs helios-engine ships (`backend/src/helios/engine/graphs`), by id: the
/// default pipeline's choices and the templates `GET /v1/pipelines/templates` offers.
/// The first template of a family is its default: the tracked detector as one node, which
/// starts on a sliced frame's first rows (the fastest path). The `-staged` ones are the same
/// detectors as node-groups, each stage a node (per-stage profiling).
pub const GRAPHS: &[(&str, &str)] = &[
    ("apriltag-36h11", include_str!("../../engine/graphs/apriltag-36h11.graph.json")),
    ("aruco-4x4_50", include_str!("../../engine/graphs/aruco-4x4_50.graph.json")),
    ("apriltag-36h11-staged", include_str!("../../engine/graphs/apriltag-36h11-staged.graph.json")),
    ("aruco-4x4_50-staged", include_str!("../../engine/graphs/aruco-4x4_50-staged.graph.json")),
    // Eidos's feature tracks (`eidos:features.track`): not a detector, so no dictionary.
    ("features-track", include_str!("../../engine/graphs/features-track.graph.json")),
    // Eidos's live mapper (`eidos:mapping.live_map`) behind the tracked AprilTag detector: the
    // camera pose, the map snapshot and the timing, for the UI's Map pane.
    ("live-map", include_str!("../../engine/graphs/live-map.graph.json")),
];

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StartupFile {
    pub default_pipeline: Option<DefaultPipeline>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DefaultPipeline {
    /// Create the pipeline (default true).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Its name in the API and the UI (default "AprilTag").
    #[serde(default = "default_name")]
    pub name: String,
    /// An embedded golden graph id, such as `apriltag-36h11`.
    pub graph: String,
    /// The Eidos dictionary the graph's detector decodes (see `PipelineSpec::dictionary`).
    #[serde(default)]
    pub dictionary: Option<String>,
    /// Half-size pyramid levels attached to the camera's frames (see `Binding::pyramid`).
    #[serde(default)]
    pub pyramid: Option<u8>,
}

fn default_true() -> bool {
    true
}

fn default_name() -> String {
    "AprilTag".into()
}

impl DefaultPipeline {
    /// The embedded golden graph this default names.
    fn graph_json(&self) -> anyhow::Result<&'static str> {
        GRAPHS.iter().find(|(id, _)| *id == self.graph).map(|(_, json)| *json).with_context(|| {
            let known: Vec<&str> = GRAPHS.iter().map(|(id, _)| *id).collect();
            format!("unknown graph {:?} in [default_pipeline] (known graphs: {})", self.graph, known.join(", "))
        })
    }

    /// The pipeline spec that creates the default, with its camera frames bound to `camera`.
    pub fn spec(&self, camera: &str) -> anyhow::Result<PipelineSpec> {
        let graph = serde_json::from_str(self.graph_json()?).context("embedded graph is not JSON")?;
        let binding = Binding { resource_id: camera.to_string(), pyramid: self.pyramid, ..Binding::default() };
        Ok(PipelineSpec {
            name: self.name.clone(),
            graph,
            bindings: BTreeMap::from([(FRAME_INPUT.to_string(), binding)]),
            enabled: true,
            search_mode: None,
            full_search_every: None,
            loss_full_search_after: None,
            margin_growth_misses: None,
            field_layout: None,
            dictionary: self.dictionary.clone(),
            cpu: None,
        })
    }
}

/// The default pipeline a startup file asks for: `None` when it has no `[default_pipeline]` or
/// it is disabled. An invalid file, or a default naming an unknown graph, is an error.
pub fn parse(text: &str) -> anyhow::Result<Option<DefaultPipeline>> {
    let file: StartupFile = toml::from_str(text).context("startup file is not valid startup TOML")?;
    let Some(default) = file.default_pipeline.filter(|default| default.enabled) else {
        return Ok(None);
    };
    default.graph_json()?;
    Ok(Some(default))
}

/// Whether the default pipeline is created now: only once, and only while Orion has no managed
/// pipeline and a camera is there to bind.
pub fn should_create_default_pipeline(has_managed_pipelines: bool, has_camera: bool, created_before: bool) -> bool {
    !created_before && !has_managed_pipelines && has_camera
}

/// The default pipeline of the startup file at `path`. A missing, unreadable or invalid file gives
/// none (logged): the API starts without one.
pub fn load_default_pipeline(path: &Path) -> Option<DefaultPipeline> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            info!(path = %path.display(), "no startup file; no default pipeline");
            return None;
        }
        Err(error) => {
            warn!(path = %path.display(), %error, "startup file is unreadable; no default pipeline");
            return None;
        }
    };
    match parse(&text) {
        Ok(Some(default)) => Some(default),
        Ok(None) => {
            info!(path = %path.display(), "the startup file asks for no default pipeline");
            None
        }
        Err(error) => {
            warn!(path = %path.display(), error = %format!("{error:#}"), "startup file is invalid; no default pipeline");
            None
        }
    }
}

/// Creates the default pipeline once its conditions hold, then stops checking. Errors (Orion not
/// answering yet, the engine not having loaded the graph's plugins) are retried, logged once each.
pub fn spawn_default_pipeline(state: SharedState, default: DefaultPipeline) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(CHECK_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last_error = None;
        loop {
            interval.tick().await;
            match settle(&state, &default).await {
                Ok(true) => return,
                Ok(false) => last_error = None,
                Err(error) => {
                    if last_error.as_deref() != Some(error.message.as_str()) {
                        warn!(error = %error.message, "default pipeline not created yet");
                        last_error = Some(error.message);
                    }
                }
            }
        }
    })
}

/// One check: `Ok(true)` once decided (created, or settled because the board has pipelines of its
/// own), `Ok(false)` while waiting for a camera.
async fn settle(state: &SharedState, default: &DefaultPipeline) -> ApiResult<bool> {
    let record = state.store.default_pipeline_record().await?;
    let view = state.orion.view().await?;
    let managed = pipelines::managed_pipeline_count(&view) > 0;
    // A playback player is not a camera to start a default on.
    let camera = cameras::camera_records(&view).filter(|record| !crate::playback::is_player(record.resource_id.as_str())).map(|record| record.resource_id.to_string()).min();
    match (should_create_default_pipeline(managed, camera.is_some(), record.is_some()), camera) {
        (true, Some(camera)) => {
            let spec = default.spec(&camera).map_err(|error| ApiError::internal(format!("{error:#}")))?;
            pipelines::deploy(state, DEFAULT_PIPELINE_ID, spec).await?;
            state.store.record_default_pipeline(true, now_ms()).await?;
            info!(camera = %camera, "created the default pipeline");
            Ok(true)
        }
        // A board that already has pipelines never gets the default, not even after the user has
        // deleted them all and rebooted.
        _ if record.is_none() && managed => {
            state.store.record_default_pipeline(false, now_ms()).await?;
            info!("the board already has pipelines; no default pipeline");
            Ok(true)
        }
        _ => Ok(record.is_some()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const APRILTAG: &str = r#"
        [default_pipeline]
        enabled = true
        name = "AprilTag"
        graph = "apriltag-36h11"
        dictionary = "apriltag_36h11"
        pyramid = 1
    "#;

    #[test]
    fn valid_startup_file_gives_the_default_pipeline() {
        let default = parse(APRILTAG).expect("parses").expect("enabled");
        assert_eq!(default.name, "AprilTag");
        assert_eq!(default.dictionary.as_deref(), Some("apriltag_36h11"));
        assert_eq!(default.pyramid, Some(1));
        let spec = default.spec("capture_device_node-local_camera-2c206880").expect("spec");
        assert_eq!(spec.name, "AprilTag");
        assert_eq!(spec.bindings.len(), 1);
        let frame = &spec.bindings["frame"];
        assert_eq!((frame.resource_id.as_str(), frame.pyramid), ("capture_device_node-local_camera-2c206880", Some(1)));
        assert_eq!(spec.dictionary.as_deref(), Some("apriltag_36h11"));
        assert_eq!(spec.graph["format"], "daedalus.graph");
    }

    #[test]
    fn disabled_or_absent_default_gives_none() {
        assert_eq!(parse(&APRILTAG.replace("enabled = true", "enabled = false")).expect("parses"), None);
        assert_eq!(parse("# nothing here\n").expect("parses"), None);
        // `enabled` defaults to true once the section is there.
        let default = parse("[default_pipeline]\ngraph = \"apriltag-36h11\"\n").expect("parses").expect("enabled");
        assert_eq!((default.name.as_str(), default.pyramid), ("AprilTag", None));
    }

    #[test]
    fn unknown_graph_and_invalid_files_are_errors() {
        let error = parse(&APRILTAG.replace("apriltag-36h11", "aruco-9")).expect_err("unknown graph");
        assert!(format!("{error:#}").contains("apriltag-36h11"), "{error:#}");
        // The stale libcamera preset of the old schema is not a startup file any more.
        assert!(parse("[[pipelines]]\nid = \"x\"\n[[streams]]\ncameraId = \"y\"\n").is_err());
        assert!(parse("[default_pipeline]\nenabled = true\n").is_err(), "graph is required");
        assert!(parse("[default_pipeline\n").is_err(), "not TOML");
    }

    #[test]
    fn default_pipeline_is_created_once_on_the_first_camera() {
        // No managed pipeline, a camera present, not created before: create.
        assert!(should_create_default_pipeline(false, true, false));
        // Nothing to bind yet.
        assert!(!should_create_default_pipeline(false, false, false));
        // The board already has a pipeline (its own or the default).
        assert!(!should_create_default_pipeline(true, true, false));
        // Created before: a deleted default stays deleted.
        assert!(!should_create_default_pipeline(false, true, true));
    }
}
