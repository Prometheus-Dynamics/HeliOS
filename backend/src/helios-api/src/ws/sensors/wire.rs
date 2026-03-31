use crate::http::error_history::{ErrorHistoryEntry, record_error_entry};
use axum::extract::ws::{Message, WebSocket};
use serde::Serialize;
use tracing::warn;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub(super) struct WsErrorContext {
    pub(super) request_id: String,
    pub(super) trace_id: String,
}

#[derive(Debug, Serialize)]
struct ErrorPayload {
    status: &'static str,
    reason: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    reported_by: Option<String>,
}

pub(super) async fn send_json_message<T: Serialize>(socket: &mut WebSocket, payload: &T) -> Result<(), ()> {
    let body = serde_json::to_string(payload).map_err(|_| ())?;
    socket.send(Message::Text(body.into())).await.map_err(|_| ())
}

pub(super) async fn send_ws_error(
    socket: &mut WebSocket,
    context: &WsErrorContext,
    reason: impl Into<String>,
    operation: &str,
) {
    let reason = reason.into();
    let payload = ErrorPayload {
        status: "sensors_stream_unavailable",
        reason: reason.clone(),
        code: None,
        timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
        source: Some("helios-api/ws/sensors".to_string()),
        operation: Some(operation.to_string()),
        request_id: Some(context.request_id.clone()),
        trace_id: Some(context.trace_id.clone()),
        retryable: None,
        remediation: None,
        reported_by: Some("helios-api".to_string()),
    };
    record_error_entry(ErrorHistoryEntry {
        id: Uuid::new_v4().to_string(),
        status: None,
        code: "sensors_stream_unavailable".to_string(),
        error: reason,
        details: None,
        timestamp_ms: payload.timestamp_ms,
        source: payload.source.clone(),
        operation: payload.operation.clone(),
        request_id: payload.request_id.clone(),
        trace_id: payload.trace_id.clone(),
        retryable: payload.retryable,
        remediation: payload.remediation.clone(),
        reported_by: payload.reported_by.clone(),
        transport: Some("ws".to_string()),
    });
    match serde_json::to_string(&payload) {
        Ok(body) => {
            let _ = socket.send(Message::Text(body.into())).await;
        }
        Err(err) => {
            warn!(%err, "failed to serialize websocket error payload");
            let _ = socket
                .send(Message::Text(
                    r#"{"status":"sensors_stream_unavailable","reason":"internal error"}"#
                        .to_string()
                        .into(),
                ))
                .await;
        }
    }
}
