use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};

use nt_client::data::DataType;
use nt_client::topic::Properties;

use crate::http::device::nt4 as device_nt4;
use crate::nt4::support::{client_name_from_hostname, team_number_to_rio_ip};

use super::super::limelight_types::LimelightPublishCache;
use super::runtime::LimelightAdapterRuntime;

#[derive(Debug, Default)]
pub(super) struct LimelightPublishRuntime {
    publishers: HashMap<String, nt_client::publish::GenericPublisher>,
    last_target: Option<(String, u16, String)>,
    last_entry_id: Option<u64>,
}

impl LimelightPublishRuntime {
    pub(super) fn clear(&mut self) {
        self.publishers.clear();
        self.last_target = None;
        self.last_entry_id = None;
    }
}

#[derive(Debug, Clone)]
pub(super) struct PublishOutcome {
    pub(super) phase: String,
    pub(super) ready: bool,
    pub(super) last_error: Option<String>,
}

pub(super) async fn publish_read_topics(settings: &device_nt4::Nt4Settings, runtime: &mut LimelightPublishRuntime, adapters: &mut BTreeMap<String, LimelightAdapterRuntime>) -> PublishOutcome {
    let host_override = settings.server_host.as_ref().map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
    let host = match host_override {
        Some(host) => Some(host),
        None => default_nt4_server_host_from_team_file().await,
    };
    let Some(host) = host else {
        runtime.clear();
        return PublishOutcome { phase: "waiting_nt_target".to_string(), ready: false, last_error: None };
    };
    let port = settings.server_port.unwrap_or(5810);
    let hostname = lib_net::get_hostname().ok().map(|value| value.to_string_lossy().trim().to_string()).unwrap_or_default();
    let client_name = client_name_from_hostname(&hostname);
    let target = (host.clone(), port, client_name.clone());

    if runtime.last_target.as_ref() != Some(&target) {
        let _ = crate::nt4::pool().disconnect(&host, port).await;
        runtime.last_target = Some(target);
        runtime.last_entry_id = None;
        runtime.publishers.clear();
    }

    let entry = match crate::nt4::pool().get_or_connect(&host, port, &client_name).await {
        Ok(entry) => entry,
        Err(err) => {
            runtime.publishers.clear();
            return PublishOutcome { phase: "nt_connect_failed".to_string(), ready: false, last_error: Some(err) };
        }
    };
    if entry.wait_ready(Duration::from_millis(1500)).await.is_err() {
        let _ = crate::nt4::pool().disconnect(&host, port).await;
        runtime.last_entry_id = None;
        runtime.publishers.clear();
        return PublishOutcome { phase: "nt_unreachable".to_string(), ready: false, last_error: None };
    }
    if runtime.last_entry_id != Some(entry.id()) {
        runtime.last_entry_id = Some(entry.id());
        runtime.publishers.clear();
    }

    let handle = entry.handle().clone();
    for adapter in adapters.values_mut() {
        if let Err(err) = publish_adapter_snapshot(&handle, &mut runtime.publishers, adapter).await {
            return PublishOutcome { phase: "publish_error".to_string(), ready: false, last_error: Some(err) };
        }
    }

    PublishOutcome { phase: "active_publish".to_string(), ready: true, last_error: None }
}

async fn publish_adapter_snapshot(
    handle: &nt_client::ClientHandle,
    publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>,
    adapter: &mut LimelightAdapterRuntime,
) -> Result<(), String> {
    publish_double_key(handle, publishers, &mut adapter.publish_cache, &adapter.id.table_name, "tv", adapter.read_snapshot.tv).await?;
    publish_double_key(handle, publishers, &mut adapter.publish_cache, &adapter.id.table_name, "tid", adapter.read_snapshot.tid).await?;
    publish_double_key(handle, publishers, &mut adapter.publish_cache, &adapter.id.table_name, "tl", adapter.read_snapshot.tl).await?;
    publish_double_key(handle, publishers, &mut adapter.publish_cache, &adapter.id.table_name, "cl", adapter.read_snapshot.cl).await?;
    publish_double_key(handle, publishers, &mut adapter.publish_cache, &adapter.id.table_name, "hb", adapter.read_snapshot.hb).await?;
    publish_double_array_key(handle, publishers, &mut adapter.publish_cache, &adapter.id.table_name, "t2d", &adapter.read_snapshot.t2d).await?;
    publish_double_array_key(handle, publishers, &mut adapter.publish_cache, &adapter.id.table_name, "hw", &adapter.read_snapshot.hw).await?;
    publish_double_array_key(handle, publishers, &mut adapter.publish_cache, &adapter.id.table_name, "imu", &adapter.read_snapshot.imu).await?;
    publish_string_key(handle, publishers, &mut adapter.publish_cache, &adapter.id.table_name, "json", &adapter.read_snapshot.json).await?;
    Ok(())
}

