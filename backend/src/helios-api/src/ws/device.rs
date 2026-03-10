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
use lib_asyncapi::{SchemaProvider, Server, Tag, TypeSchema, WsDoc};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::broadcast;
use tokio::time::MissedTickBehavior;

const DEFAULT_INTERVAL_MS: u64 = 1_000;
const MIN_INTERVAL_MS: u64 = 250;
const MAX_INTERVAL_MS: u64 = 10_000;

#[derive(Debug, Clone, Deserialize)]
pub struct TelemetryParams {
    pub interval_ms: Option<u64>,
}

pub async fn telemetry_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>, Query(params): Query<TelemetryParams>) -> impl IntoResponse {
    let interval_ms = params.interval_ms.unwrap_or(DEFAULT_INTERVAL_MS).clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS);
    state.services.system.bind_telemetry_state(&state);
    ws.on_upgrade(move |socket| telemetry_loop(socket, state, Duration::from_millis(interval_ms)))
}

async fn telemetry_loop(socket: WebSocket, state: AppState, interval: Duration) {
    let (mut tx, mut rx) = socket.split();
    let (mut sampler, initial) = state.services.system.subscribe_telemetry_payloads().await;
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut latest = initial;
    let mut last_sent_at = Instant::now().checked_sub(interval).unwrap_or_else(Instant::now);

    if let Some(sample) = latest.as_ref() {
        if tx.send(Message::Text(sample.as_ref().to_owned().into())).await.is_err() {
            return;
        }
        last_sent_at = Instant::now();
        latest = None;
    }

    loop {
        tokio::select! {
            recv = sampler.recv() => {
                match recv {
                    Ok(sample) => {
                        latest = Some(sample);
                        if last_sent_at.elapsed() >= interval
                            && let Some(sample) = latest.take()
                        {
                            if tx.send(Message::Text(sample.as_ref().to_owned().into())).await.is_err() {
                                break;
                            }
                            last_sent_at = Instant::now();
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = ticker.tick() => {
                if let Some(sample) = latest.take() {
                    if tx.send(Message::Text(sample.as_ref().to_owned().into())).await.is_err() {
                        break;
                    }
                    last_sent_at = Instant::now();
                }
            }
            Some(msg) = rx.next() => {
                if matches!(msg, Err(_) | Ok(Message::Close(_))) {
                    break;
                }
            }
            else => break,
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CpuCoreSample {
    pub id: usize,
    pub usage_percent: f32,
    pub frequency_mhz: Option<u64>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GpuMemorySample {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct NetworkInterfaceSample {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub total_rx_bytes: u64,
    pub total_tx_bytes: u64,
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
    pub mac: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct NetworkSample {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub total_rx_bytes: u64,
    pub total_tx_bytes: u64,
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
    pub interfaces: Vec<NetworkInterfaceSample>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TelemetrySample {
    pub timestamp_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<EngineTelemetry>,
    pub cpu: CpuTelemetry,
    pub memory: MemoryTelemetry,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<GpuTelemetry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk: Option<DiskTelemetry>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disks: Vec<DiskPartitionTelemetry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power: Option<PowerTelemetry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<NetworkSample>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct EngineTelemetry {
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_disconnect_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CpuTelemetry {
    pub usage_percent: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_c: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throttle: Option<CpuThrottleStatus>,
    pub cores: Vec<CpuCoreSample>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CpuThrottleStatus {
    pub raw: u32,
    pub undervoltage: bool,
    pub frequency_capped: bool,
    pub throttled: bool,
    pub soft_temp_limit: bool,
    pub undervoltage_since_boot: bool,
    pub frequency_capped_since_boot: bool,
    pub throttled_since_boot: bool,
    pub soft_temp_limit_since_boot: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MemoryTelemetry {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GpuTelemetry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_percent: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_c: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_mhz: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<GpuMemorySample>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DiskTelemetry {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DiskPartitionTelemetry {
    pub mount: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PowerTelemetry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watts: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volts: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amps: Option<f32>,
}

fn schema<T: JsonSchema>() -> serde_json::Value {
    serde_json::to_value(schema_for!(T)).expect("schema")
}

impl SchemaProvider for TelemetrySample {
    const NAME: &'static str = "TelemetrySample";
    fn schema() -> serde_json::Value {
        schema::<TelemetrySample>()
    }
    fn register_schemas(map: &mut BTreeMap<String, serde_json::Value>) {
        map.entry(Self::NAME.to_string()).or_insert_with(Self::schema);
    }
}

pub fn register_docs(host: Option<String>, registry: &mut SchemaRegistry, servers: &mut BTreeMap<String, Server>, tags: &mut Vec<Tag>, docs: &mut Vec<WsDoc>) {
    registry.track::<TelemetrySample, _>(TelemetrySample::register_schemas);

    let doc = WsDoc {
        path: "device.telemetry",
        summary: "Device telemetry",
        description: "Realtime CPU/memory/disk telemetry for the device.",
        tags: vec!["device".into(), "telemetry".into()],
        payload: None,
        responses: vec![TypeSchema { name: TelemetrySample::NAME, schema: TelemetrySample::schema() }],
        params: vec![],
    };

    let server_host = host.unwrap_or_else(|| "localhost:5800/v1/ws".to_string());
    servers.entry("primary".into()).or_insert(Server { host: server_host, protocol: "ws".into(), protocol_version: None, description: Some("Primary WebSocket entrypoint".into()) });
    docs.push(doc);
    tags.push(Tag { name: "device".into(), description: Some("Device telemetry".to_string()), external_docs: None });
}
