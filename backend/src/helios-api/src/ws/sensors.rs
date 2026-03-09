use std::collections::BTreeSet;
use std::time::Duration;

use crate::http::error_history::{ErrorHistoryEntry, record_error_entry};
use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use helios_peripherals::dto::{SensorScope, SensorSnapshot};
use helios_peripherals::ipc::{FirmwareUpdate, SensorCommand, SensorEvent};
use lib_ipc::types::CommandId;
use serde::{Deserialize, Serialize};
use tracing::{warn, warn_span};
use uuid::Uuid;

use crate::http::AppState;
use crate::http::device::imu::imu_status_from_snapshot;
use crate::http::device::lighting::LightingRuntimeStatePayload;
use crate::http::device::power::{PowerStatusPayload, power_status_from_snapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Imu,
    Power,
    Firmware,
    Lighting,
}

impl Kind {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "imu" => Some(Self::Imu),
            "power" => Some(Self::Power),
            "firmware" => Some(Self::Firmware),
            "lighting" => Some(Self::Lighting),
            _ => None,
        }
    }
}

const VALID_KIND_NAMES: &[&str] = &["imu", "power", "firmware", "lighting"];

#[derive(Debug, Deserialize)]
struct ClientMessage {
    op: String,
    #[serde(default)]
    kinds: Option<Vec<String>>,
    #[serde(default)]
    interval_ms: Option<u64>,
}

#[derive(Debug, Serialize, Default)]
struct SnapshotPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    imu: Option<lib_sensors::dto::ImuStatusPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    power: Option<PowerStatusPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lighting: Option<LightingRuntimeStatePayload>,
}

#[derive(Debug, Serialize)]
struct FirmwarePayload {
    firmware: FirmwareUpdate,
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

#[derive(Debug, Clone)]
struct WsErrorContext {
    request_id: String,
    trace_id: String,
}

pub async fn sensors_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        let span = warn_span!("sensors_ws");
        let _guard = span.enter();
        if let Err(err) = sensors_loop(socket, state).await {
            warn!(%err, "sensors websocket terminated");
        }
    })
}

