use crate::http::AppState;
use crate::system_read_model::DevicesUpdateReason;
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
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};
use tokio::time::MissedTickBehavior;

type WsSender = futures::stream::SplitSink<WebSocket, Message>;

const DEFAULT_INTERVAL_MS: u64 = 2_000;
const MIN_INTERVAL_MS: u64 = 250;
const MAX_INTERVAL_MS: u64 = 10_000;

#[derive(Debug, Clone, Deserialize)]
pub struct DevicesUpdatesParams {
    pub interval_ms: Option<u64>,
}

pub async fn devices_updates_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>, Query(params): Query<DevicesUpdatesParams>) -> impl IntoResponse {
    let interval_ms = params.interval_ms.unwrap_or(DEFAULT_INTERVAL_MS).clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS);
    state.services.system.bind_devices_updates_state(&state);
    ws.on_upgrade(move |socket| devices_updates_loop(socket, state, Duration::from_millis(interval_ms)))
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DevicesUpdatesEvent {
    Ready { interval_ms: u64 },
    Update { timestamp_ms: u64, reasons: Vec<DevicesUpdateReason> },
    Error { message: String },
}

async fn devices_updates_loop(socket: WebSocket, state: AppState, interval: Duration) {
    let (mut tx, mut rx) = socket.split();
    let mut updates = state.services.system.subscribe_devices_updates().await;

    let ready = DevicesUpdatesEvent::Ready { interval_ms: interval.as_millis() as u64 };
    let ready_body = match serde_json::to_string(&ready) {
        Ok(body) => body,
        Err(err) => {
            let evt = DevicesUpdatesEvent::Error { message: format!("failed to encode ready event: {err}") };
            let _ = tx.send(Message::Text(serde_json::to_string(&evt).unwrap_or_default().into())).await;
            return;
        }
    };
    if tx.send(Message::Text(ready_body.into())).await.is_err() {
        return;
    }

    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut pending: BTreeSet<DevicesUpdateReason> = BTreeSet::new();
    let mut pending_timestamp_ms: Option<u64> = None;
    let mut last_sent = Instant::now().checked_sub(interval).unwrap_or_else(Instant::now);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if !pending.is_empty() {
                    if send_update(&mut tx, pending_timestamp_ms.unwrap_or_else(timestamp_ms), &pending).await.is_err() {
                        break;
                    }
                    pending.clear();
                    pending_timestamp_ms = None;
                    last_sent = Instant::now();
                }
            }
            update = updates.recv() => {
                match update {
                    Ok(update) => {
                        pending.extend(update.reasons.iter().copied());
                        pending_timestamp_ms = Some(update.timestamp_ms);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        pending.insert(DevicesUpdateReason::Api);
                        pending_timestamp_ms = Some(timestamp_ms());
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }

                if !pending.is_empty() && last_sent.elapsed() >= interval {
                    if send_update(&mut tx, pending_timestamp_ms.unwrap_or_else(timestamp_ms), &pending).await.is_err() {
                        break;
                    }
                    pending.clear();
                    pending_timestamp_ms = None;
                    last_sent = Instant::now();
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

async fn send_update(tx: &mut WsSender, timestamp_ms: u64, reasons: &BTreeSet<DevicesUpdateReason>) -> Result<(), ()> {
    let payload = DevicesUpdatesEvent::Update { timestamp_ms, reasons: reasons.iter().copied().collect() };
    let body = match serde_json::to_string(&payload) {
        Ok(body) => body,
        Err(err) => serde_json::to_string(&DevicesUpdatesEvent::Error { message: format!("failed to encode update event: {err}") }).unwrap_or_default(),
    };
    tx.send(Message::Text(body.into())).await.map_err(|_| ())
}

fn timestamp_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

fn schema<T: JsonSchema>() -> serde_json::Value {
    serde_json::to_value(schema_for!(T)).expect("schema")
}

impl SchemaProvider for DevicesUpdatesEvent {
    const NAME: &'static str = "DevicesUpdatesEvent";
    fn schema() -> serde_json::Value {
        schema::<DevicesUpdatesEvent>()
    }
    fn register_schemas(map: &mut BTreeMap<String, serde_json::Value>) {
        map.entry(Self::NAME.to_string()).or_insert_with(Self::schema);
    }
}

pub fn register_docs(host: Option<String>, registry: &mut SchemaRegistry, servers: &mut BTreeMap<String, Server>, tags: &mut Vec<Tag>, docs: &mut Vec<WsDoc>) {
    registry.track::<DevicesUpdatesEvent, _>(DevicesUpdatesEvent::register_schemas);
    let doc = WsDoc {
        path: "devices.updates",
        summary: "Peripheral + stream inventory changes",
        description: "Notifies clients when USB peripherals or registered streams change.",
        tags: vec!["devices".into()],
        payload: None,
        responses: vec![TypeSchema { name: DevicesUpdatesEvent::NAME, schema: DevicesUpdatesEvent::schema() }],
        params: vec![],
    };
    let server_host = host.unwrap_or_else(|| "localhost:5800/v1/ws".to_string());
    servers.entry("primary".into()).or_insert(Server { host: server_host, protocol: "ws".into(), protocol_version: None, description: Some("Primary WebSocket entrypoint".into()) });
    docs.push(doc);
    tags.push(Tag { name: "devices".into(), description: Some("Peripheral + stream inventory updates".to_string()), external_docs: None });
}
