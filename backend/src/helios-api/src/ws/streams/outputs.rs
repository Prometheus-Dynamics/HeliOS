use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tracing::debug;
use uuid::Uuid;

use crate::http::AppState;
use crate::system_read_model::{SharedStreamOutputSample, SharedStreamOutputsEvent, SharedStreamOutputsPortsSnapshot};

use super::{DEFAULT_OUTPUT_SAMPLE_INTERVAL_MS, MAX_OUTPUT_SAMPLE_INTERVAL_MS, MIN_OUTPUT_SAMPLE_INTERVAL_MS, WsSender};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamOutputsRequest {
    Ping {
        #[serde(default)]
        request_id: Option<String>,
    },
    Subscribe {
        ports: Vec<String>,
        #[serde(default)]
        interval_ms: Option<u64>,
        #[serde(default)]
        request_id: Option<String>,
    },
    List {
        #[serde(default)]
        request_id: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
struct StreamOutputsList {
    outputs: Vec<helios_engine::ipc::GraphOutputPortDescriptor>,
    timestamp_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct StreamOutputSampleEvent {
    port: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamOutputsResponse {
    Ack {
        #[serde(default)]
        request_id: Option<String>,
    },
    Error {
        #[serde(default)]
        request_id: Option<String>,
        error: String,
    },
}

#[derive(Debug, Clone)]
struct OutputsSubscription {
    ports: Vec<String>,
    interval: Duration,
}

pub(super) async fn handle_stream_outputs(socket: WebSocket, state: AppState, stream_id: Uuid, default_interval: Duration, ports_interval: Duration) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    state.services.system.bind_stream_outputs_state(&state);
    let (client_id, mut outputs_rx, initial_ports) = match state.services.system.subscribe_stream_outputs(stream_id, default_interval, ports_interval).await {
        Ok(subscription) => subscription,
        Err(err) => {
            let _ = send_stream_outputs_error(&mut ws_sender, None, err).await;
            let _ = ws_sender.close().await;
            return;
        }
    };

    let mut subscription = OutputsSubscription { ports: Vec::new(), interval: default_interval };
    let mut last_sample_sent = BTreeMap::<String, Instant>::new();

    if let Err(err) = send_stream_outputs_list(&mut ws_sender, &initial_ports, None).await {
        let _ = send_stream_outputs_error(&mut ws_sender, None, err).await;
        let _ = ws_sender.close().await;
        drop(outputs_rx);
        state.services.system.unsubscribe_stream_outputs(stream_id, client_id).await;
        return;
    }

    loop {
        tokio::select! {
            event = outputs_rx.recv() => {
                match event {
                    Ok(event) => match event.as_ref() {
                        SharedStreamOutputsEvent::Ports(snapshot) => {
                            if send_stream_outputs_list(&mut ws_sender, snapshot, None).await.is_err() {
                                break;
                            }
                        }
                        SharedStreamOutputsEvent::Sample(sample) => {
                            if !subscription.ports.iter().any(|port| port == &sample.port) {
                                continue;
                            }
                            let now = Instant::now();
                            let allow_send = last_sample_sent
                                .get(&sample.port)
                                .map(|last_sent| now.duration_since(*last_sent) >= subscription.interval)
                                .unwrap_or(true);
                            if !allow_send {
                                continue;
                            }
                            if send_stream_output_sample(&mut ws_sender, sample).await.is_err() {
                                break;
                            }
                            last_sample_sent.insert(sample.port.clone(), now);
                        }
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = ws_receiver.next() => {
                let payload = match msg {
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(Message::Ping(bytes))) => {
                        if ws_sender.send(Message::Pong(bytes)).await.is_err() {
                            break;
                        }
                        continue;
                    }
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => continue,
                    Some(Err(err)) => {
                        debug!(error = %err, "stream outputs websocket closed");
                        break;
                    }
                    None => break,
                };

                let parsed = serde_json::from_str::<StreamOutputsRequest>(&payload);
                match parsed {
                    Ok(StreamOutputsRequest::Ping { request_id }) => {
                        if send_stream_outputs_response(&mut ws_sender, StreamOutputsResponse::Ack { request_id }).await.is_err() {
                            break;
                        }
                    }
                    Ok(StreamOutputsRequest::List { request_id }) => {
                        match state.services.system.current_stream_outputs_ports(stream_id).await {
                            Ok(snapshot) => {
                                if send_stream_outputs_list(&mut ws_sender, &snapshot, request_id).await.is_err() {
                                    break;
                                }
                            }
                            Err(err) => {
                                if send_stream_outputs_error(&mut ws_sender, request_id, err).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Ok(StreamOutputsRequest::Subscribe { ports, interval_ms, request_id }) => {
                        let normalized = normalize_subscription_ports(ports);
                        let interval = clamp_subscription_interval(interval_ms);
                        match state
                            .services
                            .system
                            .update_stream_outputs_subscription(stream_id, client_id, normalized.clone(), interval)
                            .await
                        {
                            Ok(()) => {
                                subscription = OutputsSubscription { ports: normalized, interval };
                                last_sample_sent.clear();
                                if send_stream_outputs_response(&mut ws_sender, StreamOutputsResponse::Ack { request_id }).await.is_err() {
                                    break;
                                }
                            }
                            Err(err) => {
                                if send_stream_outputs_error(&mut ws_sender, request_id, err).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Err(err) => {
                        if send_stream_outputs_error(&mut ws_sender, None, format!("invalid request: {err}")).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    }

    drop(outputs_rx);
    state.services.system.unsubscribe_stream_outputs(stream_id, client_id).await;
    let _ = ws_sender.close().await;
}

async fn send_stream_outputs_list(sender: &mut WsSender, snapshot: &SharedStreamOutputsPortsSnapshot, request_id: Option<String>) -> Result<(), String> {
    send_stream_outputs_message(sender, &StreamOutputsList { outputs: snapshot.outputs.clone(), timestamp_ms: snapshot.timestamp_ms, request_id }).await
}

async fn send_stream_output_sample(sender: &mut WsSender, sample: &SharedStreamOutputSample) -> Result<(), String> {
    send_stream_outputs_message(sender, &StreamOutputSampleEvent { port: sample.port.clone(), value: sample.value.clone(), error: sample.error.clone(), timestamp_ms: sample.timestamp_ms }).await
}

async fn send_stream_outputs_response(sender: &mut WsSender, response: StreamOutputsResponse) -> Result<(), String> {
    send_stream_outputs_message(sender, &response).await
}

async fn send_stream_outputs_error(sender: &mut WsSender, request_id: Option<String>, error: impl Into<String>) -> Result<(), String> {
    send_stream_outputs_response(sender, StreamOutputsResponse::Error { request_id, error: error.into() }).await
}

async fn send_stream_outputs_message(sender: &mut WsSender, payload: &impl Serialize) -> Result<(), String> {
    let text = serde_json::to_string(payload).map_err(|err| err.to_string())?;
    sender.send(Message::Text(text.into())).await.map_err(|err| err.to_string())
}

fn normalize_subscription_ports(ports: Vec<String>) -> Vec<String> {
    let mut normalized: Vec<String> = ports.into_iter().map(|port| port.trim().to_string()).filter(|port| !port.is_empty()).collect();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn clamp_subscription_interval(interval_ms: Option<u64>) -> Duration {
    Duration::from_millis(interval_ms.unwrap_or(DEFAULT_OUTPUT_SAMPLE_INTERVAL_MS).clamp(MIN_OUTPUT_SAMPLE_INTERVAL_MS, MAX_OUTPUT_SAMPLE_INTERVAL_MS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_subscription_ports_trims_sorts_and_dedups() {
        let normalized = normalize_subscription_ports(vec![" gain ".to_string(), "".to_string(), "frame".to_string(), "gain".to_string(), "  ".to_string(), "alpha".to_string()]);
        assert_eq!(normalized, vec!["alpha".to_string(), "frame".to_string(), "gain".to_string()]);
    }

    #[test]
    fn clamp_subscription_interval_respects_defaults_and_limits() {
        assert_eq!(clamp_subscription_interval(None), Duration::from_millis(DEFAULT_OUTPUT_SAMPLE_INTERVAL_MS));
        assert_eq!(clamp_subscription_interval(Some(MIN_OUTPUT_SAMPLE_INTERVAL_MS.saturating_sub(1))), Duration::from_millis(MIN_OUTPUT_SAMPLE_INTERVAL_MS));
        assert_eq!(clamp_subscription_interval(Some(MAX_OUTPUT_SAMPLE_INTERVAL_MS + 1)), Duration::from_millis(MAX_OUTPUT_SAMPLE_INTERVAL_MS));
        assert_eq!(clamp_subscription_interval(Some(777)), Duration::from_millis(777));
    }
}
