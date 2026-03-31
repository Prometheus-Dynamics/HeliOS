use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use helios_engine::stream::StreamMetrics;
use lib_asyncapi::registry::SchemaRegistry;
use lib_asyncapi::{SchemaProvider, Server, Tag, TypeSchema, WsDoc};
use serde::Serialize;
use serde_json::json;
use tracing::debug;
use uuid::Uuid;

use crate::http::AppState;
use crate::http::error_history::{ErrorHistoryEntry, record_error_entry};
use crate::system_read_model::SharedStreamMetricsSnapshot;

use super::{MAX_INTERVAL_MS, MIN_INTERVAL_MS, WsSender};

pub(super) async fn handle_stream_metrics(socket: WebSocket, state: AppState, stream_id: Uuid, interval: Duration) {
    let (mut sender, mut receiver) = socket.split();
    state.services.system.bind_stream_metrics_state(&state);
    let (mut metrics_rx, latest) = match state.services.system.subscribe_stream_metrics(stream_id).await {
        Ok(subscription) => subscription,
        Err(err) => {
            let _ = send_metrics_error(&mut sender, Some(stream_id), "snapshot", &err).await;
            let _ = sender.close().await;
            return;
        }
    };

    if let Some(snapshot) = latest.as_deref()
        && let Err(err) = send_shared_metrics_payload(&mut sender, snapshot).await
    {
        let _ = send_metrics_error(&mut sender, Some(stream_id), "snapshot", &err).await;
        let _ = sender.close().await;
        return;
    }

    let mut min_gap = interval;
    let mut last_sent = if latest.is_some() { Instant::now() } else { Instant::now().checked_sub(interval).unwrap_or_else(Instant::now) };

    loop {
        tokio::select! {
            event = metrics_rx.recv() => {
                match event {
                    Ok(snapshot) => {
                        if last_sent.elapsed() >= min_gap {
                            if let Err(err) = send_shared_metrics_payload(&mut sender, snapshot.as_ref()).await {
                                let _ = send_metrics_error(&mut sender, Some(stream_id), "send_metrics", &err).await;
                                let _ = sender.close().await;
                                break;
                            }
                            last_sent = Instant::now();
                        }
                    }
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

    drop(metrics_rx);
    state.services.system.unsubscribe_stream_metrics(stream_id).await;
}

fn parse_interval_update(raw: &str) -> Option<u64> {
    let parsed: serde_json::Value = serde_json::from_str(raw).ok()?;
    parsed.get("interval_ms").and_then(|value| value.as_u64())
}

async fn send_shared_metrics_payload(sender: &mut WsSender, snapshot: &SharedStreamMetricsSnapshot) -> Result<(), String> {
    send_metrics_event(sender, StreamMetricsEvent { stream_id: snapshot.stream_id, metrics: snapshot.metrics.clone(), timestamp_ms: snapshot.timestamp_ms }).await
}

async fn send_metrics_event(sender: &mut WsSender, payload: StreamMetricsEvent) -> Result<(), String> {
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

async fn send_metrics_error(sender: &mut WsSender, stream_id: Option<Uuid>, operation: &str, reason: &str) -> Result<(), String> {
    let payload = make_metrics_error(stream_id, operation, reason);
    record_metrics_error(&payload);
    let text = serde_json::to_string(&payload).unwrap_or_else(|_| format!(r#"{{"error":"{reason}"}}"#));
    sender.send(Message::Text(text.into())).await.map_err(|err| err.to_string())
}

pub(super) struct StreamUuidDoc;

pub(super) struct StreamMetricsEventDoc;

pub(super) struct StreamMetricsErrorDoc;

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

pub(super) fn register_docs(host: Option<String>, registry: &mut SchemaRegistry, servers: &mut BTreeMap<String, Server>, tags: &mut Vec<Tag>, docs: &mut Vec<WsDoc>) {
    registry.track::<StreamUuidDoc, _>(StreamUuidDoc::register_schemas);
    registry.track::<StreamMetricsEventDoc, _>(StreamMetricsEventDoc::register_schemas);
    registry.track::<StreamMetricsErrorDoc, _>(StreamMetricsErrorDoc::register_schemas);

    let metrics_doc = WsDoc {
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
    docs.push(metrics_doc);
    docs.push(frames_doc);
    tags.push(Tag { name: "streams".into(), description: Some("Capture streams and metrics".into()), external_docs: None });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_interval_update_extracts_interval_ms() {
        assert_eq!(parse_interval_update(r#"{"interval_ms":750}"#), Some(750));
        assert_eq!(parse_interval_update(r#"{"interval_ms":"slow"}"#), None);
        assert_eq!(parse_interval_update(r#"{"ping":true}"#), None);
        assert_eq!(parse_interval_update("not-json"), None);
    }

    #[test]
    fn classify_metrics_error_maps_expected_codes() {
        assert_eq!(classify_metrics_error("stream not found"), ("stream_not_found", "Stream not found"));
        assert_eq!(classify_metrics_error("engine ipc connection refused"), ("engine_unavailable", "Engine unavailable"));
        assert_eq!(classify_metrics_error("request timed out"), ("metrics_timeout", "Metrics request timed out"));
        assert_eq!(classify_metrics_error("unexpected engine response"), ("engine_unavailable", "Engine unavailable"));
        assert_eq!(classify_metrics_error("socket closed"), ("ws_send_failed", "WebSocket send failed"));
        assert_eq!(classify_metrics_error("something else"), ("metrics_error", "Metrics unavailable"));
    }

    #[test]
    fn make_metrics_error_populates_trace_fields() {
        let payload = make_metrics_error(None, "snapshot", "engine unavailable");
        assert_eq!(payload.stream_id, None);
        assert_eq!(payload.operation.as_deref(), Some("snapshot"));
        assert_eq!(payload.code.as_deref(), Some("engine_unavailable"));
        assert_eq!(payload.error, "Engine unavailable");
        assert_eq!(payload.detail.as_deref(), Some("engine unavailable"));
        assert_eq!(payload.source.as_deref(), Some("helios-api/ws/streams.metrics"));
        assert!(payload.timestamp_ms > 0);
        assert_eq!(payload.request_id, payload.trace_id);
    }

    #[test]
    fn register_docs_emits_metrics_and_frames_channels() {
        let mut registry = SchemaRegistry::default();
        let mut servers: BTreeMap<String, Server> = BTreeMap::new();
        let mut tags = Vec::new();
        let mut docs = Vec::new();

        register_docs(None, &mut registry, &mut servers, &mut tags, &mut docs);

        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].path, "streams.{stream_id}.metrics");
        assert_eq!(docs[1].path, "streams.{stream_id}.frames");
        assert_eq!(servers.get("primary").map(|server| server.host.as_str()), Some("localhost:5800/v1/ws"));
        assert!(tags.iter().any(|tag| tag.name == "streams"));
    }
}
