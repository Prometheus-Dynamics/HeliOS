use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use uuid::Uuid;

use crate::{
    http::AppState,
    http::error_history::{ErrorHistoryEntry, record_error_entry},
    ws::sensors::SharedSensorEvent,
};

pub async fn handle_imu_ws(mut socket: WebSocket, state: AppState) -> Result<(), String> {
    state.services.hardware.bind_sensor_events_state(&state).await;

    let error_context = WsErrorContext { request_id: Uuid::new_v4().to_string(), trace_id: Uuid::new_v4().to_string() };
    let (mut updates, latest): (tokio::sync::broadcast::Receiver<Arc<SharedSensorEvent>>, _) = state.services.hardware.subscribe_sensor_events().await;
    if let Some(reason) = latest.error.as_ref() {
        let reason: &str = reason.as_ref();
        send_ws_error(&mut socket, &error_context, reason, "connect").await;
        return Ok(());
    }

    if let Some(snapshot) = latest.snapshot.as_ref()
        && let Some(payload) = snapshot.imu.as_ref()
        && let Ok(body) = serde_json::to_string(payload)
        && socket.send(Message::Text(body.into())).await.is_err()
    {
        return Ok(());
    }

    loop {
        tokio::select! {
            update = updates.recv() => {
                match update {
                    Ok(update) => match update.as_ref() {
                        SharedSensorEvent::Snapshot(snapshot) => {
                            let Some(payload) = snapshot.imu.as_ref() else { continue };
                            if let Ok(body) = serde_json::to_string(payload)
                                && socket.send(Message::Text(body.into())).await.is_err()
                            {
                                break;
                            }
                        }
                        SharedSensorEvent::Error(reason) => {
                            let reason: &str = reason.as_ref();
                            send_ws_error(&mut socket, &error_context, reason, "stream").await;
                            break;
                        }
                        SharedSensorEvent::Firmware(_) | SharedSensorEvent::Lighting(_) => {}
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        send_ws_error(&mut socket, &error_context, "sensor hub closed", "stream").await;
                        break;
                    }
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Text(text))) if text.trim().eq_ignore_ascii_case("unsubscribe") => break,
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

#[derive(Debug, Default)]
struct WsErrorContext {
    request_id: String,
    trace_id: String,
}

async fn send_ws_error_with_context(socket: &mut WebSocket, context: &WsErrorContext, reason: impl Into<String>, operation: &str) {
    let reason = reason.into();
    let body = serde_json::json!({
        "status": "imu_stream_unavailable",
        "reason": reason.clone(),
        "code": null,
        "timestamp_ms": chrono::Utc::now().timestamp_millis().max(0) as u64,
        "source": "helios-api/http/device/imu.ws",
        "operation": operation,
        "request_id": context.request_id.as_str(),
        "trace_id": context.trace_id.as_str(),
        "retryable": null,
        "remediation": null,
        "reported_by": "helios-api"
    });
    record_error_entry(ErrorHistoryEntry {
        id: Uuid::new_v4().to_string(),
        status: None,
        code: "imu_stream_unavailable".to_string(),
        error: reason,
        details: None,
        timestamp_ms: body.get("timestamp_ms").and_then(|v| v.as_u64()).unwrap_or_default(),
        source: Some("helios-api/http/device/imu.ws".to_string()),
        operation: Some(operation.to_string()),
        request_id: Some(context.request_id.clone()),
        trace_id: Some(context.trace_id.clone()),
        retryable: None,
        remediation: None,
        reported_by: Some("helios-api".to_string()),
        transport: Some("ws".to_string()),
    });
    let _ = socket.send(Message::Text(body.to_string().into())).await;
}

async fn send_ws_error(socket: &mut WebSocket, context: &WsErrorContext, reason: impl Into<String>, operation: &str) {
    send_ws_error_with_context(socket, context, reason, operation).await;
}
