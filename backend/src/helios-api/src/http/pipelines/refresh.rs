use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashSet};
use tracing::warn;
use uuid::Uuid;

use crate::http::{
    AppState, streams,
    streams::util::{normalize_pipeline_manifest, preferred_pipeline_output},
    streams_persist,
};
use helios_engine::ipc::{EngineEvent, StreamManifest};

use super::types::PipelineRefreshFailure;

fn manifest_references_pipeline(manifest: &StreamManifest, pipeline_id: Uuid) -> bool {
    if manifest.active_pipeline_id == Some(pipeline_id) {
        return true;
    }
    if manifest.pipelines.iter().any(|binding| binding.pipeline_id == pipeline_id) {
        return true;
    }
    if let Some(layout) = manifest.pipeline_layout.as_ref() {
        return layout.slots.iter().any(|slot| slot.pipeline_id == Some(pipeline_id));
    }
    false
}

fn detach_pipeline_from_manifest(manifest: &mut StreamManifest, pipeline_id: Uuid) -> bool {
    if !manifest_references_pipeline(manifest, pipeline_id) {
        return false;
    }

    let mut changed = false;

    let before_pipelines = manifest.pipelines.len();
    manifest.pipelines.retain(|binding| binding.pipeline_id != pipeline_id);
    if manifest.pipelines.len() != before_pipelines {
        changed = true;
    }

    if let Some(layout) = manifest.pipeline_layout.as_mut() {
        let before_slots = layout.slots.len();
        layout.slots.retain(|slot| slot.pipeline_id != Some(pipeline_id));
        if layout.slots.len() != before_slots {
            changed = true;
        }
    }

    if manifest.active_pipeline_id == Some(pipeline_id) {
        manifest.active_pipeline_id = None;
        manifest.active_pipeline_output = None;
        changed = true;
    }

    if let Some(active_id) = manifest.active_pipeline_id
        && !manifest.pipelines.iter().any(|binding| binding.pipeline_id == active_id)
    {
        manifest.active_pipeline_id = None;
        manifest.active_pipeline_output = None;
        changed = true;
    }

    if manifest.active_pipeline_id.is_none() && !manifest.pipelines.is_empty() {
        let next_active = manifest.pipelines[0].pipeline_id;
        manifest.active_pipeline_id = Some(next_active);
        let output = manifest.pipelines.iter().find(|binding| binding.pipeline_id == next_active).and_then(|binding| binding.pipeline_output.clone());
        manifest.active_pipeline_output = output;
        changed = true;
    }

    if manifest.pipelines.is_empty() {
        manifest.pipeline_enabled = Some(false);
        manifest.pipeline_layout = None;
        manifest.active_pipeline_id = None;
        manifest.active_pipeline_output = None;
        changed = true;
    } else if manifest.pipeline_enabled == Some(false) {
        manifest.pipeline_enabled = Some(true);
        changed = true;
    }

    normalize_pipeline_manifest(manifest);
    changed
}

pub(super) async fn detach_pipeline_from_streams(state: &AppState, pipeline_id: Uuid) -> Result<(), String> {
    let mut updated_streams: HashSet<Uuid> = HashSet::new();
    let running = match state.engine.list_streams().await {
        Ok(list) => list,
        Err(err) => {
            warn!(pipeline_id = %pipeline_id, error = %err, "failed to list running streams while detaching pipeline");
            Vec::new()
        }
    };

    for stream in running {
        if stream.manifest.internal {
            continue;
        }
        let mut manifest = stream.manifest.clone();
        if !detach_pipeline_from_manifest(&mut manifest, pipeline_id) {
            continue;
        }
        manifest.identity.id = Some(stream.stream_id);
        if let Err(status) = streams::restart_stream_with_manifest(state.clone(), manifest).await {
            warn!(pipeline_id = %pipeline_id, stream_id = %stream.stream_id, status = %status, "failed to restart stream after detaching pipeline");
        }
        updated_streams.insert(stream.stream_id);
    }

    let records = streams_persist::list_persisted_records().await;
    for record in records {
        let Some(mut manifest) = record.manifest else {
            continue;
        };
        if manifest.internal {
            continue;
        }
        let stream_id = manifest.identity.id.or(record.last_stream_id).unwrap_or_else(|| streams_persist::derived_stream_id(&record.camera_id));
        if updated_streams.contains(&stream_id) {
            continue;
        }
        if !detach_pipeline_from_manifest(&mut manifest, pipeline_id) {
            continue;
        }
        manifest.identity.id = Some(stream_id);
        streams_persist::persist_manifest_checked(&record.camera_id, Some(stream_id), manifest)
            .await
            .map_err(|err| format!("deleted graph but failed to persist detached stream manifest for {}: {err}", record.camera_id))?;
    }

    Ok(())
}

