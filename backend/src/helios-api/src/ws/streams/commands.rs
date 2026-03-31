use std::collections::BTreeMap;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use futures::StreamExt;
use helios_engine::capture::CaptureControlValue;
use serde::{Deserialize, Serialize};
use tracing::debug;
use uuid::Uuid;

use crate::http::AppState;
use crate::stream_command_service;

use super::CONTROL_APPLY_INTERVAL_MS;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamUpdateRequest {
    SetGraph {
        #[serde(default)]
        request_id: Option<String>,
        graph: serde_json::Value,
        #[serde(default)]
        pipeline_id: Option<Uuid>,
        #[serde(default)]
        output: Option<String>,
    },
    SetGraphPatch {
        #[serde(default)]
        request_id: Option<String>,
        patch: serde_json::Value,
        #[serde(default)]
        pipeline_id: Option<Uuid>,
    },
    SetInputs {
        #[serde(default)]
        request_id: Option<String>,
        #[serde(default)]
        pipeline_id: Option<Uuid>,
        inputs: BTreeMap<String, Option<serde_json::Value>>,
    },
    Ping {
        #[serde(default)]
        request_id: Option<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamUpdateResponse {
    Ack {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    Error {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        error: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamControlRequest {
    SetControl {
        #[serde(default)]
        request_id: Option<String>,
        control_id: u32,
        value: CaptureControlValue,
    },
    Ping {
        #[serde(default)]
        request_id: Option<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamControlResponse {
    Ack {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    Error {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        error: String,
    },
}

pub(super) async fn handle_stream_updates(mut socket: WebSocket, state: AppState, stream_id: Uuid) {
    while let Some(msg) = socket.next().await {
        let payload = match msg {
            Ok(Message::Text(text)) => text,
            Ok(Message::Ping(bytes)) => {
                let _ = socket.send(Message::Pong(bytes)).await;
                continue;
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => continue,
            Err(err) => {
                debug!(error = %err, "stream updates websocket closed");
                break;
            }
        };

        let parsed = serde_json::from_str::<StreamUpdateRequest>(&payload);
        let response = match parsed {
            Ok(StreamUpdateRequest::Ping { request_id }) => StreamUpdateResponse::Ack { request_id },
            Ok(StreamUpdateRequest::SetGraph { request_id, graph, pipeline_id, output }) => {
                match stream_command_service::apply_stream_graph_update(&state, stream_id, graph, pipeline_id, output).await {
                    Ok(()) => {
                        state.publish_realtime_update(
                            crate::ipc::RealtimeUpdateOrigin::Ws,
                            crate::ipc::RealtimeUpdateKind::StreamsPipeline,
                            format!("/v1/ws/streams/{stream_id}/updates"),
                            Some("set_graph".to_string()),
                            request_id.clone(),
                        );
                        StreamUpdateResponse::Ack { request_id }
                    }
                    Err(err) => StreamUpdateResponse::Error { request_id, error: err },
                }
            }
            Ok(StreamUpdateRequest::SetGraphPatch { request_id, patch, pipeline_id }) => match stream_command_service::apply_stream_graph_patch(&state, stream_id, patch, pipeline_id).await {
                Ok(()) => {
                    state.publish_realtime_update(
                        crate::ipc::RealtimeUpdateOrigin::Ws,
                        crate::ipc::RealtimeUpdateKind::StreamsPipeline,
                        format!("/v1/ws/streams/{stream_id}/updates"),
                        Some("set_graph_patch".to_string()),
                        request_id.clone(),
                    );
                    StreamUpdateResponse::Ack { request_id }
                }
                Err(err) => StreamUpdateResponse::Error { request_id, error: err },
            },
            Ok(StreamUpdateRequest::SetInputs { request_id, pipeline_id, inputs }) => match stream_command_service::apply_stream_inputs(&state, stream_id, pipeline_id, inputs).await {
                Ok(()) => {
                    state.publish_realtime_update(
                        crate::ipc::RealtimeUpdateOrigin::Ws,
                        crate::ipc::RealtimeUpdateKind::StreamsPipeline,
                        format!("/v1/ws/streams/{stream_id}/updates"),
                        Some("set_inputs".to_string()),
                        request_id.clone(),
                    );
                    StreamUpdateResponse::Ack { request_id }
                }
                Err(err) => StreamUpdateResponse::Error { request_id, error: err },
            },
            Err(err) => StreamUpdateResponse::Error { request_id: None, error: format!("invalid request: {err}") },
        };

        if let Ok(text) = serde_json::to_string(&response)
            && socket.send(Message::Text(text.into())).await.is_err()
        {
            break;
        }
    }
}

pub(super) async fn handle_stream_controls(mut socket: WebSocket, state: AppState, stream_id: Uuid) {
    let mut worker = stream_command_service::spawn_stream_controls_worker(state.clone(), stream_id, Duration::from_millis(CONTROL_APPLY_INTERVAL_MS));

    loop {
        tokio::select! {
            msg = socket.next() => {
                let payload = match msg {
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(Message::Ping(bytes))) => {
                        if socket.send(Message::Pong(bytes)).await.is_err() {
                            break;
                        }
                        continue;
                    }
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => continue,
                    Some(Err(err)) => {
                        debug!(error = %err, "stream controls websocket closed");
                        break;
                    }
                    None => break,
                };

                let parsed = serde_json::from_str::<StreamControlRequest>(&payload);
                let immediate = match parsed {
                    Ok(StreamControlRequest::Ping { request_id }) => Some(StreamControlResponse::Ack { request_id }),
                    Ok(StreamControlRequest::SetControl { request_id, control_id, value }) => {
                        worker.enqueue(control_id, request_id, value).await;
                        None
                    }
                    Err(err) => Some(StreamControlResponse::Error {
                        request_id: None,
                        error: format!("invalid request: {err}"),
                    }),
                };

                if let Some(response) = immediate
                    && let Ok(text) = serde_json::to_string(&response)
                    && socket.send(Message::Text(text.into())).await.is_err()
                {
                    break;
                }
            }
            response = worker.next_result() => {
                let Some(response) = response else {
                    break;
                };
                let response = match response.result {
                    Ok(()) => StreamControlResponse::Ack { request_id: response.request_id },
                    Err(error) => StreamControlResponse::Error { request_id: response.request_id, error },
                };
                if let Ok(text) = serde_json::to_string(&response)
                    && socket.send(Message::Text(text.into())).await.is_err()
                {
                    break;
                }
            }
        }
    }

    worker.shutdown();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stream_update_request_parses_graph_mutations() {
        let set_graph = serde_json::from_value::<StreamUpdateRequest>(json!({
            "type": "set_graph",
            "request_id": "req-1",
            "graph": { "nodes": [] },
            "pipeline_id": "11111111-1111-1111-1111-111111111111",
            "output": "threshold"
        }))
        .expect("set_graph request should parse");
        match set_graph {
            StreamUpdateRequest::SetGraph { request_id, graph, pipeline_id, output } => {
                assert_eq!(request_id.as_deref(), Some("req-1"));
                assert_eq!(pipeline_id, Some(Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap()));
                assert_eq!(output.as_deref(), Some("threshold"));
                assert_eq!(graph, json!({ "nodes": [] }));
            }
            other => panic!("expected set_graph request, got {other:?}"),
        }

        let set_patch = serde_json::from_value::<StreamUpdateRequest>(json!({
            "type": "set_graph_patch",
            "patch": { "ops": [] }
        }))
        .expect("set_graph_patch request should parse");
        match set_patch {
            StreamUpdateRequest::SetGraphPatch { request_id, patch, pipeline_id } => {
                assert_eq!(request_id, None);
                assert_eq!(pipeline_id, None);
                assert_eq!(patch, json!({ "ops": [] }));
            }
            other => panic!("expected set_graph_patch request, got {other:?}"),
        }
    }

    #[test]
    fn stream_update_request_parses_inputs_and_ping() {
        let inputs = serde_json::from_value::<StreamUpdateRequest>(json!({
            "type": "set_inputs",
            "request_id": "req-2",
            "pipeline_id": "22222222-2222-2222-2222-222222222222",
            "inputs": {
                "gain": 3,
                "mask": null
            }
        }))
        .expect("set_inputs request should parse");
        match inputs {
            StreamUpdateRequest::SetInputs { request_id, pipeline_id, inputs } => {
                assert_eq!(request_id.as_deref(), Some("req-2"));
                assert_eq!(pipeline_id, Some(Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap()));
                assert_eq!(inputs.get("gain"), Some(&Some(json!(3))));
                assert_eq!(inputs.get("mask"), Some(&None));
            }
            other => panic!("expected set_inputs request, got {other:?}"),
        }

        let ping = serde_json::from_value::<StreamUpdateRequest>(json!({
            "type": "ping",
            "request_id": "req-3"
        }))
        .expect("ping request should parse");
        match ping {
            StreamUpdateRequest::Ping { request_id } => assert_eq!(request_id.as_deref(), Some("req-3")),
            other => panic!("expected ping request, got {other:?}"),
        }
    }

    #[test]
    fn stream_control_request_parses_set_control_and_ping() {
        let set_control = serde_json::from_value::<StreamControlRequest>(json!({
            "type": "set_control",
            "request_id": "req-4",
            "control_id": 12,
            "value": {
                "kind": "int",
                "value": 42
            }
        }))
        .expect("set_control request should parse");
        match set_control {
            StreamControlRequest::SetControl { request_id, control_id, value } => {
                assert_eq!(request_id.as_deref(), Some("req-4"));
                assert_eq!(control_id, 12);
                assert_eq!(value, CaptureControlValue::Int(42));
            }
            other => panic!("expected set_control request, got {other:?}"),
        }

        let ping = serde_json::from_value::<StreamControlRequest>(json!({
            "type": "ping"
        }))
        .expect("control ping should parse");
        match ping {
            StreamControlRequest::Ping { request_id } => assert_eq!(request_id, None),
            other => panic!("expected control ping, got {other:?}"),
        }
    }

    #[test]
    fn responses_serialize_with_expected_tags() {
        let update_ack = serde_json::to_value(StreamUpdateResponse::Ack { request_id: Some("req-5".to_string()) }).expect("update ack should serialize");
        assert_eq!(
            update_ack,
            json!({
                "type": "ack",
                "request_id": "req-5"
            })
        );

        let control_error = serde_json::to_value(StreamControlResponse::Error { request_id: None, error: "denied".to_string() }).expect("control error should serialize");
        assert_eq!(
            control_error,
            json!({
                "type": "error",
                "error": "denied"
            })
        );
    }
}
