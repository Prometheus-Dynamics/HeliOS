use crate::http::AppState;
use crate::http::error_history::{ErrorHistoryEntry, record_error_entry};
use crate::http::streams::util;
use axum::{
    Router,
    body::to_bytes,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use futures::{SinkExt, StreamExt};
use helios_engine::capture::CaptureControlValue;
use helios_engine::ipc::{EngineErrorCode, EngineEvent, StreamPipelineBinding};
use helios_engine::stream::StreamMetrics;
use helios_engine::stream::{read_latest_frame_with_header, touch_stream_viewer};
use lib_asyncapi::registry::SchemaRegistry;
use lib_asyncapi::{SchemaProvider, Server, Tag, TypeSchema, WsDoc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, mpsc, watch};
use tracing::debug;
use uuid::Uuid;

use super::pipelines as pipelines_ws;

type WsSender = futures::stream::SplitSink<WebSocket, Message>;

const DEFAULT_INTERVAL_MS: u64 = 1_000;
const MIN_INTERVAL_MS: u64 = 250;
const MAX_INTERVAL_MS: u64 = 10_000;

const DEFAULT_FRAMES_POLL_MS: u64 = 10;
const MIN_FRAMES_POLL_MS: u64 = 5;
const MAX_FRAMES_POLL_MS: u64 = 100;

const DEFAULT_OUTPUT_SAMPLE_INTERVAL_MS: u64 = 250;
const MIN_OUTPUT_SAMPLE_INTERVAL_MS: u64 = 100;
const MAX_OUTPUT_SAMPLE_INTERVAL_MS: u64 = 5_000;

const DEFAULT_OUTPUT_PORTS_INTERVAL_MS: u64 = 2_000;
const MIN_OUTPUT_PORTS_INTERVAL_MS: u64 = 500;
const MAX_OUTPUT_PORTS_INTERVAL_MS: u64 = 30_000;
const CONTROL_APPLY_INTERVAL_MS: u64 = 75;

#[derive(Debug, Clone, Deserialize)]
pub struct MetricsWsParams {
    pub interval_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FramesWsParams {
    /// Host bridge output port name to forward to the encoder/preview (defaults to `frame`).
    pub output: Option<String>,
    /// Restore the previous pipeline/output selection when the socket disconnects.
    pub restore: Option<bool>,
    /// Shmem poll interval in ms.
    pub poll_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OutputsWsParams {
    pub interval_ms: Option<u64>,
    pub ports_interval_ms: Option<u64>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/:id/metrics", get(stream_metrics))
        .route("/:id/frames", get(stream_frames))
        .route("/:id/updates", get(stream_updates))
        .route("/:id/controls", get(stream_controls))
        .route("/:id/outputs", get(stream_outputs))
}

async fn stream_metrics(ws: WebSocketUpgrade, State(state): State<AppState>, Path(id): Path<Uuid>, Query(params): Query<MetricsWsParams>) -> impl IntoResponse {
    let interval_ms = params.interval_ms.unwrap_or(DEFAULT_INTERVAL_MS).clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS);
    ws.on_upgrade(move |socket| handle_stream_metrics(socket, state, id, Duration::from_millis(interval_ms)))
}

async fn stream_frames(ws: WebSocketUpgrade, State(state): State<AppState>, Path(id): Path<Uuid>, Query(params): Query<FramesWsParams>) -> impl IntoResponse {
    let poll_ms = params.poll_ms.unwrap_or(DEFAULT_FRAMES_POLL_MS).clamp(MIN_FRAMES_POLL_MS, MAX_FRAMES_POLL_MS);
    ws.on_upgrade(move |socket| handle_stream_frames(socket, state, id, params, Duration::from_millis(poll_ms)))
}

async fn stream_updates(ws: WebSocketUpgrade, State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_stream_updates(socket, state, id))
}

async fn stream_controls(ws: WebSocketUpgrade, State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_stream_controls(socket, state, id))
}

async fn stream_outputs(ws: WebSocketUpgrade, State(state): State<AppState>, Path(id): Path<Uuid>, Query(params): Query<OutputsWsParams>) -> impl IntoResponse {
    let interval_ms = params.interval_ms.unwrap_or(DEFAULT_OUTPUT_SAMPLE_INTERVAL_MS).clamp(MIN_OUTPUT_SAMPLE_INTERVAL_MS, MAX_OUTPUT_SAMPLE_INTERVAL_MS);
    let ports_interval_ms = params.ports_interval_ms.unwrap_or(DEFAULT_OUTPUT_PORTS_INTERVAL_MS).clamp(MIN_OUTPUT_PORTS_INTERVAL_MS, MAX_OUTPUT_PORTS_INTERVAL_MS);
    ws.on_upgrade(move |socket| handle_stream_outputs(socket, state, id, Duration::from_millis(interval_ms), Duration::from_millis(ports_interval_ms)))
}

