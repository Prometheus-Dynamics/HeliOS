use super::hub::{SharedSensorEvent, SharedSensorLatest, SnapshotPayload};
use super::wire::{WsErrorContext, send_json_message, send_ws_error};
use crate::http::AppState;
use axum::extract::ws::{Message, WebSocket};
use helios_peripherals::ipc::FirmwareUpdate;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::time::Duration;
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Kind {
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

#[derive(Debug, Clone, Deserialize)]
struct ClientMessage {
    op: String,
    #[serde(default)]
    kinds: Option<Vec<String>>,
    #[serde(default)]
    interval_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct FirmwarePayload {
    firmware: FirmwareUpdate,
}

pub(super) async fn sensors_loop(mut socket: WebSocket, state: AppState) -> Result<(), String> {
    state.services.hardware.bind_sensor_events_state(&state);

    let error_context = WsErrorContext {
        request_id: Uuid::new_v4().to_string(),
        trace_id: Uuid::new_v4().to_string(),
    };
    let (mut updates, mut latest) = state.services.hardware.subscribe_sensor_events().await;
    if let Some(reason) = latest.error.as_ref() {
        send_ws_error(&mut socket, &error_context, reason.as_ref(), "connect").await;
        return Ok(());
    }

    let mut requested: BTreeSet<Kind> = BTreeSet::new();
    let mut min_interval = Duration::from_millis(100);
    let mut last_sent = tokio::time::Instant::now() - min_interval;

    loop {
        tokio::select! {
            update = updates.recv() => {
                match update {
                    Ok(update) => {
                        match update.as_ref() {
                            SharedSensorEvent::Snapshot(snapshot) => {
                                latest.snapshot = Some(snapshot.clone());
                                latest.error = None;
                                if requested.is_empty() {
                                    continue;
                                }

                                let now = tokio::time::Instant::now();
                                if now.duration_since(last_sent) < min_interval {
                                    continue;
                                }

                                if let Some(payload) = build_payload(&requested, snapshot.as_ref()) {
                                    last_sent = now;
                                    if send_json_message(&mut socket, &payload).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            SharedSensorEvent::Firmware(update) => {
                                latest.firmware = Some(update.clone());
                                latest.error = None;
                                if requested.contains(&Kind::Firmware) {
                                    let payload = FirmwarePayload { firmware: (*update.as_ref()).clone() };
                                    if send_json_message(&mut socket, &payload).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            SharedSensorEvent::Lighting(state) => {
                                latest.lighting = Some(state.clone());
                                latest.error = None;
                                if requested.contains(&Kind::Lighting) {
                                    let payload = SnapshotPayload {
                                        lighting: Some((*state.as_ref()).clone()),
                                        ..SnapshotPayload::default()
                                    };
                                    if send_json_message(&mut socket, &payload).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            SharedSensorEvent::Error(reason) => {
                                send_ws_error(&mut socket, &error_context, reason.as_ref(), "stream").await;
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Text(text))) => {
                        if text.trim().eq_ignore_ascii_case("unsubscribe") {
                            requested.clear();
                            continue;
                        }
                        if let Err(err) = handle_client_message(&text, &mut requested, &mut min_interval) {
                            send_ws_error(&mut socket, &error_context, err, "client_message").await;
                            continue;
                        }

                        if send_cached_for_requested(&mut socket, &requested, &latest, &error_context).await.is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

pub(super) fn handle_client_message(
    text: &str,
    requested: &mut BTreeSet<Kind>,
    min_interval: &mut Duration,
) -> Result<(), String> {
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
                return Err(format!(
                    "unsupported kinds: {} (valid: {})",
                    unknown.join(", "),
                    VALID_KIND_NAMES.join(", ")
                ));
            }
            if accepted == 0 {
                return Err(format!(
                    "subscribe requires at least one valid kind ({})",
                    VALID_KIND_NAMES.join(", ")
                ));
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
                    return Err(format!(
                        "unsupported kinds: {} (valid: {})",
                        unknown.join(", "),
                        VALID_KIND_NAMES.join(", ")
                    ));
                }
                if removed == 0 {
                    return Err(format!(
                        "unsubscribe requires at least one valid kind ({})",
                        VALID_KIND_NAMES.join(", ")
                    ));
                }
            } else {
                requested.clear();
            }
            Ok(())
        }
        other => Err(format!("unsupported op: {other}")),
    }
}

pub(super) fn build_payload(requested: &BTreeSet<Kind>, values: &SnapshotPayload) -> Option<SnapshotPayload> {
    let mut payload = SnapshotPayload::default();
    for kind in requested {
        match kind {
            Kind::Imu => payload.imu = values.imu.clone(),
            Kind::Power => payload.power = values.power.clone(),
            Kind::Firmware => {}
            Kind::Lighting => payload.lighting = values.lighting.clone(),
        }
    }
    if payload.imu.is_none() && payload.power.is_none() && payload.lighting.is_none() {
        None
    } else {
        Some(payload)
    }
}

async fn send_cached_for_requested(
    socket: &mut WebSocket,
    requested: &BTreeSet<Kind>,
    latest: &SharedSensorLatest,
    error_context: &WsErrorContext,
) -> Result<(), ()> {
    if requested.is_empty() {
        return Ok(());
    }

    if let Some(reason) = latest.error.as_ref() {
        send_ws_error(socket, error_context, reason.as_ref(), "connect").await;
        return Err(());
    }

    if let Some(snapshot) = latest.snapshot.as_ref()
        && let Some(payload) = build_payload(requested, snapshot.as_ref())
        && send_json_message(socket, &payload).await.is_err()
    {
        return Err(());
    }

    if requested.contains(&Kind::Lighting)
        && let Some(lighting) = latest.lighting.as_ref()
    {
        let payload = SnapshotPayload {
            lighting: Some((*lighting.as_ref()).clone()),
            ..SnapshotPayload::default()
        };
        if send_json_message(socket, &payload).await.is_err() {
            return Err(());
        }
    }

    if requested.contains(&Kind::Firmware)
        && let Some(update) = latest.firmware.as_ref()
    {
        let payload = FirmwarePayload {
            firmware: (*update.as_ref()).clone(),
        };
        if send_json_message(socket, &payload).await.is_err() {
            return Err(());
        }
    }

    Ok(())
}
