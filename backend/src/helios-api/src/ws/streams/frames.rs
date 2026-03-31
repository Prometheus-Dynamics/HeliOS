use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use helios_engine::ipc::EngineEvent;
use helios_engine::stream::{read_latest_frame_with_header_if_newer_than, touch_stream_viewer};
use serde::Serialize;
use uuid::Uuid;

use crate::http::AppState;
use crate::http::error_history::{ErrorHistoryEntry, record_error_entry};

use super::FramesWsParams;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
enum FramesEvent {
    #[serde(rename = "format")]
    Format { fourcc: String, width: u32, height: u32 },
    #[serde(rename = "error")]
    Error {
        error: String,
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
    },
}

pub(super) async fn handle_stream_frames(socket: WebSocket, state: AppState, stream_id: Uuid, params: FramesWsParams, poll: Duration) {
    let (mut sender, mut receiver) = socket.split();

    let prev_output = match snapshot_stream_output(&state, stream_id).await {
        Ok(snapshot) => snapshot,
        Err(err) => {
            let payload = make_frames_error("snapshot_stream_output", &err);
            record_frames_error(&payload);
            let _ = sender.send(Message::Text(serde_json::to_string(&payload).unwrap_or_else(|_| format!(r#"{{"type":"error","error":"{err}"}}"#)).into())).await;
            let _ = sender.close().await;
            return;
        }
    };

    let requested_output = params.output.clone().or(prev_output.clone()).unwrap_or_else(|| "frame".to_string());
    let restore = params.restore.unwrap_or(true);

    if let Err(err) = set_stream_output(&state, stream_id, Some(requested_output.clone())).await {
        let payload = make_frames_error("set_stream_output", &err);
        record_frames_error(&payload);
        let _ = sender.send(Message::Text(serde_json::to_string(&payload).unwrap_or_else(|_| format!(r#"{{"type":"error","error":"{err}"}}"#)).into())).await;
        let _ = sender.close().await;
        return;
    }

    let (frame_tx, mut frame_rx) = tokio::sync::mpsc::channel::<(helios_engine::stream::ShmemFrameHeader, Vec<u8>)>(1);
    tokio::task::spawn_blocking(move || {
        let mut last_seq = 0u64;
        let mut last_touch = Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(Instant::now);
        loop {
            if frame_tx.is_closed() {
                break;
            }
            if last_touch.elapsed() >= Duration::from_millis(500) {
                let _ = touch_stream_viewer(stream_id);
                last_touch = Instant::now();
            }
            let next = match read_latest_frame_with_header_if_newer_than(stream_id, last_seq) {
                Ok(next) => next,
                Err(_) => {
                    std::thread::sleep(poll);
                    continue;
                }
            };
            let Some((hdr, payload)) = next else {
                std::thread::sleep(poll);
                continue;
            };
            if hdr.len == 0 || hdr.fourcc.to_u32() == 0 || hdr.seq == last_seq {
                std::thread::sleep(poll);
                continue;
            }
            last_seq = hdr.seq;
            if frame_tx.try_send((hdr, payload)).is_err() {
                std::thread::sleep(poll);
            }
        }
    });

    let mut last_format: Option<(u32, u32, u32)> = None;

    loop {
        tokio::select! {
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(Ok(Message::Ping(bytes))) => {
                        if sender.send(Message::Pong(bytes)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Text(_))) | Some(Ok(Message::Binary(_))) | Some(Ok(Message::Pong(_))) => {}
                }
            }
            maybe_frame = frame_rx.recv() => {
                let Some((hdr, payload)) = maybe_frame else { break; };
                let format = (hdr.fourcc.to_u32(), hdr.width, hdr.height);
                if last_format != Some(format) {
                    last_format = Some(format);
                    let event = FramesEvent::Format {
                        fourcc: hdr.fourcc.to_string(),
                        width: hdr.width,
                        height: hdr.height,
                    };
                    if sender
                        .send(Message::Text(
                            serde_json::to_string(&event)
                                .unwrap_or_else(|_| r#"{"type":"format"}"#.to_string())
                                .into(),
                        ))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                if sender.send(Message::Binary(payload.into())).await.is_err() {
                    break;
                }
            }
        }
    }

    drop(frame_rx);

    if restore {
        let _ = set_stream_output(&state, stream_id, prev_output).await;
    }
}

async fn snapshot_stream_output(state: &AppState, stream_id: Uuid) -> Result<Option<String>, String> {
    let streams = state.engine.list_streams().await.map_err(|err| err.to_string())?;
    let summary = streams.into_iter().find(|stream| stream.stream_id == stream_id).ok_or_else(|| "stream not found".to_string())?;
    let manifest = summary.manifest;

    if manifest.pipeline_enabled == Some(false) {
        return Ok(None);
    }

    let output = manifest
        .active_pipeline_output
        .clone()
        .or_else(|| manifest.active_pipeline_id.and_then(|active_id| manifest.pipelines.iter().find(|pipeline| pipeline.pipeline_id == active_id)).and_then(|binding| binding.pipeline_output.clone()))
        .or_else(|| manifest.pipelines.first().and_then(|binding| binding.pipeline_output.clone()));

    Ok(output)
}

async fn set_stream_output(state: &AppState, stream_id: Uuid, output: Option<String>) -> Result<(), String> {
    match state.engine.set_graph_output(stream_id, output).await {
        Ok(EngineEvent::Ack { .. }) => Ok(()),
        Ok(EngineEvent::Nack { reason, .. }) => Err(reason),
        Ok(_) => Err("unexpected engine response".into()),
        Err(err) => Err(err.to_string()),
    }
}

fn make_frames_error(operation: &str, reason: &str) -> FramesEvent {
    let timestamp_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let request_id = Some(Uuid::new_v4().to_string());
    FramesEvent::Error {
        error: reason.to_string(),
        code: None,
        timestamp_ms,
        source: Some("helios-api/ws/streams.frames".to_string()),
        operation: Some(operation.to_string()),
        request_id: request_id.clone(),
        trace_id: request_id,
        retryable: None,
        remediation: None,
    }
}

fn record_frames_error(payload: &FramesEvent) {
    let FramesEvent::Error { error, code, timestamp_ms, source, operation, request_id, trace_id, retryable, remediation } = payload else {
        return;
    };
    record_error_entry(ErrorHistoryEntry {
        id: Uuid::new_v4().to_string(),
        status: None,
        code: code.clone().unwrap_or_else(|| "frames_error".to_string()),
        error: error.clone(),
        details: None,
        timestamp_ms: *timestamp_ms,
        source: source.clone(),
        operation: operation.clone(),
        request_id: request_id.clone(),
        trace_id: trace_id.clone(),
        retryable: *retryable,
        remediation: remediation.clone(),
        reported_by: Some("helios-api".to_string()),
        transport: Some("ws".to_string()),
    });
}
