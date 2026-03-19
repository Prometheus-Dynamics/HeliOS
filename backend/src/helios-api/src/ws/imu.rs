use crate::http::AppState;
use crate::http::device::handle_imu_ws;
use axum::extract::State;
use axum::extract::ws::WebSocketUpgrade;
use axum::response::IntoResponse;
use lib_asyncapi::registry::SchemaRegistry;
use lib_asyncapi::{Server, Tag, TypeSchema, WsDoc};
use std::collections::BTreeMap;
use tracing::warn;

pub async fn imu_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        if let Err(err) = handle_imu_ws(socket, state).await {
            warn!(error = %err, "imu websocket terminated early");
        }
    })
}

pub fn register_docs(host: Option<String>, _registry: &mut SchemaRegistry, servers: &mut BTreeMap<String, Server>, tags: &mut Vec<Tag>, docs: &mut Vec<WsDoc>) {
    let doc = WsDoc {
        path: "imu.stream",
        summary: "IMU status stream",
        description: "Streams realtime IMU status payloads from the device IMU runtime.",
        tags: vec!["device".into(), "imu".into()],
        payload: None,
        responses: vec![TypeSchema { name: "ImuStatusPayload", schema: serde_json::json!({ "type": "object" }) }],
        params: vec![],
    };

    let server_host = host.unwrap_or_else(|| "localhost:5800/v1/ws".to_string());
    servers.entry("primary".into()).or_insert(Server { host: server_host, protocol: "ws".into(), protocol_version: None, description: Some("Primary WebSocket entrypoint".into()) });
    docs.push(doc);
    tags.push(Tag { name: "imu".into(), description: Some("Realtime IMU updates".to_string()), external_docs: None });
}
