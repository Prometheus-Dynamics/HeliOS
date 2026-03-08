use nt_client::data::Properties;
use nt_client::data::r#type::{DataType, JsonString};
use std::net::Ipv4Addr;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tracing::warn;

use crate::config::ApiConfig;
use crate::http::device::nt4 as device_nt4;
use crate::ipc::IpcHandles;

pub fn init(handles: Arc<IpcHandles>) {
    crate::nt4::limelight::init(handles.clone());

    let api_port = ApiConfig::from_env().bind_addr.port();
    tokio::spawn(async move {
        let mut publishers: HashMap<String, nt_client::publish::GenericPublisher> = HashMap::new();
        let mut last_target: Option<(String, u16, String)> = None;
        let mut last_entry_id: Option<u64> = None;

        let mut tick = tokio::time::interval(Duration::from_secs(5));
        loop {
            tick.tick().await;

            let settings = device_nt4::load_settings().await;
            if !settings.enabled {
                last_target = None;
                publishers.clear();
                continue;
            }

            let host_override = settings.server_host.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            let host = match host_override {
                Some(host) => Some(host),
                None => default_nt4_server_host_from_team_file().await,
            };
            let Some(host) = host else {
                continue;
            };
            let port = settings.server_port.unwrap_or(5810);

            let hostname = lib_net::get_hostname().ok().map(|s| s.to_string_lossy().trim().to_string()).unwrap_or_default();
            let client_name = client_name_from_hostname(&hostname);
            let target = (host.clone(), port, client_name.clone());
            if last_target.as_ref() != Some(&target) {
                // Reconnect when target host/port/client-name changes (for hostname updates and retargeting).
                if let Some((prev_host, prev_port, _)) = last_target.as_ref() {
                    let _ = crate::nt4::pool().disconnect(prev_host, *prev_port).await;
                }
                // Also clear any stale slot at the new endpoint before reconnecting.
                let _ = crate::nt4::pool().disconnect(&host, port).await;
                last_target = Some(target.clone());
                last_entry_id = None;
                publishers.clear();
            }

            let entry = match crate::nt4::pool().get_or_connect(&host, port, &client_name).await {
                Ok(entry) => entry,
                Err(err) => {
                    warn!(%err, "nt4 publish connect failed");
                    continue;
                }
            };
            if entry.wait_ready(Duration::from_millis(1500)).await.is_err() {
                // Force a fresh connection attempt on the next tick.
                let _ = crate::nt4::pool().disconnect(&host, port).await;
                last_entry_id = None;
                publishers.clear();
                continue;
            }
            if last_entry_id != Some(entry.id()) {
                last_entry_id = Some(entry.id());
                publishers.clear();
            }

            let prefix = publish_prefix_from_hostname(&hostname);

            let device_ipv4 = best_local_ipv4().await;
            let default_api_url = device_ipv4.map(|ip| format!("http://{ip}:{api_port}")).unwrap_or_default();
            let api_url = settings.public_api_url.clone().filter(|s| !s.trim().is_empty()).unwrap_or(default_api_url);
            let ui_url = device_ipv4.map(|ip| format!("http://{ip}:{api_port}")).unwrap_or_default();

            let mut stream_values: Vec<(String, serde_json::Value)> = Vec::new();
            let mut camera_publishers: Vec<(String, String, Vec<String>, serde_json::Value)> = Vec::new();
            let (engine_ok, streams_json) = match handles.engine.list_streams().await {
                Ok(streams) => {
                    let list = streams
                        .into_iter()
                        .filter(|stream| !stream.manifest.internal)
                        .map(|stream| {
                            let stream_id = stream.stream_id.to_string();
                            let alias = stream.manifest.identity.alias.clone();
                            let stream_name = alias.clone().unwrap_or_else(|| stream_id.clone());
                            let stream_key = topic_segment(&stream_id, "stream");
                            let stream_preview_url = preview_url(&api_url, &stream_id);
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
                            camera_publishers.push((
                                camera_publisher_topic,
                                stream_name.clone(),
                                vec![stream_preview_url.clone()],
                                serde_json::json!({
                                    "streams": [stream_preview_url],
                                    "stream_id": payload["id"],
                                    "alias": payload["alias"],
                                    "source": "HeliOS",
                                }),
                            ));
                            payload
                        })
                        .collect::<Vec<_>>();
                    (true, serde_json::Value::Array(list))
                }
                Err(err) => (false, serde_json::json!({ "error": err.to_string() })),
            };

            let handle = entry.handle().clone();
            let mut publish_error: Option<String> = None;

            let hostname_topic = format!("{prefix}/info/hostname");
            if let Err(err) = set_string(&handle, &mut publishers, &hostname_topic, &hostname).await {
                publish_error = Some(format!("{hostname_topic}: {err}"));
            }
            if let Some(ip) = device_ipv4 {
                let ip_topic = format!("{prefix}/info/ip");
                if publish_error.is_none()
                    && let Err(err) = set_string(&handle, &mut publishers, &ip_topic, &ip.to_string()).await
                {
                    publish_error = Some(format!("{ip_topic}: {err}"));
                }
            }
            let api_url_topic = format!("{prefix}/info/api_url");
            if publish_error.is_none()
                && let Err(err) = set_string(&handle, &mut publishers, &api_url_topic, &api_url).await
            {
                publish_error = Some(format!("{api_url_topic}: {err}"));
            }
            let ui_url_topic = format!("{prefix}/info/ui_url");
            if publish_error.is_none()
                && let Err(err) = set_string(&handle, &mut publishers, &ui_url_topic, &ui_url).await
            {
                publish_error = Some(format!("{ui_url_topic}: {err}"));
            }
            let streams_topic = format!("{prefix}/info/streams");
            if publish_error.is_none()
                && let Err(err) = set_json(&handle, &mut publishers, &streams_topic, &streams_json).await
            {
                publish_error = Some(format!("{streams_topic}: {err}"));
            }
            for (topic, payload) in &stream_values {
                if publish_error.is_none()
                    && let Err(err) = set_json(&handle, &mut publishers, topic, payload).await
                {
                    publish_error = Some(format!("{topic}: {err}"));
                }
            }
            for (camera_topic, source_name, stream_urls, payload) in &camera_publishers {
                let source_topic = format!("{camera_topic}/source");
                if publish_error.is_none()
                    && let Err(err) = set_string(&handle, &mut publishers, &source_topic, source_name).await
                {
                    publish_error = Some(format!("{source_topic}: {err}"));
                }
                let streams_topic = format!("{camera_topic}/streams");
                if publish_error.is_none()
                    && let Err(err) = set_string_array(&handle, &mut publishers, &streams_topic, stream_urls).await
                {
                    publish_error = Some(format!("{streams_topic}: {err}"));
                }
                let value_topic = format!("{camera_topic}/value");
                if publish_error.is_none()
                    && let Err(err) = set_json(&handle, &mut publishers, &value_topic, payload).await
                {
                    publish_error = Some(format!("{value_topic}: {err}"));
                }
                let connected_topic = format!("{camera_topic}/connected");
                if publish_error.is_none()
                    && let Err(err) = set_bool(&handle, &mut publishers, &connected_topic, true).await
                {
                    publish_error = Some(format!("{connected_topic}: {err}"));
                }
            }

            let device_value = serde_json::json!({
                "hostname": hostname,
                "ip": device_ipv4.map(|ip| ip.to_string()),
                "api_url": api_url,
                "ui_url": ui_url,
                "engine_ok": engine_ok,
                "streams": streams_json,
                "timestamp_ms": chrono::Utc::now().timestamp_millis().max(0) as u64,
            });
            let value_topic = format!("{prefix}/value");
            if publish_error.is_none()
                && let Err(err) = set_json(&handle, &mut publishers, &value_topic, &device_value).await
            {
                publish_error = Some(format!("{value_topic}: {err}"));
            }

            // Small "summary" telemetry blob for NT4 consumers that want a single topic.
            let telemetry = serde_json::json!({
                "engine_ok": engine_ok,
                "ipv4": device_ipv4.map(|ip| ip.to_string()),
                "stream_count": streams_json.as_array().map(|v| v.len()).unwrap_or(0),
                "timestamp_ms": chrono::Utc::now().timestamp_millis().max(0) as u64,
            });
            let telemetry_topic = format!("{prefix}/telemetry");
            if publish_error.is_none()
                && let Err(err) = set_json(&handle, &mut publishers, &telemetry_topic, &telemetry).await
            {
                publish_error = Some(format!("{telemetry_topic}: {err}"));
            }

            if let Some(err) = publish_error {
                warn!(target_host = %host, target_port = port, %err, "nt4 publish failed; forcing reconnect");
                let _ = crate::nt4::pool().disconnect(&host, port).await;
                last_entry_id = None;
                publishers.clear();
                continue;
            }
        }
    });
}

