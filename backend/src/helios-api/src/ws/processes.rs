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
use sysinfo::{ProcessesToUpdate, System};
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

pub async fn processes_upgrade(ws: WebSocketUpgrade, State(_state): State<AppState>, Query(params): Query<ProcessesParams>) -> impl IntoResponse {
    let interval_ms = params.interval_ms.unwrap_or(DEFAULT_INTERVAL_MS).clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS);
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(MIN_LIMIT, MAX_LIMIT);
    ws.on_upgrade(move |socket| processes_loop(socket, Duration::from_millis(interval_ms), limit))
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

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProcessSample {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub virtual_memory_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cmd: Vec<String>,
}

async fn processes_loop(socket: WebSocket, interval: Duration, limit: usize) {
    let (mut tx, mut rx) = socket.split();

    let ready = ProcessesServerEvent::Ready { interval_ms: interval.as_millis() as u64 };
    let ready_body = match serde_json::to_string(&ready) {
        Ok(body) => body,
        Err(err) => {
            let evt = ProcessesServerEvent::Error { message: format!("failed to encode ready event: {err}") };
            let _ = tx.send(Message::Text(serde_json::to_string(&evt).unwrap_or_default())).await;
            return;
        }
    };
    if tx.send(Message::Text(ready_body)).await.is_err() {
        return;
    }

    let mut sys = System::new_all();
    sys.refresh_all();

    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                sys.refresh_processes(ProcessesToUpdate::All, true);
                sys.refresh_memory();
                let snapshot = build_snapshot(&sys, limit);
                let evt = ProcessesServerEvent::Snapshot { snapshot };
                let body = match serde_json::to_string(&evt) {
                    Ok(body) => body,
                    Err(err) => serde_json::to_string(&ProcessesServerEvent::Error { message: format!("failed to encode snapshot event: {err}") }).unwrap_or_default(),
                };
                if tx.send(Message::Text(body)).await.is_err() {
                    break;
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

fn build_snapshot(sys: &System, limit: usize) -> ProcessesSnapshot {
    let cpu_count = sys.cpus().len().max(1) as f32;
    let mut processes: Vec<ProcessSample> = sys
        .processes()
        .iter()
        .map(|(pid, process)| {
            let pid_u32 = pid.as_u32();
            let name = process.name().to_string_lossy().to_string();
            let cpu_percent = (process.cpu_usage() / cpu_count).max(0.0);
            let memory_bytes = process.memory();
            let virtual_memory_bytes = process.virtual_memory();
            let status = Some(format!("{:?}", process.status()));
            let cmd = process.cmd().iter().map(|part| part.to_string_lossy().to_string()).collect();
            ProcessSample { pid: pid_u32, name, cpu_percent, memory_bytes, virtual_memory_bytes, status, cmd }
        })
        .collect();

    processes.sort_by(|a, b| b.cpu_percent.total_cmp(&a.cpu_percent).then_with(|| b.memory_bytes.cmp(&a.memory_bytes)));
    processes.truncate(limit);

    let total_memory_bytes = sys.total_memory();
    let used_memory_bytes = sys.used_memory();
    ProcessesSnapshot { timestamp_ms: chrono::Utc::now().timestamp_millis() as u64, total_memory_bytes, used_memory_bytes, processes }
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
