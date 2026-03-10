use crate::http::AppState;
use crate::http::streams::util;
use crate::pipeline_command_service;
use axum::body::to_bytes;
use helios_engine::capture::CaptureControlValue;
use helios_engine::ipc::{EngineErrorCode, EngineEvent, StreamPipelineBinding};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc, watch};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct PendingControlUpdate {
    request_id: Option<String>,
    value: CaptureControlValue,
}

#[derive(Debug, Clone)]
pub(crate) struct StreamControlOutcome {
    pub request_id: Option<String>,
    pub result: Result<(), String>,
}

pub(crate) struct StreamControlsWorker {
    pending_controls: Arc<Mutex<BTreeMap<u32, Vec<PendingControlUpdate>>>>,
    responses_rx: mpsc::UnboundedReceiver<StreamControlOutcome>,
    shutdown_tx: watch::Sender<bool>,
}

pub(crate) async fn apply_stream_graph_update(state: &AppState, stream_id: Uuid, graph: serde_json::Value, pipeline_id: Option<Uuid>, output: Option<String>) -> Result<(), String> {
    let pipeline_id = match pipeline_id {
        Some(id) => id,
        None => resolve_stream_pipeline_id(state, stream_id).await?,
    };

    pipeline_command_service::update_pipeline_graph(state, pipeline_id, graph, None).await?;
    let doc = pipeline_command_service::load_pipeline_doc(pipeline_id).await?;

    match state.engine.set_graph(stream_id, doc.graph.clone(), Some(pipeline_id), output.clone()).await {
        Ok(EngineEvent::Ack { .. }) => {
            util::persist_live_stream_manifest_update(state, stream_id, |manifest| {
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
                manifest.pipeline_enabled = Some(true);
                if manifest.active_pipeline_id.is_none() {
                    manifest.active_pipeline_id = Some(pipeline_id);
                }
                if output.is_some() && manifest.active_pipeline_id == Some(pipeline_id) {
                    manifest.active_pipeline_output = output.clone();
                }
            })
            .await?;
            Ok(())
        }
        Ok(EngineEvent::Nack { code: EngineErrorCode::NotFound, .. }) => {
            let updated = util::update_persisted_manifest_by_stream_id_checked(stream_id, |manifest| {
                util::normalize_pipeline_manifest(manifest);

                let mut replaced = false;
                for binding in &mut manifest.pipelines {
                    if binding.pipeline_id == pipeline_id {
                        if output.is_some() {
                            binding.pipeline_output = output.clone();
                        }
                        binding.pipeline_patch = None;
                        replaced = true;
                        break;
                    }
                }
                if !replaced {
                    manifest.pipelines.push(StreamPipelineBinding { pipeline_id, pipeline_graph: None, pipeline_output: output.clone(), pipeline_patch: None });
                }

                manifest.pipeline_enabled = Some(true);
                if manifest.active_pipeline_id.is_none() {
                    manifest.active_pipeline_id = Some(pipeline_id);
                }
                if output.is_some() && manifest.active_pipeline_id == Some(pipeline_id) {
                    manifest.active_pipeline_output = output.clone();
                }
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

    let mut doc = pipeline_command_service::load_pipeline_doc(pipeline_id).await?;
    let mut daedalus_graph: daedalus::planner::Graph = serde_json::from_value(doc.graph.clone()).map_err(|err| format!("failed to decode pipeline graph: {err}"))?;
    let patch_model: daedalus::planner::GraphPatch = serde_json::from_value(patch).map_err(|err| format!("invalid graph patch: {err}"))?;
    let _report = patch_model.apply_to_graph(&mut daedalus_graph);
    doc.graph = serde_json::to_value(&daedalus_graph).map_err(|err| format!("failed to encode patched graph: {err}"))?;
    doc.updated_at_ms = chrono::Utc::now().timestamp_millis();
    pipeline_command_service::save_pipeline_doc(pipeline_id, &doc).await?;
    pipeline_command_service::update_pipeline_graph(state, pipeline_id, doc.graph.clone(), doc.name.clone()).await?;
    let doc = pipeline_command_service::load_pipeline_doc(pipeline_id).await?;

    match state.engine.set_graph(stream_id, doc.graph.clone(), Some(pipeline_id), None).await {
        Ok(EngineEvent::Ack { .. }) => {
            util::persist_live_stream_manifest_update(state, stream_id, |manifest| {
                util::normalize_pipeline_manifest(manifest);

                let mut updated = false;
                for binding in &mut manifest.pipelines {
                    if binding.pipeline_id == pipeline_id {
                        binding.pipeline_patch = None;
                        updated = true;
                        break;
                    }
                }
                if !updated {
                    manifest.pipelines.push(StreamPipelineBinding { pipeline_id, pipeline_graph: None, pipeline_output: None, pipeline_patch: None });
                }
                manifest.pipeline_enabled = Some(true);
                if manifest.active_pipeline_id.is_none() {
                    manifest.active_pipeline_id = Some(pipeline_id);
                }
            })
            .await?;
            Ok(())
        }
        Ok(EngineEvent::Nack { code: EngineErrorCode::NotFound, .. }) => {
            let updated = util::update_persisted_manifest_by_stream_id_checked(stream_id, |manifest| {
                util::normalize_pipeline_manifest(manifest);

                let mut replaced = false;
                for binding in &mut manifest.pipelines {
                    if binding.pipeline_id == pipeline_id {
                        binding.pipeline_patch = None;
                        replaced = true;
                        break;
                    }
                }
                if !replaced {
                    manifest.pipelines.push(StreamPipelineBinding { pipeline_id, pipeline_graph: None, pipeline_output: None, pipeline_patch: None });
                }

                manifest.pipeline_enabled = Some(true);
                if manifest.active_pipeline_id.is_none() {
                    manifest.active_pipeline_id = Some(pipeline_id);
                }
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

pub(crate) fn spawn_stream_controls_worker(state: AppState, stream_id: Uuid, apply_interval: Duration) -> StreamControlsWorker {
    let pending_controls = Arc::new(Mutex::new(BTreeMap::<u32, Vec<PendingControlUpdate>>::new()));
    let (responses_tx, responses_rx) = mpsc::unbounded_channel::<StreamControlOutcome>();
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    let worker_state = state.clone();
    let worker_pending_controls = Arc::clone(&pending_controls);
    tokio::spawn(async move {
        let mut apply_tick = tokio::time::interval(apply_interval);
        apply_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = apply_tick.tick() => {
                    let latest = {
                        let mut guard = worker_pending_controls.lock().await;
                        if guard.is_empty() {
                            continue;
                        }
                        std::mem::take(&mut *guard)
                    };

                    for (apply_id, apply_value) in latest {
                        let Some(latest_update) = apply_value.last().cloned() else {
                            continue;
                        };
                        let result = control_apply_result(
                            crate::http::streams::controls::set_control(worker_state.clone(), stream_id, apply_id, latest_update.value).await
                        )
                        .await;
                        if result.is_ok() {
                            worker_state.publish_realtime_update(
                                crate::ipc::RealtimeUpdateOrigin::Ws,
                                crate::ipc::RealtimeUpdateKind::StreamsControls,
                                format!("/v1/ws/streams/{stream_id}/controls"),
                                Some("set_control".to_string()),
                                latest_update.request_id.clone(),
                            );
                        }
                        for pending in apply_value {
                            if responses_tx
                                .send(StreamControlOutcome {
                                    request_id: pending.request_id.clone(),
                                    result: result.clone(),
                                })
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                }
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                }
            }
        }
    });

    StreamControlsWorker { pending_controls, responses_rx, shutdown_tx }
}

impl StreamControlsWorker {
    pub(crate) async fn enqueue(&self, control_id: u32, request_id: Option<String>, value: CaptureControlValue) {
        let mut guard = self.pending_controls.lock().await;
        guard.entry(control_id).or_default().push(PendingControlUpdate { request_id, value });
    }

    pub(crate) async fn next_result(&mut self) -> Option<StreamControlOutcome> {
        self.responses_rx.recv().await
    }

    pub(crate) fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
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

async fn control_apply_result(response: axum::response::Response) -> Result<(), String> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = to_bytes(response.into_body(), 64 * 1024).await.ok();
    let detail = body
        .as_deref()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(bytes).ok())
        .and_then(|json| json.get("error").and_then(|value| value.as_str()).map(str::to_string))
        .unwrap_or_else(|| format!("control update failed ({status})"));
    Err(detail)
}
