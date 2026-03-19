use crate::http::AppState;
use crate::system_read_model::{ProcessSample, SharedProcessesSnapshot};
use axum::{
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use lib_asyncapi::registry::SchemaRegistry;
use lib_asyncapi::{SchemaProvider, Server, Tag, TypeSchema, WsDoc};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio::time::MissedTickBehavior;

const DEFAULT_INTERVAL_MS: u64 = 1_000;
const MIN_INTERVAL_MS: u64 = 250;
const MAX_INTERVAL_MS: u64 = 10_000;
const DEFAULT_LIMIT: usize = 200;
const MIN_LIMIT: usize = 10;
const MAX_LIMIT: usize = 2_000;

#[derive(Debug, Clone, Deserialize)]
pub struct ProcessesParams {
    pub interval_ms: Option<u64>,
    pub limit: Option<usize>,
}

pub async fn processes_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>, Query(params): Query<ProcessesParams>) -> impl IntoResponse {
    let interval_ms = params.interval_ms.unwrap_or(DEFAULT_INTERVAL_MS).clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS);
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(MIN_LIMIT, MAX_LIMIT);
    ws.on_upgrade(move |socket| processes_loop(socket, state, Duration::from_millis(interval_ms), limit))
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProcessesServerEvent {
    Ready { interval_ms: u64 },
    Snapshot { snapshot: ProcessesSnapshot },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProcessesSnapshot {
    pub timestamp_ms: u64,
    pub total_memory_bytes: u64,
    pub used_memory_bytes: u64,
    pub processes: Vec<ProcessSample>,
}

async fn processes_loop(socket: WebSocket, state: AppState, interval: Duration, limit: usize) {
    let (mut tx, mut rx) = socket.split();
    let (mut updates, mut latest) = state.services.system.subscribe_process_snapshots().await;

    let ready = ProcessesServerEvent::Ready { interval_ms: interval.as_millis() as u64 };
    let ready_body = match serde_json::to_string(&ready) {
        Ok(body) => body,
        Err(err) => {
            let evt = ProcessesServerEvent::Error { message: format!("failed to encode ready event: {err}") };
            let _ = tx.send(Message::Text(serde_json::to_string(&evt).unwrap_or_default().into())).await;
            return;
        }
    };
    if tx.send(Message::Text(ready_body.into())).await.is_err() {
        return;
    }

    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last_sent = Instant::now().checked_sub(interval).unwrap_or_else(Instant::now);

    if let Some(snapshot) = latest.take()
        && send_snapshot(&mut tx, snapshot.as_ref(), limit).await.is_err()
    {
        return;
    }

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if let Some(snapshot) = latest.take() {
                    if send_snapshot(&mut tx, snapshot.as_ref(), limit).await.is_err() {
                        break;
                    }
                    last_sent = Instant::now();
                }
            }
            update = updates.recv() => {
                match update {
                    Ok(update) => {
                        latest = Some(update);
                        if last_sent.elapsed() >= interval
                            && let Some(snapshot) = latest.take()
                        {
                            if send_snapshot(&mut tx, snapshot.as_ref(), limit).await.is_err() {
                                break;
                            }
                            last_sent = Instant::now();
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = rx.next() => {
                if matches!(msg, None | Some(Err(_)) | Some(Ok(Message::Close(_)))) {
                    break;
                }
            }
        }
    }
}

async fn send_snapshot(tx: &mut futures::stream::SplitSink<WebSocket, Message>, snapshot: &SharedProcessesSnapshot, limit: usize) -> Result<(), ()> {
    let materialized = ProcessesSnapshot {
        timestamp_ms: snapshot.timestamp_ms,
        total_memory_bytes: snapshot.total_memory_bytes,
        used_memory_bytes: snapshot.used_memory_bytes,
        processes: snapshot.processes.iter().take(limit).cloned().collect(),
    };
    let evt = ProcessesServerEvent::Snapshot { snapshot: materialized };
    let body = match serde_json::to_string(&evt) {
        Ok(body) => body,
        Err(err) => serde_json::to_string(&ProcessesServerEvent::Error { message: format!("failed to encode snapshot event: {err}") }).unwrap_or_default(),
    };
    tx.send(Message::Text(body.into())).await.map_err(|_| ())
}

fn schema<T: JsonSchema>() -> serde_json::Value {
    serde_json::to_value(schema_for!(T)).expect("schema")
}

impl SchemaProvider for ProcessesServerEvent {
    const NAME: &'static str = "ProcessesServerEvent";
    fn schema() -> serde_json::Value {
        schema::<ProcessesServerEvent>()
    }
    fn register_schemas(map: &mut BTreeMap<String, serde_json::Value>) {
        map.entry(Self::NAME.to_string()).or_insert_with(Self::schema);
    }
}

pub fn register_docs(host: Option<String>, registry: &mut SchemaRegistry, servers: &mut BTreeMap<String, Server>, tags: &mut Vec<Tag>, docs: &mut Vec<WsDoc>) {
    registry.track::<ProcessesServerEvent, _>(ProcessesServerEvent::register_schemas);
    let doc = WsDoc {
        path: "processes.stream",
        summary: "Per-process resource usage",
        description: "Periodic snapshots of CPU and memory usage per process.",
        tags: vec!["processes".into()],
        payload: None,
        responses: vec![TypeSchema { name: ProcessesServerEvent::NAME, schema: ProcessesServerEvent::schema() }],
        params: vec![],
    };
    let server_host = host.unwrap_or_else(|| "localhost:5800/v1/ws".to_string());
    servers.entry("primary".into()).or_insert(Server { host: server_host, protocol: "ws".into(), protocol_version: None, description: Some("Primary WebSocket entrypoint".into()) });
    docs.push(doc);
    tags.push(Tag { name: "processes".into(), description: Some("Process resource usage".to_string()), external_docs: None });
}
