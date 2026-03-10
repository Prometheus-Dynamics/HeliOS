use crate::http::AppState;
use crate::logs::LogSource;
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
use tokio::io::AsyncRead;
use tokio_util::codec::{FramedRead, LinesCodec};

const DEFAULT_LINES: usize = 200;
const MIN_LINES: usize = 0;
const MAX_LINES: usize = 2_000;

#[derive(Debug, Clone, Deserialize)]
pub struct LogsParams {
    pub source: Option<String>,
    pub lines: Option<usize>,
    pub follow: Option<bool>,
}

pub async fn logs_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>, Query(params): Query<LogsParams>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| log_stream(socket, state, params))
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LogsServerEvent {
    Ready { source: Box<LogSource> },
    Line { timestamp_ms: u64, line: String },
    Error { message: String },
    Eof,
}

async fn log_stream(socket: WebSocket, state: AppState, params: LogsParams) {
    let (mut tx, mut rx) = socket.split();

    let follow = params.follow.unwrap_or(true);
    let lines = params.lines.unwrap_or(DEFAULT_LINES).clamp(MIN_LINES, MAX_LINES);
    let source_id = params.source.clone().unwrap_or_else(|| "journal".into());

    let Some(source) = state.services.system.resolve_log_source(&source_id).await else {
        let _ = tx.send(Message::Text(serde_json::to_string(&LogsServerEvent::Error { message: format!("unknown log source: {source_id}") }).unwrap_or_default().into())).await;
        let _ = tx.send(Message::Close(None)).await;
        return;
    };

    if tx.send(Message::Text(serde_json::to_string(&LogsServerEvent::Ready { source: Box::new(source.clone()) }).unwrap_or_default().into())).await.is_err() {
        return;
    }

    let mut child = match state.services.system.spawn_log_stream(&source, lines, follow) {
        Ok(child) => child,
        Err(err) => {
            let _ = tx.send(Message::Text(serde_json::to_string(&LogsServerEvent::Error { message: err }).unwrap_or_default().into())).await;
            let _ = tx.send(Message::Close(None)).await;
            return;
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let mut stdout_lines = stdout.map(|s| FramedRead::new(s, LinesCodec::new()));
    let mut stderr_lines = stderr.map(|s| FramedRead::new(s, LinesCodec::new()));

    loop {
        let stdout_active = stdout_lines.is_some();
        let stderr_active = stderr_lines.is_some();

        tokio::select! {
            Some(msg) = rx.next() => {
                if matches!(msg, Err(_) | Ok(Message::Close(_))) {
                    break;
                }
            }
            line = next_line(&mut stdout_lines), if stdout_active => {
                if let Some(line) = line
                    && send_line(&mut tx, line).await.is_err()
                {
                    break;
                }
            }
            line = next_line(&mut stderr_lines), if stderr_active => {
                if let Some(line) = line
                    && send_line(&mut tx, line).await.is_err()
                {
                    break;
                }
            }
            status = child.wait() => {
                if status.is_err() {
                    let _ = tx.send(Message::Text(serde_json::to_string(&LogsServerEvent::Error { message: "log process failed".into() }).unwrap_or_default().into())).await;
                }
                let _ = tx.send(Message::Text(serde_json::to_string(&LogsServerEvent::Eof).unwrap_or_default().into())).await;
                break;
            }
        }
    }

    drop(child);
}

async fn send_line(tx: &mut futures::stream::SplitSink<WebSocket, Message>, line: String) -> Result<(), ()> {
    let evt = LogsServerEvent::Line { timestamp_ms: chrono::Utc::now().timestamp_millis() as u64, line };
    tx.send(Message::Text(serde_json::to_string(&evt).unwrap_or_default().into())).await.map_err(|_| ())
}

async fn next_line<R: AsyncRead + Unpin>(stream_opt: &mut Option<FramedRead<R, LinesCodec>>) -> Option<String> {
    let stream = stream_opt.as_mut()?;
    match stream.next().await {
        Some(Ok(line)) => Some(line),
        Some(Err(_)) | None => {
            *stream_opt = None;
            None
        }
    }
}

fn schema<T: JsonSchema>() -> serde_json::Value {
    serde_json::to_value(schema_for!(T)).expect("schema")
}

impl SchemaProvider for LogsServerEvent {
    const NAME: &'static str = "LogsServerEvent";
    fn schema() -> serde_json::Value {
        schema::<LogsServerEvent>()
    }
    fn register_schemas(map: &mut BTreeMap<String, serde_json::Value>) {
        map.entry(Self::NAME.to_string()).or_insert_with(Self::schema);
    }
}

pub fn register_docs(host: Option<String>, registry: &mut SchemaRegistry, servers: &mut BTreeMap<String, Server>, tags: &mut Vec<Tag>, docs: &mut Vec<WsDoc>) {
    registry.track::<LogsServerEvent, _>(LogsServerEvent::register_schemas);
    let doc = WsDoc {
        path: "logs.stream",
        summary: "Device logs",
        description: "Stream of log lines from the device.",
        tags: vec!["logs".into()],
        payload: None,
        responses: vec![TypeSchema { name: LogsServerEvent::NAME, schema: LogsServerEvent::schema() }],
        params: vec![],
    };
    let server_host = host.unwrap_or_else(|| "localhost:5800/v1/ws".to_string());
    servers.entry("primary".into()).or_insert(Server { host: server_host, protocol: "ws".into(), protocol_version: None, description: Some("Primary WebSocket entrypoint".into()) });
    docs.push(doc);
    tags.push(Tag { name: "logs".into(), description: Some("Streaming logs".to_string()), external_docs: None });
}
