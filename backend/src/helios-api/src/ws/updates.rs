use crate::http::AppState;
use axum::{
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use lib_asyncapi::registry::SchemaRegistry;
use lib_asyncapi::{Server, Tag, TypeSchema, WsDoc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokio::sync::broadcast;
use tokio::time::{Duration, MissedTickBehavior};

const DEFAULT_HEARTBEAT_MS: u64 = 15_000;
const MIN_HEARTBEAT_MS: u64 = 1_000;
const MAX_HEARTBEAT_MS: u64 = 60_000;

#[derive(Debug, Clone, Deserialize)]
pub struct UpdatesParams {
    pub heartbeat_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum UpdatesEvent {
    Ready { heartbeat_ms: u64 },
    Change { event: crate::ipc::RealtimeUpdateEvent },
    Heartbeat { timestamp_ms: u64 },
    Error { message: String },
}

pub async fn updates_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>, Query(params): Query<UpdatesParams>) -> impl IntoResponse {
    let heartbeat_ms = params.heartbeat_ms.unwrap_or(DEFAULT_HEARTBEAT_MS).clamp(MIN_HEARTBEAT_MS, MAX_HEARTBEAT_MS);
    ws.on_upgrade(move |socket| updates_loop(socket, state, Duration::from_millis(heartbeat_ms)))
}

async fn updates_loop(socket: WebSocket, state: AppState, heartbeat: Duration) {
    let (mut tx, mut rx) = socket.split();
    let mut updates = state.subscribe_realtime_updates();
    let mut heartbeat_ticker = tokio::time::interval(heartbeat);
    heartbeat_ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let ready = UpdatesEvent::Ready { heartbeat_ms: heartbeat.as_millis() as u64 };
    if tx.send(Message::Text(serde_json::to_string(&ready).unwrap_or_default().into())).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            update = updates.recv() => {
                match update {
                    Ok(event) => {
                        let body = serde_json::to_string(&UpdatesEvent::Change { event }).unwrap_or_default();
                        if tx.send(Message::Text(body.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        let body = serde_json::to_string(&UpdatesEvent::Error { message: format!("updates stream lagged by {skipped} events") }).unwrap_or_default();
                        if tx.send(Message::Text(body.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = heartbeat_ticker.tick() => {
                let heartbeat_event = UpdatesEvent::Heartbeat { timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64 };
                if tx.send(Message::Text(serde_json::to_string(&heartbeat_event).unwrap_or_default().into())).await.is_err() {
                    break;
                }
            }
            msg = rx.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Ping(bytes))) => {
                        if tx.send(Message::Pong(bytes)).await.is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn realtime_updates_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "oneOf": [
            {
                "type": "object",
                "required": ["type", "heartbeat_ms"],
                "properties": {
                    "type": { "type": "string", "enum": ["ready"] },
                    "heartbeat_ms": { "type": "integer", "minimum": 0 }
                }
            },
            {
                "type": "object",
                "required": ["type", "event"],
                "properties": {
                    "type": { "type": "string", "enum": ["change"] },
                    "event": {
                        "type": "object",
                        "required": ["seq", "timestamp_ms", "origin", "kind", "path"],
                        "properties": {
                            "seq": { "type": "integer", "minimum": 1 },
                            "timestamp_ms": { "type": "integer", "minimum": 0 },
                            "origin": { "type": "string", "enum": ["http", "ws"] },
                            "kind": { "type": "string" },
                            "path": { "type": "string" },
                            "method": { "type": "string" },
                            "request_id": { "type": "string" }
                        }
                    }
                }
            },
            {
                "type": "object",
                "required": ["type", "timestamp_ms"],
                "properties": {
                    "type": { "type": "string", "enum": ["heartbeat"] },
                    "timestamp_ms": { "type": "integer", "minimum": 0 }
                }
            },
            {
                "type": "object",
                "required": ["type", "message"],
                "properties": {
                    "type": { "type": "string", "enum": ["error"] },
                    "message": { "type": "string" }
                }
            }
        ]
    })
}

pub fn register_docs(host: Option<String>, _registry: &mut SchemaRegistry, servers: &mut BTreeMap<String, Server>, tags: &mut Vec<Tag>, docs: &mut Vec<WsDoc>) {
    let doc = WsDoc {
        path: "updates.stream",
        summary: "Global realtime change stream",
        description: "Streams mutating API/WS changes for streams, pipelines, localization, media, and device settings.",
        tags: vec!["updates".into()],
        payload: None,
        responses: vec![TypeSchema { name: "RealtimeUpdatesEvent", schema: realtime_updates_schema() }],
        params: vec![],
    };
    let server_host = host.unwrap_or_else(|| "localhost:5800/v1/ws".to_string());
    servers.entry("primary".into()).or_insert(Server { host: server_host, protocol: "ws".into(), protocol_version: None, description: Some("Primary WebSocket entrypoint".into()) });
    docs.push(doc);
    tags.push(Tag { name: "updates".into(), description: Some("Cross-domain realtime update notifications".to_string()), external_docs: None });
}