async fn handle_stream_metrics(socket: WebSocket, state: AppState, stream_id: Uuid, interval: Duration) {
    let (mut sender, mut receiver) = socket.split();
    let mut events = state.engine.subscribe_events();

    // Seed with a snapshot so UI can render immediately.
    if let Err(err) = send_metrics_snapshot(&mut sender, &state, stream_id).await {
        let _ = send_metrics_error(&mut sender, Some(stream_id), "snapshot", &err).await;
        let _ = sender.close().await;
        return;
    }

    let mut min_gap = interval;
    let mut last_sent = Instant::now();

    loop {
        tokio::select! {
            event = events.recv() => {
                match event {
                    Ok(EngineEvent::Metrics { stream_id: sid, metrics, .. }) if sid == stream_id => {
                        if last_sent.elapsed() >= min_gap {
                            if let Err(err) = send_metrics_payload(&mut sender, stream_id, metrics).await {
                                let _ = send_metrics_error(&mut sender, Some(stream_id), "send_metrics", &err).await;
                                let _ = sender.close().await;
                                break;
                            }
                            last_sent = Instant::now();
                        }
                    }
                    Ok(EngineEvent::MetricsUpdate { stream_id: sid, metrics }) if sid == stream_id => {
                        if last_sent.elapsed() >= min_gap {
                            if let Err(err) = send_metrics_payload(&mut sender, stream_id, metrics).await {
                                let _ = send_metrics_error(&mut sender, Some(stream_id), "send_metrics", &err).await;
                                let _ = sender.close().await;
                                break;
                            }
                            last_sent = Instant::now();
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | Some(Err(_)) => break,
                    Some(Ok(Message::Ping(bytes))) => {
                        if sender.send(Message::Pong(bytes)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Text(text))) => {
                        if let Some(new_interval) = parse_interval_update(&text) {
                            let clamped = new_interval.clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS);
                            min_gap = Duration::from_millis(clamped);
                            debug!(stream_id = %stream_id, %clamped, "updated stream metrics interval from client request");
                        }
                    }
                    Some(Ok(Message::Binary(_))) | Some(Ok(Message::Pong(_))) => {}
                    None => break,
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamUpdateRequest {
    SetGraph {
        #[serde(default)]
        request_id: Option<String>,
        graph: serde_json::Value,
        #[serde(default)]
        pipeline_id: Option<Uuid>,
        #[serde(default)]
        output: Option<String>,
    },
    SetGraphPatch {
        #[serde(default)]
        request_id: Option<String>,
        patch: serde_json::Value,
        #[serde(default)]
        pipeline_id: Option<Uuid>,
    },
    SetInputs {
        #[serde(default)]
        request_id: Option<String>,
        #[serde(default)]
        pipeline_id: Option<Uuid>,
        inputs: BTreeMap<String, Option<serde_json::Value>>,
    },
    Ping {
        #[serde(default)]
        request_id: Option<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamUpdateResponse {
    Ack {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    Error {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        error: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamControlRequest {
    SetControl {
        #[serde(default)]
        request_id: Option<String>,
        control_id: u32,
        value: CaptureControlValue,
    },
    Ping {
        #[serde(default)]
        request_id: Option<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamControlResponse {
    Ack {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    Error {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        error: String,
    },
}

#[derive(Debug, Clone)]
struct PendingControlUpdate {
    request_id: Option<String>,
    value: CaptureControlValue,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamOutputsRequest {
    Ping {
        #[serde(default)]
        request_id: Option<String>,
    },
    Subscribe {
        ports: Vec<String>,
        #[serde(default)]
        interval_ms: Option<u64>,
        #[serde(default)]
        request_id: Option<String>,
    },
    List {
        #[serde(default)]
        request_id: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
struct StreamOutputsList {
    outputs: Vec<helios_engine::ipc::GraphOutputPortDescriptor>,
    timestamp_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct StreamOutputSampleEvent {
    port: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamOutputsResponse {
    Ack {
        #[serde(default)]
        request_id: Option<String>,
    },
    Error {
        #[serde(default)]
        request_id: Option<String>,
        error: String,
    },
}

#[derive(Debug, Clone)]
struct OutputsSubscription {
    ports: Vec<String>,
    interval: Duration,
}

async fn handle_stream_outputs(socket: WebSocket, state: AppState, stream_id: Uuid, default_interval: Duration, ports_interval: Duration) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();

    // Send initial port list so UI can populate immediately.
    match state.engine.list_graph_outputs_event(stream_id).await {
        Ok(EngineEvent::GraphOutputs { outputs, .. }) => {
            let payload = StreamOutputsList { outputs, timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64, request_id: None };
            if let Ok(text) = serde_json::to_string(&payload) {
                let _ = out_tx.send(Message::Text(text.into()));
            }
        }
        Ok(EngineEvent::Nack { code, reason, .. }) => {
            let payload = StreamOutputsResponse::Error { request_id: None, error: format!("engine rejected outputs list: {code:?}: {reason}") };
            if let Ok(text) = serde_json::to_string(&payload) {
                let _ = out_tx.send(Message::Text(text.into()));
            }
        }
        Ok(_) => {
            let payload = StreamOutputsResponse::Error { request_id: None, error: "unexpected engine response listing outputs".to_string() };
            if let Ok(text) = serde_json::to_string(&payload) {
                let _ = out_tx.send(Message::Text(text.into()));
            }
        }
        Err(err) => {
            let payload = StreamOutputsResponse::Error { request_id: None, error: format!("engine error listing outputs: {err}") };
            if let Ok(text) = serde_json::to_string(&payload) {
                let _ = out_tx.send(Message::Text(text.into()));
            }
        }
    }

    let (sub_tx, mut sub_rx) = watch::channel(OutputsSubscription { ports: Vec::new(), interval: default_interval });

    // Writer task.
    let writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if ws_sender.send(msg).await.is_err() {
                break;
            }
        }
        let _ = ws_sender.close().await;
    });

    // Sampler task.
    let sampler_state = state.clone();
    let sampler_tx = out_tx.clone();
    let sampler = tokio::spawn(async move {
        let mut subscription = sub_rx.borrow().clone();
        let mut sample_tick = tokio::time::interval(subscription.interval);
        sample_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let mut ports_tick = tokio::time::interval(ports_interval);
        ports_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let mut last_outputs_local: Vec<helios_engine::ipc::GraphOutputPortDescriptor> = Vec::new();
        let mut immediate = true;

        loop {
            tokio::select! {
                _ = sub_rx.changed() => {
                    subscription = sub_rx.borrow().clone();
                    sample_tick = tokio::time::interval(subscription.interval);
                    sample_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    immediate = true;
                }
                _ = ports_tick.tick() => {
                    if let Ok(EngineEvent::GraphOutputs { outputs, .. }) = sampler_state.engine.list_graph_outputs_event(stream_id).await
                        && outputs != last_outputs_local
                    {
                        last_outputs_local = outputs.clone();
                        let payload = StreamOutputsList { outputs, timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64, request_id: None };
                        if let Ok(text) = serde_json::to_string(&payload) {
                            let _ = sampler_tx.send(Message::Text(text.into()));
                        }
                    }
                }
                _ = sample_tick.tick() => {
                    if subscription.ports.is_empty() && !immediate {
                        continue;
                    }
                    immediate = false;
                    if subscription.ports.is_empty() {
                        continue;
                    }
                    for port in &subscription.ports {
                        let timestamp_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
                        match sampler_state.engine.get_graph_output_sample_event(stream_id, port.clone()).await {
                            Ok(EngineEvent::GraphOutputSample { value, .. }) => {
                                let payload = StreamOutputSampleEvent { port: port.clone(), value: Some(value.0), error: None, timestamp_ms };
                                if let Ok(text) = serde_json::to_string(&payload) {
                                    let _ = sampler_tx.send(Message::Text(text.into()));
                                }
                            }
                            Ok(EngineEvent::Nack { code, reason, .. }) => {
                                let payload = StreamOutputSampleEvent { port: port.clone(), value: None, error: Some(format!("{code:?}: {reason}")), timestamp_ms };
                                if let Ok(text) = serde_json::to_string(&payload) {
                                    let _ = sampler_tx.send(Message::Text(text.into()));
                                }
                            }
                            Ok(other) => {
                                let payload = StreamOutputSampleEvent { port: port.clone(), value: None, error: Some(format!("unexpected engine response: {other:?}")), timestamp_ms };
                                if let Ok(text) = serde_json::to_string(&payload) {
                                    let _ = sampler_tx.send(Message::Text(text.into()));
                                }
                            }
                            Err(err) => {
                                let payload = StreamOutputSampleEvent { port: port.clone(), value: None, error: Some(err.to_string()), timestamp_ms };
                                if let Ok(text) = serde_json::to_string(&payload) {
                                    let _ = sampler_tx.send(Message::Text(text.into()));
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    // Reader loop (in this task) updates the subscription.
    while let Some(msg) = ws_receiver.next().await {
        let payload = match msg {
            Ok(Message::Text(text)) => text,
            Ok(Message::Ping(bytes)) => {
                let _ = out_tx.send(Message::Pong(bytes));
                continue;
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => continue,
            Err(err) => {
                debug!(error = %err, "stream outputs websocket closed");
                break;
            }
        };

        let parsed = serde_json::from_str::<StreamOutputsRequest>(&payload);
        match parsed {
            Ok(StreamOutputsRequest::Ping { request_id }) => {
                let response = StreamOutputsResponse::Ack { request_id };
                if let Ok(text) = serde_json::to_string(&response) {
                    let _ = out_tx.send(Message::Text(text.into()));
                }
            }
            Ok(StreamOutputsRequest::List { request_id }) => match state.engine.list_graph_outputs_event(stream_id).await {
                Ok(EngineEvent::GraphOutputs { outputs, .. }) => {
                    let event = StreamOutputsList { outputs, timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64, request_id };
                    if let Ok(text) = serde_json::to_string(&event) {
                        let _ = out_tx.send(Message::Text(text.into()));
                    }
                }
                Ok(EngineEvent::Nack { code, reason, .. }) => {
                    let response = StreamOutputsResponse::Error { request_id, error: format!("engine rejected outputs list: {code:?}: {reason}") };
                    if let Ok(text) = serde_json::to_string(&response) {
                        let _ = out_tx.send(Message::Text(text.into()));
                    }
                }
                Ok(_) => {
                    let response = StreamOutputsResponse::Error { request_id, error: "unexpected engine response listing outputs".to_string() };
                    if let Ok(text) = serde_json::to_string(&response) {
                        let _ = out_tx.send(Message::Text(text.into()));
                    }
                }
                Err(err) => {
                    let response = StreamOutputsResponse::Error { request_id, error: format!("engine error listing outputs: {err}") };
                    if let Ok(text) = serde_json::to_string(&response) {
                        let _ = out_tx.send(Message::Text(text.into()));
                    }
                }
            },
            Ok(StreamOutputsRequest::Subscribe { ports, interval_ms, request_id }) => {
                let mut normalized: Vec<String> = ports.into_iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
                normalized.sort();
                normalized.dedup();
                let interval = Duration::from_millis(interval_ms.unwrap_or(DEFAULT_OUTPUT_SAMPLE_INTERVAL_MS).clamp(MIN_OUTPUT_SAMPLE_INTERVAL_MS, MAX_OUTPUT_SAMPLE_INTERVAL_MS));
                let _ = sub_tx.send(OutputsSubscription { ports: normalized, interval });
                let response = StreamOutputsResponse::Ack { request_id };
                if let Ok(text) = serde_json::to_string(&response) {
                    let _ = out_tx.send(Message::Text(text.into()));
                }
            }
            Err(err) => {
                let response = StreamOutputsResponse::Error { request_id: None, error: format!("invalid request: {err}") };
                if let Ok(text) = serde_json::to_string(&response) {
                    let _ = out_tx.send(Message::Text(text.into()));
                }
            }
        }
    }

    // Shutdown background tasks by dropping senders.
    drop(out_tx);
    sampler.abort();
    let _ = sampler.await;
    let _ = writer.await;
}

async fn handle_stream_updates(mut socket: WebSocket, state: AppState, stream_id: Uuid) {
    while let Some(msg) = socket.next().await {
        let payload = match msg {
            Ok(Message::Text(text)) => text,
            Ok(Message::Ping(bytes)) => {
                let _ = socket.send(Message::Pong(bytes)).await;
                continue;
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => continue,
            Err(err) => {
                debug!(error = %err, "stream updates websocket closed");
                break;
            }
        };

        let parsed = serde_json::from_str::<StreamUpdateRequest>(&payload);
        let response = match parsed {
            Ok(StreamUpdateRequest::Ping { request_id }) => StreamUpdateResponse::Ack { request_id },
            Ok(StreamUpdateRequest::SetGraph { request_id, graph, pipeline_id, output }) => match apply_stream_graph_update(&state, stream_id, graph, pipeline_id, output).await {
                Ok(()) => {
                    state.publish_realtime_update(crate::ipc::RealtimeUpdateOrigin::Ws, "streams", format!("/v1/ws/streams/{stream_id}/updates"), Some("set_graph".to_string()), request_id.clone());
                    StreamUpdateResponse::Ack { request_id }
                }
                Err(err) => StreamUpdateResponse::Error { request_id, error: err },
            },
            Ok(StreamUpdateRequest::SetGraphPatch { request_id, patch, pipeline_id }) => match apply_stream_graph_patch(&state, stream_id, patch, pipeline_id).await {
                Ok(()) => {
                    state.publish_realtime_update(
                        crate::ipc::RealtimeUpdateOrigin::Ws,
                        "streams",
                        format!("/v1/ws/streams/{stream_id}/updates"),
                        Some("set_graph_patch".to_string()),
                        request_id.clone(),
                    );
                    StreamUpdateResponse::Ack { request_id }
                }
                Err(err) => StreamUpdateResponse::Error { request_id, error: err },
            },
            Ok(StreamUpdateRequest::SetInputs { request_id, pipeline_id, inputs }) => {
                let pipeline_id = match pipeline_id {
                    Some(id) => Some(id),
                    None => resolve_stream_pipeline_id(&state, stream_id).await.ok(),
                };
                match state.engine.set_pipeline_inputs(stream_id, pipeline_id, inputs.clone()).await {
                    Ok(EngineEvent::Ack { .. }) => {
                        if let Err(err) = util::persist_live_stream_manifest_update(&state, stream_id, |manifest| {
                            util::apply_pipeline_host_inputs_update(manifest, &inputs);
                        })
                        .await
                        {
                            StreamUpdateResponse::Error { request_id, error: err }
                        } else {
                            state.publish_realtime_update(
                                crate::ipc::RealtimeUpdateOrigin::Ws,
                                "streams",
                                format!("/v1/ws/streams/{stream_id}/updates"),
                                Some("set_inputs".to_string()),
                                request_id.clone(),
                            );
                            StreamUpdateResponse::Ack { request_id }
                        }
                    }
                    Ok(EngineEvent::Nack { code, reason, .. }) => StreamUpdateResponse::Error { request_id, error: format!("engine rejected inputs: {code:?}: {reason}") },
                    Ok(other) => StreamUpdateResponse::Error { request_id, error: format!("unexpected engine response: {other:?}") },
                    Err(err) => StreamUpdateResponse::Error { request_id, error: format!("engine error: {err}") },
                }
            }
            Err(err) => StreamUpdateResponse::Error { request_id: None, error: format!("invalid request: {err}") },
        };

        if let Ok(text) = serde_json::to_string(&response)
            && socket.send(Message::Text(text.into())).await.is_err()
        {
            break;
        }
    }
}

async fn handle_stream_controls(mut socket: WebSocket, state: AppState, stream_id: Uuid) {
    let pending_controls = Arc::new(Mutex::new(BTreeMap::<u32, Vec<PendingControlUpdate>>::new()));
    let (responses_tx, mut responses_rx) = mpsc::unbounded_channel::<StreamControlResponse>();
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    let worker_state = state.clone();
    let worker_pending_controls = Arc::clone(&pending_controls);
    let worker_responses = responses_tx;
    tokio::spawn(async move {
        let mut apply_tick = tokio::time::interval(Duration::from_millis(CONTROL_APPLY_INTERVAL_MS));
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
                        let result =
                            control_apply_result(crate::http::streams::controls::set_control(worker_state.clone(), stream_id, apply_id, latest_update.value).await).await;
                        if result.is_ok() {
                            worker_state.publish_realtime_update(
                                crate::ipc::RealtimeUpdateOrigin::Ws,
                                "streams",
                                format!("/v1/ws/streams/{stream_id}/controls"),
                                Some("set_control".to_string()),
                                latest_update.request_id.clone(),
                            );
                        }
                        for pending in apply_value {
                            let response = match &result {
                                Ok(()) => StreamControlResponse::Ack { request_id: pending.request_id.clone() },
                                Err(err) => StreamControlResponse::Error { request_id: pending.request_id.clone(), error: err.clone() },
                            };
                            if worker_responses.send(response).is_err() {
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

    loop {
        tokio::select! {
            msg = socket.next() => {
                let payload = match msg {
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(Message::Ping(bytes))) => {
                        if socket.send(Message::Pong(bytes)).await.is_err() {
                            break;
                        }
                        continue;
                    }
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => continue,
                    Some(Err(err)) => {
                        debug!(error = %err, "stream controls websocket closed");
                        break;
                    }
                    None => break,
                };

                let parsed = serde_json::from_str::<StreamControlRequest>(&payload);
                let immediate = match parsed {
                    Ok(StreamControlRequest::Ping { request_id }) => Some(StreamControlResponse::Ack { request_id }),
                    Ok(StreamControlRequest::SetControl { request_id, control_id, value }) => {
                        let mut guard = pending_controls.lock().await;
                        guard.entry(control_id).or_default().push(PendingControlUpdate { request_id, value });
                        None
                    }
                    Err(err) => Some(StreamControlResponse::Error { request_id: None, error: format!("invalid request: {err}") }),
                };

                if let Some(response) = immediate
                    && let Ok(text) = serde_json::to_string(&response)
                    && socket.send(Message::Text(text.into())).await.is_err()
                {
                    break;
                }
            }
            response = responses_rx.recv() => {
                let Some(response) = response else {
                    break;
                };
                if let Ok(text) = serde_json::to_string(&response)
                    && socket.send(Message::Text(text.into())).await.is_err()
                {
                    break;
                }
            }
        }
    }

    let _ = shutdown_tx.send(true);
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

async fn apply_stream_graph_update(state: &AppState, stream_id: Uuid, graph: serde_json::Value, pipeline_id: Option<Uuid>, output: Option<String>) -> Result<(), String> {
    let pipeline_id = match pipeline_id {
        Some(id) => id,
        None => resolve_stream_pipeline_id(state, stream_id).await?,
    };

    // Persist graph updates into the pipeline store first. Streams must only reference pipeline IDs
    // (no floating inline graphs), otherwise we end up with untracked/undeployable graphs.
    pipelines_ws::update_pipeline_graph(state, pipeline_id, graph.clone(), None).await?;
    let doc = pipelines_ws::load_pipeline_doc(pipeline_id).await?;

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

async fn apply_stream_graph_patch(state: &AppState, stream_id: Uuid, patch: serde_json::Value, pipeline_id: Option<Uuid>) -> Result<(), String> {
    let pipeline_id = match pipeline_id {
        Some(id) => id,
        None => resolve_stream_pipeline_id(state, stream_id).await?,
    };

    // Apply patch to the pipeline graph (persist), then push the updated graph to the running stream.
    // This avoids per-stream floating patches that cannot be reasoned about or deployed.
    let mut doc = pipelines_ws::load_pipeline_doc(pipeline_id).await?;
    let mut daedalus_graph: daedalus::planner::Graph = serde_json::from_value(doc.graph.clone()).map_err(|err| format!("failed to decode pipeline graph: {err}"))?;
    let patch_model: daedalus::planner::GraphPatch = serde_json::from_value(patch.clone()).map_err(|err| format!("invalid graph patch: {err}"))?;
    let _report = patch_model.apply_to_graph(&mut daedalus_graph);
    doc.graph = serde_json::to_value(&daedalus_graph).map_err(|err| format!("failed to encode patched graph: {err}"))?;
    doc.updated_at_ms = chrono::Utc::now().timestamp_millis();
    pipelines_ws::save_pipeline_doc(pipeline_id, &doc).await?;
    pipelines_ws::update_pipeline_graph(state, pipeline_id, doc.graph.clone(), doc.name.clone()).await?;
    let doc = pipelines_ws::load_pipeline_doc(pipeline_id).await?;

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

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
enum FramesEvent {
    #[serde(rename = "format")]
    Format { fourcc: String, width: u32, height: u32 },
    #[serde(rename = "error")]
    Error {
        error: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        timestamp_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        operation: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        retryable: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        remediation: Option<String>,
    },
}

async fn handle_stream_frames(socket: WebSocket, state: AppState, stream_id: Uuid, params: FramesWsParams, poll: Duration) {
    let (mut sender, mut receiver) = socket.split();

    let prev_output = match snapshot_stream_output(&state, stream_id).await {
        Ok(snapshot) => snapshot,
        Err(err) => {
            let payload = make_frames_error("snapshot_stream_output", &err);
            record_frames_error(&payload);
            let _ = sender.send(Message::Text(serde_json::to_string(&payload).unwrap_or_else(|_| format!(r#"{{"type":"error","error":"{err}"}}"#)).into())).await;
            let _ = sender.close().await;
            return;
        }
    };

    let requested_output = params.output.clone().or(prev_output.clone()).unwrap_or_else(|| "frame".to_string());
    let restore = params.restore.unwrap_or(true);

    if let Err(err) = set_stream_output(&state, stream_id, Some(requested_output.clone())).await {
        let payload = make_frames_error("set_stream_output", &err);
        record_frames_error(&payload);
        let _ = sender.send(Message::Text(serde_json::to_string(&payload).unwrap_or_else(|_| format!(r#"{{"type":"error","error":"{err}"}}"#)).into())).await;
        let _ = sender.close().await;
        return;
    }

    // This socket is "latest frame wins": keep a tiny buffer and drop frames if the client can't keep up.
    let (frame_tx, mut frame_rx) = tokio::sync::mpsc::channel::<(helios_engine::stream::ShmemFrameHeader, Vec<u8>)>(1);
    tokio::task::spawn_blocking(move || {
        let mut last_seq = 0u64;
        let mut last_touch = Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(Instant::now);
        loop {
            if frame_tx.is_closed() {
                break;
            }
            if last_touch.elapsed() >= Duration::from_millis(500) {
                let _ = touch_stream_viewer(stream_id);
                last_touch = Instant::now();
            }
            let (hdr, payload) = match read_latest_frame_with_header(stream_id) {
                Ok((hdr, payload)) => (hdr, payload),
                Err(_) => {
                    // The shmem backing can be transiently unavailable during capture/pipeline restarts.
                    std::thread::sleep(poll);
                    continue;
                }
            };
            if hdr.len == 0 || hdr.fourcc.to_u32() == 0 || hdr.seq == last_seq {
                std::thread::sleep(poll);
                continue;
            }
            last_seq = hdr.seq;
            if frame_tx.try_send((hdr, payload)).is_err() {
                // Client is slow; drop this frame and keep polling for the newest one.
                std::thread::sleep(poll);
            }
        }
    });

    let mut last_format: Option<(u32, u32, u32)> = None;

    loop {
        tokio::select! {
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(Ok(Message::Ping(bytes))) => {
                        if sender.send(Message::Pong(bytes)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Text(_))) | Some(Ok(Message::Binary(_))) | Some(Ok(Message::Pong(_))) => {}
                }
            }
            maybe_frame = frame_rx.recv() => {
                let Some((hdr, payload)) = maybe_frame else { break; };
                let format = (hdr.fourcc.to_u32(), hdr.width, hdr.height);
                if last_format != Some(format) {
                    last_format = Some(format);
                    let event = FramesEvent::Format { fourcc: hdr.fourcc.to_string(), width: hdr.width, height: hdr.height };
                    if sender.send(Message::Text(serde_json::to_string(&event).unwrap_or_else(|_| r#"{"type":"format"}"#.to_string()).into())).await.is_err() {
                        break;
                    }
                }
                if sender.send(Message::Binary(payload.into())).await.is_err() {
                    break;
                }
            }
        }
    }

    drop(frame_rx);

    if restore {
        let _ = set_stream_output(&state, stream_id, prev_output).await;
    }
}

async fn snapshot_stream_output(state: &AppState, stream_id: Uuid) -> Result<Option<String>, String> {
    let streams = state.engine.list_streams().await.map_err(|err| err.to_string())?;
    let summary = streams.into_iter().find(|s| s.stream_id == stream_id).ok_or_else(|| "stream not found".to_string())?;
    let manifest = summary.manifest;

    if manifest.pipeline_enabled == Some(false) {
        return Ok(None);
    }

    // Legacy `pipeline_output` is no longer the source of truth; the active binding carries the
    // selected output. Use the most specific value available so websocket preview does not
    // override the user's chosen output.
    let output = manifest
        .active_pipeline_output
        .clone()
        .or_else(|| manifest.active_pipeline_id.and_then(|active_id| manifest.pipelines.iter().find(|p| p.pipeline_id == active_id)).and_then(|binding| binding.pipeline_output.clone()))
        .or_else(|| manifest.pipelines.first().and_then(|binding| binding.pipeline_output.clone()));

    Ok(output)
}

async fn set_stream_output(state: &AppState, stream_id: Uuid, output: Option<String>) -> Result<(), String> {
    match state.engine.set_graph_output(stream_id, output).await {
        Ok(EngineEvent::Ack { .. }) => Ok(()),
        Ok(EngineEvent::Nack { reason, .. }) => Err(reason),
        Ok(_) => Err("unexpected engine response".into()),
        Err(err) => Err(err.to_string()),
    }
}

fn parse_interval_update(raw: &str) -> Option<u64> {
    let parsed: serde_json::Value = serde_json::from_str(raw).ok()?;
    parsed.get("interval_ms").and_then(|v| v.as_u64())
}

async fn send_metrics_snapshot(sender: &mut WsSender, state: &AppState, stream_id: Uuid) -> Result<(), String> {
    match state.engine.get_metrics(stream_id).await {
        Ok(EngineEvent::Metrics { metrics, .. }) => send_metrics_payload(sender, stream_id, metrics).await,
        Ok(EngineEvent::Nack { reason, .. }) => Err(reason),
        Ok(_) => Err("unexpected engine response".into()),
        Err(err) => Err(err.to_string()),
    }
}

async fn send_metrics_payload(sender: &mut WsSender, stream_id: Uuid, metrics: StreamMetrics) -> Result<(), String> {
    let timestamp_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let payload = StreamMetricsEvent { stream_id, metrics, timestamp_ms };
    let text = serde_json::to_string(&payload).map_err(|err| err.to_string())?;
    sender.send(Message::Text(text.into())).await.map_err(|err| err.to_string())
}

#[derive(Debug, Clone, Serialize)]
struct StreamMetricsEvent {
    stream_id: Uuid,
    metrics: StreamMetrics,
    timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
struct StreamMetricsError {
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_id: Option<Uuid>,
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    timestamp_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remediation: Option<String>,
}

fn classify_metrics_error(reason: &str) -> (&'static str, &'static str) {
    let normalized = reason.to_lowercase();
    if normalized.contains("stream") && normalized.contains("not found") {
        return ("stream_not_found", "Stream not found");
    }
    if normalized.contains("handshake") || normalized.contains("connection refused") || normalized.contains("engine") || normalized.contains("ipc") {
        return ("engine_unavailable", "Engine unavailable");
    }
    if normalized.contains("timed out") || normalized.contains("timeout") {
        return ("metrics_timeout", "Metrics request timed out");
    }
    if normalized.contains("unexpected engine response") {
        return ("engine_unexpected", "Unexpected engine response");
    }
    if normalized.contains("broken pipe") || normalized.contains("closed") {
        return ("ws_send_failed", "WebSocket send failed");
    }
    ("metrics_error", "Metrics unavailable")
}

fn make_metrics_error(stream_id: Option<Uuid>, operation: &str, reason: &str) -> StreamMetricsError {
    let (code, summary) = classify_metrics_error(reason);
    let timestamp_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let request_id = Some(Uuid::new_v4().to_string());
    StreamMetricsError {
        stream_id,
        error: summary.to_string(),
        code: Some(code.to_string()),
        detail: Some(reason.to_string()),
        timestamp_ms,
        source: Some("helios-api/ws/streams.metrics".to_string()),
        operation: Some(operation.to_string()),
        request_id: request_id.clone(),
        trace_id: request_id,
        retryable: None,
        remediation: None,
    }
}

fn make_frames_error(operation: &str, reason: &str) -> FramesEvent {
    let timestamp_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let request_id = Some(Uuid::new_v4().to_string());
    FramesEvent::Error {
        error: reason.to_string(),
        code: None,
        timestamp_ms,
        source: Some("helios-api/ws/streams.frames".to_string()),
        operation: Some(operation.to_string()),
        request_id: request_id.clone(),
        trace_id: request_id,
        retryable: None,
        remediation: None,
    }
}

fn record_metrics_error(payload: &StreamMetricsError) {
    record_error_entry(ErrorHistoryEntry {
        id: Uuid::new_v4().to_string(),
        status: None,
        code: payload.code.clone().unwrap_or_else(|| "metrics_error".to_string()),
        error: payload.error.clone(),
        details: payload.detail.clone(),
        timestamp_ms: payload.timestamp_ms,
        source: payload.source.clone(),
        operation: payload.operation.clone(),
        request_id: payload.request_id.clone(),
        trace_id: payload.trace_id.clone(),
        retryable: payload.retryable,
        remediation: payload.remediation.clone(),
        reported_by: Some("helios-api".to_string()),
        transport: Some("ws".to_string()),
    });
}

fn record_frames_error(payload: &FramesEvent) {
    let FramesEvent::Error { error, code, timestamp_ms, source, operation, request_id, trace_id, retryable, remediation } = payload else {
        return;
    };
    record_error_entry(ErrorHistoryEntry {
        id: Uuid::new_v4().to_string(),
        status: None,
        code: code.clone().unwrap_or_else(|| "frames_error".to_string()),
        error: error.clone(),
        details: None,
        timestamp_ms: *timestamp_ms,
        source: source.clone(),
        operation: operation.clone(),
        request_id: request_id.clone(),
        trace_id: trace_id.clone(),
        retryable: *retryable,
        remediation: remediation.clone(),
        reported_by: Some("helios-api".to_string()),
        transport: Some("ws".to_string()),
    });
}

async fn send_metrics_error(sender: &mut WsSender, stream_id: Option<Uuid>, operation: &str, reason: &str) -> Result<(), String> {
    let payload = make_metrics_error(stream_id, operation, reason);
    record_metrics_error(&payload);
    let text = serde_json::to_string(&payload).unwrap_or_else(|_| format!(r#"{{"error":"{reason}"}}"#));
    sender.send(Message::Text(text.into())).await.map_err(|err| err.to_string())
}

pub struct StreamUuidDoc;

pub struct StreamMetricsEventDoc;

pub struct StreamMetricsErrorDoc;

impl SchemaProvider for StreamUuidDoc {
    const NAME: &'static str = "Uuid";
    fn schema() -> serde_json::Value {
        json!({
            "type": "string",
            "format": "uuid"
        })
    }
    fn register_schemas(map: &mut BTreeMap<String, serde_json::Value>) {
        map.entry(Self::NAME.to_string()).or_insert_with(Self::schema);
    }
}

impl SchemaProvider for StreamMetricsEventDoc {
    const NAME: &'static str = "StreamMetricsEvent";
    fn schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "stream_id": { "type": "string", "format": "uuid", "example": "2f1be7d8-5d2d-40a6-a4fb-2ad69d5d4e42" },
                "metrics": { "type": "object", "description": "Metrics payload for the stream" },
                "timestamp_ms": { "type": "integer", "format": "int64", "example": 1_701_000_000_000u64 }
            },
            "required": ["stream_id", "metrics", "timestamp_ms"]
        })
    }
    fn register_schemas(map: &mut BTreeMap<String, serde_json::Value>) {
        map.entry(Self::NAME.to_string()).or_insert_with(Self::schema);
    }
}

impl SchemaProvider for StreamMetricsErrorDoc {
    const NAME: &'static str = "StreamMetricsError";
    fn schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "stream_id": { "type": ["string", "null"], "format": "uuid", "example": "2f1be7d8-5d2d-40a6-a4fb-2ad69d5d4e42" },
                "error": { "type": "string", "example": "metrics unavailable" },
                "code": { "type": "string", "example": "stream_not_found" },
                "detail": { "type": "string", "example": "stream not found" },
                "timestamp_ms": { "type": "integer", "format": "int64", "example": 1_701_000_000_000u64 },
                "source": { "type": "string", "example": "helios-api/ws/streams.metrics" },
                "operation": { "type": "string", "example": "snapshot" },
                "request_id": { "type": "string", "example": "bd4520d3-2e5c-4b21-84f2-9a0d6d3fbf9e" },
                "trace_id": { "type": "string", "example": "bd4520d3-2e5c-4b21-84f2-9a0d6d3fbf9e" },
                "retryable": { "type": "boolean", "example": false },
                "remediation": { "type": "string", "example": "Restart the stream or check engine connectivity." }
            },
            "required": ["error"]
        })
    }
    fn register_schemas(map: &mut BTreeMap<String, serde_json::Value>) {
        map.entry(Self::NAME.to_string()).or_insert_with(Self::schema);
    }
}

pub fn register_docs(host: Option<String>, registry: &mut SchemaRegistry, servers: &mut BTreeMap<String, Server>, tags: &mut Vec<Tag>, docs: &mut Vec<WsDoc>) {
    registry.track::<StreamUuidDoc, _>(StreamUuidDoc::register_schemas);
    registry.track::<StreamMetricsEventDoc, _>(StreamMetricsEventDoc::register_schemas);
    registry.track::<StreamMetricsErrorDoc, _>(StreamMetricsErrorDoc::register_schemas);

    let doc = WsDoc {
        path: "streams.{stream_id}.metrics",
        summary: "Stream metrics",
        description: "Periodic metrics snapshots for an active stream.",
        tags: vec!["streams".into(), "metrics".into()],
        payload: None,
        responses: vec![
            TypeSchema { name: StreamMetricsEventDoc::NAME, schema: StreamMetricsEventDoc::schema() },
            TypeSchema { name: StreamMetricsErrorDoc::NAME, schema: StreamMetricsErrorDoc::schema() },
        ],
        params: vec![("stream_id".into(), TypeSchema { name: StreamUuidDoc::NAME, schema: StreamUuidDoc::schema() })],
    };

    let frames_doc = WsDoc {
        path: "streams.{stream_id}.frames",
        summary: "Stream frames",
        description: "Binary encoded frames for the currently active pipeline output (optionally switched for the socket duration).",
        tags: vec!["streams".into()],
        payload: None,
        responses: vec![],
        params: vec![("stream_id".into(), TypeSchema { name: StreamUuidDoc::NAME, schema: StreamUuidDoc::schema() })],
    };

    let server_host = host.unwrap_or_else(|| "localhost:5800/v1/ws".to_string());
    servers.entry("primary".into()).or_insert(Server { host: server_host, protocol: "ws".into(), protocol_version: None, description: Some("Primary WebSocket entrypoint".into()) });
    docs.push(doc);
    docs.push(frames_doc);
    tags.push(Tag { name: "streams".into(), description: Some("Capture streams and metrics".into()), external_docs: None });
}
