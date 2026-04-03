use crate::http::AppState;
use crate::http::streams::util;
use helios_engine::ipc::{EngineErrorCode, EngineEvent, JsonWire, StreamPipelineBinding};
use std::collections::BTreeMap;
use uuid::Uuid;

pub(crate) async fn apply_stream_graph_update(state: &AppState, stream_id: Uuid, graph: serde_json::Value, pipeline_id: Option<Uuid>, output: Option<String>) -> Result<(), String> {
    let pipeline_id = match pipeline_id {
        Some(id) => id,
        None => resolve_stream_pipeline_id(state, stream_id).await?,
    };

    // Stream websocket graph updates are live-stream mutations. They must not rewrite the
    // persisted pipeline document under `/pipelines`, or opening the UI can permanently alter
    // the canonical graph for every stream.
    crate::pipeline_command_service::load_pipeline_doc(pipeline_id).await?;

    match state.engine.set_graph(stream_id, graph, Some(pipeline_id), output.clone()).await {
        Ok(EngineEvent::Ack { .. }) => {
            util::persist_live_stream_manifest_update(state, stream_id, |manifest| {
                update_graph_binding(manifest, pipeline_id, output.clone());
            })
            .await?;
            Ok(())
        }
        Ok(EngineEvent::Nack { code: EngineErrorCode::NotFound, .. }) => {
            let updated = util::update_persisted_manifest_by_stream_id_checked(stream_id, |manifest| {
                update_graph_binding(manifest, pipeline_id, output.clone());
            })
            .await
            .map_err(|err| format!("failed to persist stream manifest: {err}"))?;
            if updated.is_some() { Ok(()) } else { Err("stream not found".to_string()) }
        }
        Ok(EngineEvent::Nack { code, reason, .. }) => Err(format!("engine rejected graph: {code:?}: {reason}")),
        Ok(_) => Err("unexpected engine response".to_string()),
        Err(err) => Err(format!("engine error: {err}")),
    }
}

pub(crate) async fn apply_stream_graph_patch(state: &AppState, stream_id: Uuid, patch: serde_json::Value, pipeline_id: Option<Uuid>) -> Result<(), String> {
    let pipeline_id = match pipeline_id {
        Some(id) => id,
        None => resolve_stream_pipeline_id(state, stream_id).await?,
    };

    crate::pipeline_command_service::load_pipeline_doc(pipeline_id).await?;

    match state.engine.set_graph_patch(stream_id, patch.clone(), Some(pipeline_id)).await {
        Ok(EngineEvent::Ack { .. }) => {
            util::persist_live_stream_manifest_update(state, stream_id, |manifest| {
                update_graph_patch_binding(manifest, pipeline_id, patch.clone());
            })
            .await?;
            Ok(())
        }
        Ok(EngineEvent::Nack { code: EngineErrorCode::NotFound, .. }) => {
            let updated = util::update_persisted_manifest_by_stream_id_checked(stream_id, |manifest| {
                update_graph_patch_binding(manifest, pipeline_id, patch.clone());
            })
            .await
            .map_err(|err| format!("failed to persist stream manifest: {err}"))?;
            if updated.is_some() { Ok(()) } else { Err("stream not found".to_string()) }
        }
        Ok(EngineEvent::Nack { code, reason, .. }) => Err(format!("engine rejected graph: {code:?}: {reason}")),
        Ok(_) => Err("unexpected engine response".to_string()),
        Err(err) => Err(format!("engine error: {err}")),
    }
}

pub(crate) async fn apply_stream_inputs(state: &AppState, stream_id: Uuid, pipeline_id: Option<Uuid>, inputs: BTreeMap<String, Option<serde_json::Value>>) -> Result<(), String> {
    let pipeline_id = match pipeline_id {
        Some(id) => Some(id),
        None => resolve_stream_pipeline_id(state, stream_id).await.ok(),
    };
    match state.engine.set_pipeline_inputs(stream_id, pipeline_id, inputs.clone()).await {
        Ok(EngineEvent::Ack { .. }) => {
            util::persist_live_stream_manifest_update(state, stream_id, |manifest| {
                util::apply_pipeline_host_inputs_update(manifest, &inputs);
            })
            .await?;
            Ok(())
        }
        Ok(EngineEvent::Nack { code, reason, .. }) => Err(format!("engine rejected inputs: {code:?}: {reason}")),
        Ok(other) => Err(format!("unexpected engine response: {other:?}")),
        Err(err) => Err(format!("engine error: {err}")),
    }
}

fn update_graph_binding(manifest: &mut helios_engine::ipc::StreamManifest, pipeline_id: Uuid, output: Option<String>) {
    util::normalize_pipeline_manifest(manifest);

    let mut updated = false;
    for binding in &mut manifest.pipelines {
        if binding.pipeline_id == pipeline_id {
            if output.is_some() {
                binding.pipeline_output = output.clone();
            }
            binding.pipeline_patch = None;
            updated = true;
            break;
        }
    }
    if !updated {
        manifest.pipelines.push(StreamPipelineBinding { pipeline_id, pipeline_graph: None, pipeline_output: output.clone(), pipeline_patch: None });
    }
    manifest.pipeline_enabled = true;
    if manifest.active_pipeline_id.is_none() {
        manifest.active_pipeline_id = Some(pipeline_id);
    }
    if output.is_some() && manifest.active_pipeline_id == Some(pipeline_id) {
        manifest.active_pipeline_output = output.clone();
    }
    util::promote_single_view_pipeline_selection(manifest, pipeline_id, output);
}

fn update_graph_patch_binding(manifest: &mut helios_engine::ipc::StreamManifest, pipeline_id: Uuid, patch: serde_json::Value) {
    util::normalize_pipeline_manifest(manifest);

    let patch = JsonWire::from(patch);
    let mut updated = false;
    for binding in &mut manifest.pipelines {
        if binding.pipeline_id == pipeline_id {
            binding.pipeline_patch = Some(patch.clone());
            updated = true;
            break;
        }
    }
    if !updated {
        manifest.pipelines.push(StreamPipelineBinding { pipeline_id, pipeline_graph: None, pipeline_output: None, pipeline_patch: Some(patch) });
    }

    manifest.pipeline_enabled = true;
    if manifest.active_pipeline_id.is_none() {
        manifest.active_pipeline_id = Some(pipeline_id);
    }
    util::promote_single_view_pipeline_selection(manifest, pipeline_id, None);
}

async fn resolve_stream_pipeline_id(state: &AppState, stream_id: Uuid) -> Result<Uuid, String> {
    let streams = state.engine.list_streams().await.map_err(|err| err.to_string())?;
    let stream = streams.iter().find(|s| s.stream_id == stream_id).ok_or_else(|| "stream not found".to_string())?;
    let manifest = &stream.manifest;
    if let Some(active) = manifest.active_pipeline_id {
        return Ok(active);
    }
    if let Some(binding) = manifest.pipelines.first() {
        return Ok(binding.pipeline_id);
    }
    Err("pipeline_id is required".to_string())
}
