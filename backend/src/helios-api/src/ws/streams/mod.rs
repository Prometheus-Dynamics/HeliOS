mod commands;
mod frames;
mod metrics;
mod outputs;

use crate::http::AppState;
use axum::{
    Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use std::time::Duration;
use uuid::Uuid;

use self::commands::{handle_stream_controls, handle_stream_updates};
use self::frames::handle_stream_frames;
use self::metrics::{handle_stream_metrics, register_docs as register_metrics_docs};
use self::outputs::handle_stream_outputs;

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

#[derive(Debug, Clone, serde::Deserialize)]
pub struct MetricsWsParams {
    pub interval_ms: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct FramesWsParams {
    /// Host bridge output port name to forward to the encoder/preview (defaults to `frame`).
    pub output: Option<String>,
    /// Restore the previous pipeline/output selection when the socket disconnects.
    pub restore: Option<bool>,
    /// Shmem poll interval in ms.
    pub poll_ms: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct OutputsWsParams {
    pub interval_ms: Option<u64>,
    pub ports_interval_ms: Option<u64>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{id}/metrics", get(stream_metrics))
        .route("/{id}/frames", get(stream_frames))
        .route("/{id}/updates", get(stream_updates))
        .route("/{id}/controls", get(stream_controls))
        .route("/{id}/outputs", get(stream_outputs))
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

pub fn register_docs(
    host: Option<String>,
    registry: &mut lib_asyncapi::registry::SchemaRegistry,
    servers: &mut std::collections::BTreeMap<String, lib_asyncapi::Server>,
    tags: &mut Vec<lib_asyncapi::Tag>,
    docs: &mut Vec<lib_asyncapi::WsDoc>,
) {
    register_metrics_docs(host, registry, servers, tags, docs);
}
