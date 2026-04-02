mod hub;
mod session;
mod wire;

#[cfg(test)]
mod tests;

pub(crate) use hub::{SensorEventsState, SharedSensorEvent, SharedSensorLatest};

use crate::http::AppState;
use axum::{
    extract::{State, ws::WebSocketUpgrade},
    response::IntoResponse,
};
use lib_asyncapi::registry::SchemaRegistry;
use lib_asyncapi::{Server, Tag, TypeSchema, WsDoc};
use session::sensors_loop;
use std::collections::BTreeMap;
use tracing::{warn, warn_span};

pub async fn sensors_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    state.services.hardware.bind_sensor_events_state(&state);
    ws.on_upgrade(move |socket| async move {
        let span = warn_span!("sensors_ws");
        let _guard = span.enter();
        if let Err(err) = sensors_loop(socket, state).await {
            warn!(%err, "sensors websocket terminated");
        }
    })
}

pub fn register_docs(host: Option<String>, _registry: &mut SchemaRegistry, servers: &mut BTreeMap<String, Server>, tags: &mut Vec<Tag>, docs: &mut Vec<WsDoc>) {
    let doc = WsDoc {
        path: "sensors.stream",
        summary: "Peripheral sensor stream",
        description: "Streams subscribed IMU, power, lighting, and firmware sensor events from the peripherals runtime.",
        tags: vec!["device".into(), "sensors".into()],
        payload: None,
        responses: vec![TypeSchema { name: "SensorsStreamEvent", schema: serde_json::json!({ "type": "object" }) }],
        params: vec![],
    };

    let server_host = host.unwrap_or_else(|| "localhost:5800/v1/ws".to_string());
    servers.entry("primary".into()).or_insert(Server { host: server_host, protocol: "ws".into(), protocol_version: None, description: Some("Primary WebSocket entrypoint".into()) });
    docs.push(doc);
    tags.push(Tag { name: "sensors".into(), description: Some("Peripheral sensor events".to_string()), external_docs: None });
}
