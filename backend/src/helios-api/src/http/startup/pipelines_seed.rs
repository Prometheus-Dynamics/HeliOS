use chrono::Utc;
use helios_engine::ipc::NodeRegistrySnapshot;
use serde_json::Value as JsonValue;
use std::collections::HashSet;
use tokio::fs;
use tracing::warn;
use uuid::Uuid;

use super::StartupPipelinePreset;
use crate::http::{AppState, pipelines, storage};

pub(super) async fn seed_pipelines(state: &AppState, presets: &[StartupPipelinePreset], registry: Option<&NodeRegistrySnapshot>) -> HashSet<Uuid> {
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

        let doc = pipelines::PipelineDocument::new(preset.id, name, graph_json, Utc::now().timestamp_millis());
        let path = dir.join(format!("{}.json", preset.id));
        let bytes = match doc.encode_pretty() {
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
            warn!(pipeline_id = %preset.id, "startup pipeline preset has both graph and template_id; using inline graph");
        }
        return Ok(graph.clone());
    }

    let template_id = preset.template_id.as_deref().map(str::trim).filter(|value| !value.is_empty()).ok_or_else(|| "pipeline preset requires either graph or templateId".to_string())?;

    pipelines::load_template_graph(template_id).await.map_err(|err| format!("failed to load template {template_id}: {err}"))
}
