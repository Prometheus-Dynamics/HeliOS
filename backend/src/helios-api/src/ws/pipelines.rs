use crate::http::AppState;
use crate::pipeline_command_service;
use axum::{
    Router,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tracing::debug;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/{id}/updates", get(pipeline_updates))
}

async fn pipeline_updates(ws: WebSocketUpgrade, State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_pipeline_updates(socket, state, id))
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PipelineUpdateRequest {
    SetNodeConst {
        #[serde(default)]
        request_id: Option<String>,
        node_id: String,
        port: String,
        value: Option<JsonValue>,
    },
    SetInputValue {
        #[serde(default)]
        request_id: Option<String>,
        port: String,
        value: Option<JsonValue>,
    },
    SetGraph {
        #[serde(default)]
        request_id: Option<String>,
        graph: JsonValue,
        #[serde(default)]
        name: Option<String>,
    },
    Ping {
        #[serde(default)]
        request_id: Option<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PipelineUpdateResponse {
    Ack {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        updated_at_ms: Option<i64>,
    },
    Error {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        error: String,
    },
}

async fn handle_pipeline_updates(mut socket: WebSocket, state: AppState, pipeline_id: Uuid) {
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
                debug!(error = %err, "pipeline updates websocket closed");
                break;
            }
        };

        let parsed = serde_json::from_str::<PipelineUpdateRequest>(&payload);
        let response = match parsed {
            Ok(PipelineUpdateRequest::Ping { request_id }) => PipelineUpdateResponse::Ack { request_id, updated_at_ms: None },
            Ok(PipelineUpdateRequest::SetNodeConst { request_id, node_id, port, value }) => {
                match pipeline_command_service::update_pipeline_node_const(&state, pipeline_id, &node_id, &port, value).await {
                    Ok(updated_at_ms) => {
                        state.publish_realtime_update(
                            crate::ipc::RealtimeUpdateOrigin::Ws,
                            crate::ipc::RealtimeUpdateKind::PipelinesGraphs,
                            format!("/v1/ws/pipelines/{pipeline_id}/updates"),
                            Some("set_node_const".to_string()),
                            request_id.clone(),
                        );
                        PipelineUpdateResponse::Ack { request_id, updated_at_ms: Some(updated_at_ms) }
                    }
                    Err(err) => PipelineUpdateResponse::Error { request_id, error: err },
                }
            }
            Ok(PipelineUpdateRequest::SetInputValue { request_id, port, value }) => match pipeline_command_service::update_pipeline_input_value(&state, pipeline_id, &port, value).await {
                Ok(updated_at_ms) => {
                    state.publish_realtime_update(
                        crate::ipc::RealtimeUpdateOrigin::Ws,
                        crate::ipc::RealtimeUpdateKind::PipelinesGraphs,
                        format!("/v1/ws/pipelines/{pipeline_id}/updates"),
                        Some("set_input_value".to_string()),
                        request_id.clone(),
                    );
                    PipelineUpdateResponse::Ack { request_id, updated_at_ms: Some(updated_at_ms) }
                }
                Err(err) => PipelineUpdateResponse::Error { request_id, error: err },
            },
            Ok(PipelineUpdateRequest::SetGraph { request_id, graph, name }) => match pipeline_command_service::update_pipeline_graph(&state, pipeline_id, graph, name).await {
                Ok(updated_at_ms) => {
                    state.publish_realtime_update(
                        crate::ipc::RealtimeUpdateOrigin::Ws,
                        crate::ipc::RealtimeUpdateKind::PipelinesGraphs,
                        format!("/v1/ws/pipelines/{pipeline_id}/updates"),
                        Some("set_graph".to_string()),
                        request_id.clone(),
                    );
                    PipelineUpdateResponse::Ack { request_id, updated_at_ms: Some(updated_at_ms) }
                }
                Err(err) => PipelineUpdateResponse::Error { request_id, error: err },
            },
            Err(err) => PipelineUpdateResponse::Error { request_id: None, error: format!("invalid request: {err}") },
        };

        if let Ok(text) = serde_json::to_string(&response)
            && socket.send(Message::Text(text.into())).await.is_err()
        {
            break;
        }
    }
}
