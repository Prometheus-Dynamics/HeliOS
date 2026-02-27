use crate::http::AppState;
use crate::http::streams::util::list_streams_timeout;
use axum::{
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use helios_engine::ipc::StreamState;
use lib_asyncapi::registry::SchemaRegistry;
use lib_asyncapi::{SchemaProvider, Server, Tag, TypeSchema, WsDoc};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::time::MissedTickBehavior;
use tracing::debug;

type WsSender = futures::stream::SplitSink<WebSocket, Message>;

const DEFAULT_INTERVAL_MS: u64 = 2_000;
const MIN_INTERVAL_MS: u64 = 250;
const MAX_INTERVAL_MS: u64 = 10_000;
const MIN_UPDATE_GAP_MS: u64 = 250;

#[derive(Debug, Clone, Deserialize)]
pub struct DevicesUpdatesParams {
    pub interval_ms: Option<u64>,
}

pub async fn devices_updates_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>, Query(params): Query<DevicesUpdatesParams>) -> impl IntoResponse {
    let interval_ms = params.interval_ms.unwrap_or(DEFAULT_INTERVAL_MS).clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS);
    ws.on_upgrade(move |socket| devices_updates_loop(socket, state, Duration::from_millis(interval_ms)))
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum DevicesUpdateReason {
    Api,
    Pipelines,
    Localization,
    Media,
    Imu,
    Device,
    Settings,
    Usb,
    Streams,
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
    let mut updates = state.subscribe_realtime_updates();

    let ready = DevicesUpdatesEvent::Ready { interval_ms: interval.as_millis() as u64 };
    let ready_body = match serde_json::to_string(&ready) {
        Ok(body) => body,
        Err(err) => {
            let evt = DevicesUpdatesEvent::Error { message: format!("failed to encode ready event: {err}") };
            let _ = tx.send(Message::Text(serde_json::to_string(&evt).unwrap_or_default())).await;
            return;
        }
    };
    if tx.send(Message::Text(ready_body)).await.is_err() {
        return;
    }

    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut last_usb = usb_fingerprint();
    let mut last_streams = stream_fingerprint(&state).await.unwrap_or_default();
    let mut pending: BTreeSet<DevicesUpdateReason> = BTreeSet::new();
    let min_gap = Duration::from_millis(MIN_UPDATE_GAP_MS);
    let mut last_sent = Instant::now() - min_gap;

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let next_usb = usb_fingerprint();
                if next_usb != last_usb {
                    last_usb = next_usb;
                    pending.insert(DevicesUpdateReason::Usb);
                }

                if let Some(next_streams) = stream_fingerprint(&state).await
                    && next_streams != last_streams {
                        last_streams = next_streams;
                        pending.insert(DevicesUpdateReason::Streams);
                    }

                if !pending.is_empty() && last_sent.elapsed() >= min_gap {
                    if send_update(&mut tx, &pending).await.is_err() {
                        break;
                    }
                    pending.clear();
                    last_sent = Instant::now();
                }
            }
            update = updates.recv() => {
                match update {
                    Ok(update) => {
                        pending.insert(reason_for_update_kind(&update.kind));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        pending.insert(DevicesUpdateReason::Api);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }

                if !pending.is_empty() && last_sent.elapsed() >= min_gap {
                    if send_update(&mut tx, &pending).await.is_err() {
                        break;
                    }
                    pending.clear();
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

fn reason_for_update_kind(kind: &str) -> DevicesUpdateReason {
    match kind {
        "streams" => DevicesUpdateReason::Streams,
        "pipelines" => DevicesUpdateReason::Pipelines,
        "localization" => DevicesUpdateReason::Localization,
        "media" => DevicesUpdateReason::Media,
        "imu" => DevicesUpdateReason::Imu,
        "device" => DevicesUpdateReason::Device,
        "settings" => DevicesUpdateReason::Settings,
        _ => DevicesUpdateReason::Api,
    }
}

async fn send_update(tx: &mut WsSender, reasons: &BTreeSet<DevicesUpdateReason>) -> Result<(), ()> {
    let payload = DevicesUpdatesEvent::Update { timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64, reasons: reasons.iter().copied().collect() };
    let body = match serde_json::to_string(&payload) {
        Ok(body) => body,
        Err(err) => serde_json::to_string(&DevicesUpdatesEvent::Error { message: format!("failed to encode update event: {err}") }).unwrap_or_default(),
    };
    tx.send(Message::Text(body)).await.map_err(|_| ())
}

fn usb_fingerprint() -> String {
    let root = Path::new("/sys/bus/usb/devices");
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return String::new(),
    };

    let mut keys: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let vendor = fs::read_to_string(path.join("idVendor")).ok();
        let product = fs::read_to_string(path.join("idProduct")).ok();
        let (Some(vendor), Some(product)) = (vendor, product) else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        keys.push(format!("{name}:{}:{}", vendor.trim(), product.trim()));
    }
    keys.sort();
    keys.join("|")
}

async fn stream_fingerprint(state: &AppState) -> Option<String> {
    let streams = match state.engine.list_streams_with_timeout(list_streams_timeout()).await {
        Ok(streams) => streams,
        Err(err) => {
            debug!(%err, "devices updates stream list failed");
            return None;
        }
    };

    let mut keys: Vec<String> = streams.into_iter().map(|summary| format!("{}:{}", summary.stream_id, stream_state_label(summary.status.state))).collect();
    keys.sort();
    Some(keys.join("|"))
}

fn stream_state_label(state: StreamState) -> &'static str {
    match state {
        StreamState::Running => "running",
        StreamState::Disabled => "disabled",
    }
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