fn client_name_from_hostname(hostname: &str) -> String {
    let trimmed = hostname.trim();
    if trimmed.is_empty() {
        return "HeliOS".to_string();
    }
    trimmed.to_string()
}

fn publish_prefix_from_hostname(hostname: &str) -> String {
    let trimmed = hostname.trim();
    if trimmed.is_empty() {
        return "/helios".to_string();
    }
    if trimmed.starts_with('/') { trimmed.to_string() } else { format!("/{trimmed}") }
}

async fn best_local_ipv4() -> Option<Ipv4Addr> {
    let addrs = lib_net::interface::get_local_addresses().await.ok()?;
    let mut v4s = addrs
        .into_iter()
        .filter_map(|ip| match ip {
            std::net::IpAddr::V4(v4) => Some(v4),
            std::net::IpAddr::V6(_) => None,
        })
        .filter(|ip| !ip.is_loopback() && !ip.is_link_local() && *ip != Ipv4Addr::UNSPECIFIED)
        .collect::<Vec<_>>();
    v4s.sort_by_key(|ip| (!ip.is_private(), *ip));
    v4s.into_iter().next()
}

async fn default_nt4_server_host_from_team_file() -> Option<String> {
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

fn team_number_to_rio_ip(team: u32) -> Option<Ipv4Addr> {
    if team == 0 || team > 25_599 {
        return None;
    }
    let a = (team / 100) as u8;
    let b = (team % 100) as u8;
    Some(Ipv4Addr::new(10, a, b, 2))
}

fn topic_segment(value: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.trim().chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() { fallback.to_string() } else { out }
}

fn preview_url(api_url: &str, stream_id: &str) -> String {
    if api_url.trim().is_empty() { format!("/v1/streams/{stream_id}/preview") } else { format!("{api_url}/v1/streams/{stream_id}/preview") }
}

async fn publisher<'a>(
    handle: &nt_client::ClientHandle,
    publishers: &'a mut HashMap<String, nt_client::publish::GenericPublisher>,
    topic: &str,
    data_type: DataType,
) -> Result<&'a nt_client::publish::GenericPublisher, String> {
    if !publishers.contains_key(topic) {
        let pubr = handle.topic(topic.to_string()).generic_publish(data_type, Properties::default()).await.map_err(|e| e.to_string())?;
        publishers.insert(topic.to_string(), pubr);
    }
    Ok(publishers.get(topic).expect("inserted"))
}