async fn publish_double_key(
    handle: &nt_client::ClientHandle,
    publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>,
    cache: &mut LimelightPublishCache,
    table: &str,
    key: &str,
    value: f64,
) -> Result<(), String> {
    let topic = format!("/{table}/{key}");
    let cached = serde_json::Value::from(value);
    if !cache_changed(cache, &topic, cached) {
        return Ok(());
    }
    set_double(handle, publishers, &topic, value).await
}

async fn publish_double_array_key(
    handle: &nt_client::ClientHandle,
    publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>,
    cache: &mut LimelightPublishCache,
    table: &str,
    key: &str,
    value: &[f64],
) -> Result<(), String> {
    let topic = format!("/{table}/{key}");
    let cached = serde_json::Value::Array(value.iter().copied().map(serde_json::Value::from).collect());
    if !cache_changed(cache, &topic, cached) {
        return Ok(());
    }
    set_double_array(handle, publishers, &topic, value).await
}

async fn publish_string_key(
    handle: &nt_client::ClientHandle,
    publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>,
    cache: &mut LimelightPublishCache,
    table: &str,
    key: &str,
    value: &str,
) -> Result<(), String> {
    let topic = format!("/{table}/{key}");
    let cached = serde_json::Value::String(value.to_string());
    if !cache_changed(cache, &topic, cached) {
        return Ok(());
    }
    set_string(handle, publishers, &topic, value).await
}

fn cache_changed(cache: &mut LimelightPublishCache, topic: &str, value: serde_json::Value) -> bool {
    if cache.by_topic.get(topic) == Some(&value) {
        return false;
    }
    cache.by_topic.insert(topic.to_string(), value);
    true
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
    Ok(publishers.get(topic).expect("publisher exists"))
}

async fn set_double(handle: &nt_client::ClientHandle, publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>, topic: &str, value: f64) -> Result<(), String> {
    let publisher = publisher(handle, publishers, topic, DataType::Double).await?;
    publisher.set(value).await.map_err(|err| err.to_string())
}

async fn set_double_array(handle: &nt_client::ClientHandle, publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>, topic: &str, value: &[f64]) -> Result<(), String> {
    let publisher = publisher(handle, publishers, topic, DataType::DoubleArray).await?;
    publisher.set(value.to_vec()).await.map_err(|err| err.to_string())
}

async fn set_string(handle: &nt_client::ClientHandle, publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>, topic: &str, value: &str) -> Result<(), String> {
    let publisher = publisher(handle, publishers, topic, DataType::String).await?;
    publisher.set(value.to_string()).await.map_err(|err| err.to_string())
}

async fn default_nt4_server_host_from_team_file() -> Option<String> {
    // TEMP_SHIM: nt4-limelight-team-file-etc-fallback
    // Keep the /etc team fallback until every deployed image writes the canonical team file into /var/lib/helios.
    let primary = std::env::var_os("HELIOS_TEAM_FILE").map(std::path::PathBuf::from).unwrap_or_else(|| "/var/lib/helios/team".into());
    let fallback = (primary.as_path() == std::path::Path::new("/var/lib/helios/team")).then_some(std::path::Path::new("/etc/helios/team"));
    let content = match tokio::fs::read_to_string(&primary).await {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => tokio::fs::read_to_string(fallback?).await.ok()?,
        Err(_) => return None,
    };
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let team: u32 = trimmed.parse().ok()?;
    team_number_to_rio_ip(team).map(|ip| ip.to_string())
}
