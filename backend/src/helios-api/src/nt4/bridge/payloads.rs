use std::sync::Arc;

use serde_json::Value;

use crate::ipc::IpcHandles;

use super::topics::{preview_url, topic_segment};

pub(super) struct BridgeCameraPublisher {
    pub(super) topic: String,
    pub(super) source_name: String,
    pub(super) stream_urls: Vec<String>,
    pub(super) value: Value,
}

pub(super) struct BridgePayloads {
    pub(super) engine_ok: bool,
    pub(super) streams_json: Value,
    pub(super) stream_values: Vec<(String, Value)>,
    pub(super) camera_publishers: Vec<BridgeCameraPublisher>,
}

pub(super) async fn build_bridge_payloads(handles: &Arc<IpcHandles>, api_url: &str, prefix: &str) -> BridgePayloads {
    match handles.engine.list_streams().await {
        Ok(streams) => {
            let mut stream_values = Vec::new();
            let mut camera_publishers = Vec::new();
            let list = streams
                .into_iter()
                .filter(|stream| !stream.manifest.internal)
                .map(|stream| {
                    let stream_id = stream.stream_id.to_string();
                    let alias = stream.manifest.identity.alias.clone();
                    let stream_name = alias.clone().unwrap_or_else(|| stream_id.clone());
                    let stream_key = topic_segment(&stream_id, "stream");
                    let stream_preview_url = preview_url(api_url, &stream_id);
                    let camera_name = topic_segment(&format!("{stream_name}-{stream_id}"), "camera");
                    let stream_value_topic = format!("{prefix}/streams/{stream_key}/value");
                    let camera_publisher_topic = format!("/CameraPublisher/{camera_name}");
                    let url = match &stream.manifest.capture.handle {
                        styx::BackendHandle::Netcam { url, .. } => Some(url.clone()),
                        _ => None,
                    };
                    let payload = serde_json::json!({
                        "id": stream_id,
                        "alias": alias,
                        "url": url,
                        "preview_url": stream_preview_url,
                        "status": stream.status,
                        "nt": {
                            "value_topic": stream_value_topic,
                            "camera_publisher_topic": camera_publisher_topic,
                        }
                    });
                    stream_values.push((stream_value_topic, payload.clone()));
                    camera_publishers.push(BridgeCameraPublisher {
                        topic: camera_publisher_topic,
                        source_name: stream_name.clone(),
                        stream_urls: vec![stream_preview_url.clone()],
                        value: serde_json::json!({
                            "streams": [stream_preview_url],
                            "stream_id": payload["id"],
                            "alias": payload["alias"],
                            "source": "HeliOS",
                        }),
                    });
                    payload
                })
                .collect::<Vec<_>>();

            BridgePayloads { engine_ok: true, streams_json: Value::Array(list), stream_values, camera_publishers }
        }
        Err(err) => BridgePayloads { engine_ok: false, streams_json: serde_json::json!({ "error": err.to_string() }), stream_values: Vec::new(), camera_publishers: Vec::new() },
    }
}