async fn set_string(handle: &nt_client::ClientHandle, publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>, topic: &str, value: &str) -> Result<(), String> {
    let pubr = publisher(handle, publishers, topic, DataType::String).await?;
    pubr.set(value.to_string()).await.map_err(|e| e.to_string())?;
    Ok(())
}

async fn set_json(handle: &nt_client::ClientHandle, publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>, topic: &str, value: &serde_json::Value) -> Result<(), String> {
    let pubr = publisher(handle, publishers, topic, DataType::Json).await?;
    let payload = JsonString(serde_json::to_string(value).map_err(|e| e.to_string())?);
    pubr.set(payload).await.map_err(|e| e.to_string())?;
    Ok(())
}

async fn set_bool(handle: &nt_client::ClientHandle, publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>, topic: &str, value: bool) -> Result<(), String> {
    let pubr = publisher(handle, publishers, topic, DataType::Boolean).await?;
    pubr.set(value).await.map_err(|e| e.to_string())?;
    Ok(())
}

async fn set_string_array(handle: &nt_client::ClientHandle, publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>, topic: &str, value: &[String]) -> Result<(), String> {
    let pubr = publisher(handle, publishers, topic, DataType::StringArray).await?;
    pubr.set(value.to_vec()).await.map_err(|e| e.to_string())?;
    Ok(())
}