pub(crate) async fn refresh_pipeline_consumers(state: &AppState, pipeline_id: Uuid, graph: &JsonValue) -> Vec<PipelineRefreshFailure> {
    let mut failures = Vec::new();
    let streams = match state.engine.list_streams().await {
        Ok(streams) => streams,
        Err(err) => {
            warn!(pipeline_id = %pipeline_id, error = %err, "failed to list streams for pipeline refresh");
            return vec![PipelineRefreshFailure { stream_id: Uuid::nil(), error: err.to_string() }];
        }
    };

    for stream in streams {
        let manifest = &stream.manifest;
        let uses_pipeline = manifest.pipelines.iter().any(|binding| binding.pipeline_id == pipeline_id);
        if !uses_pipeline {
            continue;
        }
        let output = preferred_pipeline_output(manifest, pipeline_id);
        if let Err(err) = state.engine.set_graph(stream.stream_id, graph.clone(), Some(pipeline_id), output).await {
            warn!(stream_id = %stream.stream_id, pipeline_id = %pipeline_id, error = %err, "failed to refresh pipeline graph on stream");
            failures.push(PipelineRefreshFailure { stream_id: stream.stream_id, error: err.to_string() });
        }
    }

    failures
}

pub(crate) async fn refresh_pipeline_input_consumers(state: &AppState, pipeline_id: Uuid, inputs: BTreeMap<String, Option<JsonValue>>) -> Vec<PipelineRefreshFailure> {
    let mut failures = Vec::new();
    let streams = match state.engine.list_streams().await {
        Ok(streams) => streams,
        Err(err) => {
            warn!(pipeline_id = %pipeline_id, error = %err, "failed to list streams for pipeline input refresh");
            return vec![PipelineRefreshFailure { stream_id: Uuid::nil(), error: err.to_string() }];
        }
    };

    for stream in streams {
        let manifest = &stream.manifest;
        let uses_pipeline = manifest.pipelines.iter().any(|binding| binding.pipeline_id == pipeline_id);
        if !uses_pipeline {
            continue;
        }
        match state.engine.set_pipeline_inputs(stream.stream_id, Some(pipeline_id), inputs.clone()).await {
            Ok(EngineEvent::Ack { .. }) => {}
            Ok(EngineEvent::Nack { code, reason, .. }) => {
                let error = format!("engine rejected inputs: {code:?}: {reason}");
                warn!(stream_id = %stream.stream_id, pipeline_id = %pipeline_id, %error, "failed to refresh pipeline inputs on stream");
                failures.push(PipelineRefreshFailure { stream_id: stream.stream_id, error });
            }
            Ok(other) => {
                let error = format!("unexpected engine response: {other:?}");
                warn!(stream_id = %stream.stream_id, pipeline_id = %pipeline_id, %error, "failed to refresh pipeline inputs on stream");
                failures.push(PipelineRefreshFailure { stream_id: stream.stream_id, error });
            }
            Err(err) => {
                warn!(stream_id = %stream.stream_id, pipeline_id = %pipeline_id, error = %err, "failed to refresh pipeline inputs on stream");
                failures.push(PipelineRefreshFailure { stream_id: stream.stream_id, error: err.to_string() });
            }
        }
    }

    failures
}
