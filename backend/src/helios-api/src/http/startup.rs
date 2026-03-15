use super::AppState;
use crate::features;
use crate::http::{pipelines, storage, streams, streams_persist};
use chrono::Utc;
use helios_engine::ipc::{NodeRegistrySnapshot, StreamManifest};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{info, warn};
use uuid::Uuid;

const DEFAULT_STARTUP_PRESET_TOML_PATH: &str = "/etc/helios/startup.toml";
const LEGACY_STARTUP_PRESET_JSON_PATH: &str = "/etc/helios/startup.json";
const STARTUP_PRESET_FILE_ENV: &str = "HELIOS_STARTUP_PRESET_FILE";
const STARTUP_PRESET_MARKER_ENV: &str = "HELIOS_STARTUP_PRESET_MARKER";
const STARTUP_PRESET_MARKER_NAME: &str = ".startup-preset-applied-v1.json";

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct StartupPresetDocument {
    pipelines: Vec<StartupPipelinePreset>,
    streams: Vec<StartupStreamPreset>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartupPipelinePreset {
    id: Uuid,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    graph: Option<JsonValue>,
    #[serde(default)]
    template_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartupStreamPreset {
    camera_id: String,
    #[serde(default)]
    stream_id: Option<Uuid>,
    manifest: StreamManifest,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartupPresetMarker {
    status: String,
    config_path: String,
    applied_at: String,
    pipelines_seeded: usize,
    streams_seeded: usize,
}

pub(crate) async fn apply_startup_preset(state: AppState) {
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

    let existing_streams = streams_persist::list_persisted_records().await;
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

fn startup_preset_path() -> PathBuf {
    if let Some(path) = std::env::var_os(STARTUP_PRESET_FILE_ENV).map(PathBuf::from) {
        return path;
    }
    let toml_default = PathBuf::from(DEFAULT_STARTUP_PRESET_TOML_PATH);
    if toml_default.exists() { toml_default } else { PathBuf::from(LEGACY_STARTUP_PRESET_JSON_PATH) }
}

fn startup_marker_path() -> PathBuf {
    std::env::var_os(STARTUP_PRESET_MARKER_ENV).map(PathBuf::from).unwrap_or_else(|| storage::data_root_path().join(STARTUP_PRESET_MARKER_NAME))
}

async fn write_marker(path: &Path, marker: StartupPresetMarker) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let bytes = serde_json::to_vec_pretty(&marker).map_err(io::Error::other)?;
    fs::write(path, bytes).await
}

fn decode_startup_preset(path: &Path, bytes: &[u8]) -> Result<StartupPresetDocument, String> {
    let ext = path.extension().and_then(OsStr::to_str).map(|value| value.to_ascii_lowercase());

    match ext.as_deref() {
        Some("toml") => toml::from_str::<StartupPresetDocument>(&String::from_utf8_lossy(bytes))
            .map_err(|err| err.to_string())
            .or_else(|toml_err| serde_json::from_slice::<StartupPresetDocument>(bytes).map_err(|json_err| format!("toml parse error: {toml_err}; json parse error: {json_err}"))),
        Some("json") => serde_json::from_slice::<StartupPresetDocument>(bytes)
            .map_err(|err| err.to_string())
            .or_else(|json_err| toml::from_str::<StartupPresetDocument>(&String::from_utf8_lossy(bytes)).map_err(|toml_err| format!("json parse error: {json_err}; toml parse error: {toml_err}"))),
        _ => serde_json::from_slice::<StartupPresetDocument>(bytes).or_else(|_| toml::from_str::<StartupPresetDocument>(&String::from_utf8_lossy(bytes))).map_err(|err| err.to_string()),
    }
}

async fn pipeline_document_count() -> io::Result<usize> {
    let dir = storage::ensure_subdir_async("pipelines").await?;
    let mut entries = fs::read_dir(dir).await?;
    let mut count = 0usize;
    while let Some(entry) = entries.next_entry().await? {
        if !entry.file_type().await.map(|ty| ty.is_file()).unwrap_or(false) {
            continue;
        }
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        count += 1;
    }
    Ok(count)
}

async fn seed_pipelines(state: &AppState, presets: &[StartupPipelinePreset], registry: Option<&NodeRegistrySnapshot>) -> HashSet<Uuid> {
    let mut seeded = HashSet::new();
    let mut seen = HashSet::new();
    let dir = match storage::ensure_subdir_async("pipelines").await {
        Ok(dir) => dir,
        Err(err) => {
            warn!(error = %err, "failed to prepare pipeline storage for startup preset");
            return seeded;
        }
    };

    for preset in presets {
        if !seen.insert(preset.id) {
            warn!(pipeline_id = %preset.id, "startup preset has duplicate pipeline id; keeping the first entry");
            continue;
        }

        let graph = match resolve_preset_graph(preset).await {
            Ok(graph) => graph,
            Err(err) => {
                warn!(pipeline_id = %preset.id, error = %err, "failed to resolve startup pipeline graph");
                continue;
            }
        };

        let mut raw_graph = graph;
        pipelines::normalize_graph_metadata(&mut raw_graph);
        let mut parsed_graph: daedalus::planner::Graph = match serde_json::from_value(raw_graph.clone()) {
            Ok(graph) => graph,
            Err(err) => {
                warn!(pipeline_id = %preset.id, error = %err, "startup pipeline graph is not a valid daedalus graph");
                continue;
            }
        };
        pipelines::normalize_graph_node_ids(&mut parsed_graph);
        let mut graph_json = match serde_json::to_value(&parsed_graph) {
            Ok(value) => value,
            Err(err) => {
                warn!(pipeline_id = %preset.id, error = %err, "failed to encode startup pipeline graph");
                continue;
            }
        };

        if let Some(snapshot) = registry {
            pipelines::inject_port_metadata(&mut graph_json, snapshot);
        }
        pipelines::merge_edge_metadata(&raw_graph, &mut graph_json);

        let name = preset
            .name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string())
            .or_else(|| preset.template_id.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(|value| value.to_string()));

        pipelines::inject_pipeline_alias_metadata(&mut graph_json, name.as_deref());

        let doc = pipelines::PipelineDocument { id: preset.id, name, graph: graph_json, updated_at_ms: Utc::now().timestamp_millis() };
        let path = dir.join(format!("{}.json", preset.id));
        let bytes = match serde_json::to_vec_pretty(&doc) {
            Ok(bytes) => bytes,
            Err(err) => {
                warn!(pipeline_id = %preset.id, error = %err, "failed to encode startup pipeline document");
                continue;
            }
        };

        match fs::write(&path, bytes).await {
            Ok(()) => {
                pipelines::refresh_graph_validation(state, preset.id, &doc.graph).await;
                seeded.insert(preset.id);
            }
            Err(err) => {
                warn!(pipeline_id = %preset.id, path = %path.display(), error = %err, "failed to write startup pipeline document");
            }
        }
    }

    seeded
}

async fn resolve_preset_graph(preset: &StartupPipelinePreset) -> Result<JsonValue, String> {
    if let Some(graph) = &preset.graph {
        if preset.template_id.is_some() {
            warn!(
                pipeline_id = %preset.id,
                "startup pipeline preset has both graph and template_id; using inline graph"
            );
        }
        return Ok(graph.clone());
    }

    let template_id = preset.template_id.as_deref().map(str::trim).filter(|value| !value.is_empty()).ok_or_else(|| "pipeline preset requires either graph or templateId".to_string())?;

    pipelines::load_template_graph(template_id).await.map_err(|err| format!("failed to load template {template_id}: {err}"))
}

async fn seed_streams(presets: &[StartupStreamPreset]) -> usize {
    let mut seeded = 0usize;
    let mut seen_camera_ids: HashSet<String> = HashSet::new();

    for preset in presets {
        let camera_id = preset.camera_id.trim();
        if camera_id.is_empty() {
            warn!("startup stream preset has empty camera_id; skipping");
            continue;
        }

        if !seen_camera_ids.insert(camera_id.to_string()) {
            warn!(camera_id, "startup stream preset has duplicate camera_id; later entry will overwrite earlier stream record");
        }

        let mut manifest = preset.manifest.clone();
        manifest.internal = false;
        if let Some(stream_id) = preset.stream_id {
            manifest.identity.id = Some(stream_id);
        }

        let validation = streams::validation::validate_stream_manifest(manifest).await;
        let manifest = match validation {
            Ok(validated) => {
                if !validated.warnings.is_empty() {
                    warn!(
                        camera_id,
                        warning_count = validated.warnings.len(),
                        warnings = ?validated.warnings,
                        "startup stream preset required sanitization"
                    );
                }
                validated.manifest
            }
            Err(err) => {
                warn!(
                    camera_id,
                    issue_count = err.issues.len(),
                    warning_count = err.warnings.len(),
                    issues = ?err.issues,
                    warnings = ?err.warnings,
                    "startup stream preset failed semantic validation; skipping"
                );
                continue;
            }
        };

        let stream_id = manifest.identity.id;
        streams_persist::persist_manifest(camera_id, stream_id, manifest).await;
        seeded += 1;
    }

    seeded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_toml_decodes() {
        let raw = r#"
            [[pipelines]]
            id = "4fca14eb-e218-48e4-adf9-b957d645c604"
            name = "Default ArUco"
            templateId = "daedalus_aruco"

            [[streams]]
            cameraId = "ov9782"
            streamId = "43e2cb68-95b0-4269-97da-7e16c730c36a"

            [streams.manifest.identity]
            id = "43e2cb68-95b0-4269-97da-7e16c730c36a"
            alias = "ov9782-csi"
            hardware_id = "ov9782"

            [streams.manifest.capture]
            backend = "Libcamera"
            target_fps = 120
            enable_tdn_output = true
            device_keys = ["ov9782"]

            [streams.manifest.capture.handle]
            type = "libcamera"
            id = "ov9782"

            [streams.manifest.capture.mode]
            interval = { numerator = 1, denominator = 120 }

            [streams.manifest.capture.mode.format]
            code = "NV12"
            color = "Unknown"
            resolution = { width = 1280, height = 720 }

            [[streams.manifest.capture.controls]]
            id = 10002
            value = { kind = "int", value = 3 }

            [[streams.manifest.pipelines]]
            pipeline_id = "4fca14eb-e218-48e4-adf9-b957d645c604"
            pipeline_output = "overlay"

            [streams.manifest.pipeline_layout]
            rows = 1
            columns = 1

            [[streams.manifest.pipeline_layout.slots]]
            row = 0
            column = 0
            pipeline_id = "4fca14eb-e218-48e4-adf9-b957d645c604"
            output_key = "overlay"

            [streams.manifest]
            pipeline_enabled = true
            active_pipeline_id = "4fca14eb-e218-48e4-adf9-b957d645c604"
            active_pipeline_output = "overlay"
            encoder_enabled = true
            start_on_boot = true

            [streams.manifest.encoder_settings]
            framerate = { numerator = 20, denominator = 1 }
            output_resolution = { width = 854, height = 480 }
        "#;

        let doc = decode_startup_preset(Path::new("/etc/helios/startup.toml"), raw.as_bytes()).expect("decode startup preset");
        assert_eq!(doc.pipelines.len(), 1);
        assert_eq!(doc.streams.len(), 1);
        assert_eq!(doc.streams[0].camera_id, "ov9782");
        assert_eq!(doc.streams[0].manifest.capture.target_fps, Some(120));
        assert_eq!(doc.streams[0].manifest.encoder_settings.as_ref().and_then(|enc| enc.output_resolution.as_ref()).map(|res| (res.width, res.height)), Some((854, 480)));
    }
}
