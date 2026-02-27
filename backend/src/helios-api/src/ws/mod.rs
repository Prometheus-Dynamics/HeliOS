mod console;
mod device;
mod devices;
mod imu;
pub(crate) mod logs;
mod ota;
mod pipelines;
pub(crate) mod processes;
mod sensors;
mod streams;
mod updates;

use crate::http::AppState;
use axum::Router;
use lib_asyncapi::registry::SchemaRegistry;
use lib_asyncapi::{AsyncApiInfo, Server, Tag};
use std::collections::BTreeMap;

pub fn router(state: AppState) -> Router {
    Router::new()
        .nest("/streams", streams::router())
        .nest("/pipelines", pipelines::router())
        .route("/devices", axum::routing::get(devices::devices_updates_upgrade))
        .route("/telemetry", axum::routing::get(device::telemetry_upgrade))
        .route("/sensors", axum::routing::get(sensors::sensors_upgrade))
        .route("/imu", axum::routing::get(imu::imu_upgrade))
        .route("/console", axum::routing::get(console::console_upgrade))
        .route("/logs", axum::routing::get(logs::logs_upgrade))
        .route("/processes", axum::routing::get(processes::processes_upgrade))
        .route("/ota", axum::routing::get(ota::ota_upgrade))
        .route("/updates", axum::routing::get(updates::updates_upgrade))
        .with_state(state)
}

pub fn asyncapi_json(host: Option<String>) -> serde_json::Value {
    let mut registry = SchemaRegistry::default();
    let mut servers: BTreeMap<String, Server> = BTreeMap::new();
    let mut tags: Vec<Tag> = Vec::new();
    let mut docs = Vec::new();

    streams::register_docs(host.clone(), &mut registry, &mut servers, &mut tags, &mut docs);
    devices::register_docs(host.clone(), &mut registry, &mut servers, &mut tags, &mut docs);
    device::register_docs(host.clone(), &mut registry, &mut servers, &mut tags, &mut docs);
    console::register_docs(host.clone(), &mut registry, &mut servers, &mut tags, &mut docs);
    logs::register_docs(host.clone(), &mut registry, &mut servers, &mut tags, &mut docs);
    processes::register_docs(host.clone(), &mut registry, &mut servers, &mut tags, &mut docs);
    ota::register_docs(host.clone(), &mut registry, &mut servers, &mut tags, &mut docs);
    updates::register_docs(host, &mut registry, &mut servers, &mut tags, &mut docs);

    let info = AsyncApiInfo {
        title: "Helios WebSocket API".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        summary: Some("Realtime telemetry and control streams".into()),
        description: Some("WebSocket endpoints for device telemetry, stream metrics, pipeline metrics, logs, and console access.".into()),
        ..Default::default()
    };

    let doc = lib_asyncapi::generate_asyncapi(&docs, &[], &registry, info, servers, None, &tags, None);
    serde_json::to_value(doc).unwrap_or_default()
}
