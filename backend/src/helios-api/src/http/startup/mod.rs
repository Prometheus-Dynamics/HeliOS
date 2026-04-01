mod files;
mod pipelines_seed;
mod reconcile;
mod streams_seed;

use super::AppState;
use crate::features;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::io;
use tokio::fs;
use tracing::{info, warn};
use uuid::Uuid;

use self::{
    files::{decode_startup_preset, pipeline_document_count, startup_marker_path, startup_preset_path, write_marker},
    pipelines_seed::seed_pipelines,
    reconcile::reconcile_persisted_startup_state,
    streams_seed::seed_streams,
};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct StartupPresetDocument {
    pub(super) pipelines: Vec<StartupPipelinePreset>,
    pub(super) streams: Vec<StartupStreamPreset>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StartupPipelinePreset {
    pub(super) id: Uuid,
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) graph: Option<JsonValue>,
    #[serde(default)]
    pub(super) template_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StartupStreamPreset {
    pub(super) camera_id: String,
    #[serde(default)]
    pub(super) stream_id: Option<Uuid>,
    pub(super) manifest: helios_engine::ipc::StreamManifest,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StartupPresetMarker {
    pub(super) status: String,
    pub(super) config_path: String,
    pub(super) applied_at: String,
    pub(super) pipelines_seeded: usize,
    pub(super) streams_seeded: usize,
}

pub(crate) async fn apply_startup_preset(state: AppState) {
    reconcile_persisted_startup_state().await;
    super::localization::maps::seed_bundled_field_maps().await;

    let preset_path = startup_preset_path();
    let marker_path = startup_marker_path();

    if fs::metadata(&marker_path).await.is_ok() {
        return;
    }

    let bytes = match fs::read(&preset_path).await {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return,
        Err(err) => {
            warn!(path = %preset_path.display(), error = %err, "failed to read startup preset");
            return;
        }
    };

    let preset = match decode_startup_preset(&preset_path, &bytes) {
        Ok(preset) => preset,
        Err(err) => {
            warn!(path = %preset_path.display(), error = %err, "failed to parse startup preset");
            return;
        }
    };

    if preset.pipelines.is_empty() && preset.streams.is_empty() {
        info!(path = %preset_path.display(), "startup preset is empty; marking as consumed");
        let _ = write_marker(
            &marker_path,
            StartupPresetMarker { status: "skipped-empty".to_string(), config_path: preset_path.display().to_string(), applied_at: Utc::now().to_rfc3339(), pipelines_seeded: 0, streams_seeded: 0 },
        )
        .await;
        return;
    }

    let existing_streams = super::streams_persist::list_persisted_records().await;
    let existing_pipeline_count = pipeline_document_count().await.unwrap_or(0);
    if !existing_streams.is_empty() || existing_pipeline_count > 0 {
        info!(
            path = %preset_path.display(),
            existing_streams = existing_streams.len(),
            existing_pipelines = existing_pipeline_count,
            "startup preset skipped because persisted streams/pipelines already exist"
        );
        let _ = write_marker(
            &marker_path,
            StartupPresetMarker {
                status: "skipped-existing-state".to_string(),
                config_path: preset_path.display().to_string(),
                applied_at: Utc::now().to_rfc3339(),
                pipelines_seeded: 0,
                streams_seeded: 0,
            },
        )
        .await;
        return;
    }

    let registry = if features::startup_pipeline_metadata_injection_enabled() {
        match state.engine.get_node_registry().await {
            Ok(snapshot) => Some(snapshot),
            Err(err) => {
                warn!(error = %err, "failed to fetch node registry during startup preset apply; continuing without injected port metadata");
                None
            }
        }
    } else {
        None
    };

    let pipeline_ids = seed_pipelines(&state, &preset.pipelines, registry.as_ref()).await;
    let seeded_streams = seed_streams(&preset.streams).await;

    info!(
        path = %preset_path.display(),
        pipelines_seeded = pipeline_ids.len(),
        streams_seeded = seeded_streams,
        "startup preset apply completed"
    );

    if let Err(err) = write_marker(
        &marker_path,
        StartupPresetMarker {
            status: "applied".to_string(),
            config_path: preset_path.display().to_string(),
            applied_at: Utc::now().to_rfc3339(),
            pipelines_seeded: pipeline_ids.len(),
            streams_seeded: seeded_streams,
        },
    )
    .await
    {
        warn!(path = %marker_path.display(), error = %err, "failed to write startup preset marker");
    }
}