async fn sensors_loop(mut socket: WebSocket, state: AppState) -> Result<(), String> {
    let error_context = WsErrorContext { request_id: Uuid::new_v4().to_string(), trace_id: Uuid::new_v4().to_string() };
    if state.ensure_sensors().await.is_none() {
        send_ws_error(&mut socket, &error_context, "peripherals IPC unavailable", "connect").await;
        return Ok(());
    }

    let conn = crate::ipc::peripherals::connect_sensors_stream().await.map_err(|err| err.to_string())?;
    let scope = SensorScope::Device;
    let mut session = conn.session;

    let subscribe = SensorCommand::Subscribe { command_id: CommandId::new(), scope: scope.clone() };
    if let Err(err) = session.send_command(conn.client.journal(), &subscribe).await {
        send_ws_error(&mut socket, &error_context, format!("failed to subscribe: {err}"), "subscribe").await;
        return Err(err.to_string());
    }

    let mut requested: BTreeSet<Kind> = BTreeSet::new();
    let mut min_interval = Duration::from_millis(100);
    let mut last_sent = tokio::time::Instant::now() - min_interval;

    loop {
        tokio::select! {
            event = session.next_event() => {
                match event {
                    Ok(Some(SensorEvent::Snapshot { scope: event_scope, values, .. })) if event_scope == scope => {
                        if requested.is_empty() {
                            continue;
                        }

                        let now = tokio::time::Instant::now();
                        if now.duration_since(last_sent) < min_interval {
                            continue;
                        }
                        last_sent = now;

                        if let Some(payload) = build_payload(&requested, &values)
                            && let Ok(body) = serde_json::to_string(&payload)
                            && socket.send(Message::Text(body.into())).await.is_err()
                        {
                            break;
                        }
                    }
                    Ok(Some(SensorEvent::FirmwareUpdate { update })) => {
                        if !requested.contains(&Kind::Firmware) {
                            continue;
                        }
                        let payload = FirmwarePayload { firmware: update };
                        if let Ok(body) = serde_json::to_string(&payload)
                            && socket.send(Message::Text(body.into())).await.is_err()
                        {
                            break;
                        }
                    }
                    Ok(Some(SensorEvent::LightingState { state, .. })) => {
                        if !requested.contains(&Kind::Lighting) {
                            continue;
                        }
                        let payload = SnapshotPayload {
                            lighting: Some(LightingRuntimeStatePayload::from(state)),
                            ..SnapshotPayload::default()
                        };
                        if let Ok(body) = serde_json::to_string(&payload)
                            && socket.send(Message::Text(body.into())).await.is_err()
                        {
                            break;
                        }
                    }
                    Ok(Some(SensorEvent::Nack { reason, .. })) => {
                        send_ws_error(&mut socket, &error_context, reason.clone(), "stream").await;
                        break;
                    }
                    Ok(Some(SensorEvent::Unsubscribed { scope: event_scope })) if event_scope == scope => break,
                    Ok(None) => break,
                    Err(err) => {
                        send_ws_error(&mut socket, &error_context, err.to_string(), "stream").await;
                        break;
                    }
                    _ => {}
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => {
                        let _ = session.send_command(conn.client.journal(), &SensorCommand::Unsubscribe { command_id: CommandId::new(), scope: scope.clone() }).await;
                        break;
                    }
                    Some(Ok(Message::Text(text))) => {
                        if text.trim().eq_ignore_ascii_case("unsubscribe") {
                            requested.clear();
                            continue;
                        }
                        if let Err(err) = handle_client_message(&text, &mut requested, &mut min_interval) {
                            send_ws_error(&mut socket, &error_context, err, "client_message").await;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

fn handle_client_message(text: &str, requested: &mut BTreeSet<Kind>, min_interval: &mut Duration) -> Result<(), String> {
    let msg: ClientMessage = serde_json::from_str(text).map_err(|err| format!("invalid message: {err}"))?;
    let op = msg.op.trim().to_ascii_lowercase();

    if let Some(interval_ms) = msg.interval_ms {
        if !(50..=10_000).contains(&interval_ms) {
            return Err("interval_ms must be between 50 and 10000".into());
        }
        *min_interval = Duration::from_millis(interval_ms);
    }

    match op.as_str() {
        "subscribe" => {
            let kinds = msg.kinds.unwrap_or_default();
            if kinds.is_empty() {
                return Err("subscribe requires kinds".into());
            }
            let mut accepted = 0usize;
            let mut unknown = Vec::new();
            for kind in kinds {
                if let Some(kind) = Kind::parse(&kind) {
                    requested.insert(kind);
                    accepted += 1;
                } else {
                    unknown.push(kind);
                }
            }
            if !unknown.is_empty() {
                return Err(format!("unsupported kinds: {} (valid: {})", unknown.join(", "), VALID_KIND_NAMES.join(", ")));
            }
            if accepted == 0 {
                return Err(format!("subscribe requires at least one valid kind ({})", VALID_KIND_NAMES.join(", ")));
            }
            Ok(())
        }
        "unsubscribe" => {
            if let Some(kinds) = msg.kinds {
                let mut removed = 0usize;
                let mut unknown = Vec::new();
                for kind in kinds {
                    if let Some(kind) = Kind::parse(&kind) {
                        requested.remove(&kind);
                        removed += 1;
                    } else {
                        unknown.push(kind);
                    }
                }
                if !unknown.is_empty() {
                    return Err(format!("unsupported kinds: {} (valid: {})", unknown.join(", "), VALID_KIND_NAMES.join(", ")));
                }
                if removed == 0 {
                    return Err(format!("unsubscribe requires at least one valid kind ({})", VALID_KIND_NAMES.join(", ")));
                }
            } else {
                requested.clear();
            }
            Ok(())
        }
        other => Err(format!("unsupported op: {other}")),
    }
}

fn build_payload(requested: &BTreeSet<Kind>, values: &SensorSnapshot) -> Option<SnapshotPayload> {
    let mut payload = SnapshotPayload::default();
    for kind in requested {
        match kind {
            Kind::Imu => payload.imu = Some(imu_status_from_snapshot(values)),
            Kind::Power => payload.power = Some(power_status_from_snapshot(values)),
            Kind::Firmware => {}
            Kind::Lighting => {}
        }
    }
    if payload.imu.is_none() && payload.power.is_none() && payload.lighting.is_none() { None } else { Some(payload) }
}

async fn send_ws_error(socket: &mut WebSocket, context: &WsErrorContext, reason: impl Into<String>, operation: &str) {
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
            let _ = socket.send(Message::Text(r#"{"status":"sensors_stream_unavailable","reason":"internal error"}"#.to_string().into())).await;
        }
    }
}
