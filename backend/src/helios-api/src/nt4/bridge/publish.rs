use nt_client::data::{DataType, JsonString};
use nt_client::topic::Properties;
use std::{collections::HashMap, net::Ipv4Addr};

use super::payloads::BridgePayloads;

#[derive(Debug, Default)]
pub(super) struct BridgePublishRuntime {
    pub(super) publishers: HashMap<String, nt_client::publish::GenericPublisher>,
    pub(super) last_target: Option<(String, u16, String)>,
    pub(super) last_entry_id: Option<u64>,
}

impl BridgePublishRuntime {
    pub(super) fn clear(&mut self) {
        self.publishers.clear();
        self.last_target = None;
        self.last_entry_id = None;
    }
}

pub(super) async fn publish_bridge_payloads(
    handle: &nt_client::ClientHandle,
    runtime: &mut BridgePublishRuntime,
    prefix: &str,
    hostname: &str,
    device_ipv4: Option<Ipv4Addr>,
    api_url: &str,
    ui_url: &str,
    payloads: &BridgePayloads,
) -> Result<(), String> {
    let hostname_topic = format!("{prefix}/info/hostname");
    set_string(handle, &mut runtime.publishers, &hostname_topic, hostname).await?;

    if let Some(ip) = device_ipv4 {
        let ip_topic = format!("{prefix}/info/ip");
        set_string(handle, &mut runtime.publishers, &ip_topic, &ip.to_string()).await?;
    }

    let api_url_topic = format!("{prefix}/info/api_url");
    set_string(handle, &mut runtime.publishers, &api_url_topic, api_url).await?;

    let ui_url_topic = format!("{prefix}/info/ui_url");
    set_string(handle, &mut runtime.publishers, &ui_url_topic, ui_url).await?;

    let streams_topic = format!("{prefix}/info/streams");
    set_json(handle, &mut runtime.publishers, &streams_topic, &payloads.streams_json).await?;

    for (topic, payload) in &payloads.stream_values {
        set_json(handle, &mut runtime.publishers, topic, payload).await?;
    }

    for publisher in &payloads.camera_publishers {
        let source_topic = format!("{}/source", publisher.topic);
        set_string(handle, &mut runtime.publishers, &source_topic, &publisher.source_name).await?;

        let streams_topic = format!("{}/streams", publisher.topic);
        set_string_array(handle, &mut runtime.publishers, &streams_topic, &publisher.stream_urls).await?;

        let value_topic = format!("{}/value", publisher.topic);
        set_json(handle, &mut runtime.publishers, &value_topic, &publisher.value).await?;

        let connected_topic = format!("{}/connected", publisher.topic);
        set_bool(handle, &mut runtime.publishers, &connected_topic, true).await?;
    }

    let device_value = serde_json::json!({
        "hostname": hostname,
        "ip": device_ipv4.map(|ip| ip.to_string()),
        "api_url": api_url,
        "ui_url": ui_url,
        "engine_ok": payloads.engine_ok,
        "streams": payloads.streams_json,
        "timestamp_ms": chrono::Utc::now().timestamp_millis().max(0) as u64,
    });
    let value_topic = format!("{prefix}/value");
    set_json(handle, &mut runtime.publishers, &value_topic, &device_value).await?;

    let telemetry = serde_json::json!({
        "engine_ok": payloads.engine_ok,
        "ipv4": device_ipv4.map(|ip| ip.to_string()),
        "stream_count": payloads.streams_json.as_array().map(|values| values.len()).unwrap_or(0),
        "timestamp_ms": chrono::Utc::now().timestamp_millis().max(0) as u64,
    });
    let telemetry_topic = format!("{prefix}/telemetry");
    set_json(handle, &mut runtime.publishers, &telemetry_topic, &telemetry).await?;

    Ok(())
}

async fn publisher<'a>(
    handle: &nt_client::ClientHandle,
    publishers: &'a mut HashMap<String, nt_client::publish::GenericPublisher>,
    topic: &str,
    data_type: DataType,
) -> Result<&'a nt_client::publish::GenericPublisher, String> {
    if !publishers.contains_key(topic) {
        let publisher = handle.topic(topic.to_string()).generic_publish(data_type, Properties::default()).await.map_err(|err| err.to_string())?;
        publishers.insert(topic.to_string(), publisher);
    }
    Ok(publishers.get(topic).expect("inserted"))
}

async fn set_string(handle: &nt_client::ClientHandle, publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>, topic: &str, value: &str) -> Result<(), String> {
    let publisher = publisher(handle, publishers, topic, DataType::String).await?;
    publisher.set(value.to_string()).await.map_err(|err| err.to_string())
}

async fn set_json(handle: &nt_client::ClientHandle, publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>, topic: &str, value: &serde_json::Value) -> Result<(), String> {
    let publisher = publisher(handle, publishers, topic, DataType::Json).await?;
    let payload = JsonString(serde_json::to_string(value).map_err(|err| err.to_string())?);
    publisher.set::<JsonString>(payload).await.map_err(|err| err.to_string())
}

async fn set_bool(handle: &nt_client::ClientHandle, publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>, topic: &str, value: bool) -> Result<(), String> {
    let publisher = publisher(handle, publishers, topic, DataType::Boolean).await?;
    publisher.set(value).await.map_err(|err| err.to_string())
}

async fn set_string_array(handle: &nt_client::ClientHandle, publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>, topic: &str, value: &[String]) -> Result<(), String> {
    let publisher = publisher(handle, publishers, topic, DataType::StringArray).await?;
    publisher.set(value.to_vec()).await.map_err(|err| err.to_string())
}
