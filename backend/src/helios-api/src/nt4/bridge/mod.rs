mod network;
mod observability;
mod payloads;
mod publish;
mod topics;

#[cfg(test)]
mod tests;

use std::{sync::Arc, time::Duration};

use tracing::warn;

use crate::config::ApiConfig;
use crate::http::device::nt4 as device_nt4;
use crate::ipc::IpcHandles;
use crate::nt4::support::client_name_from_hostname;

use self::network::{best_local_ipv4, default_nt4_server_host_from_team_file};
use self::observability::{record_bridge_publish_failure, record_bridge_publish_success, record_bridge_reconnect, record_bridge_skip};
use self::payloads::build_bridge_payloads;
use self::publish::{BridgePublishRuntime, publish_bridge_payloads};
use self::topics::publish_prefix_from_hostname;
pub use observability::snapshot;

pub fn init(handles: Arc<IpcHandles>) {
    crate::nt4::limelight::init(handles.clone());

    let api_port = ApiConfig::from_env().bind_addr.port();
    tokio::spawn(async move {
        let mut publish_runtime = BridgePublishRuntime::default();
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        loop {
            tick.tick().await;

            let settings = device_nt4::load_settings().await;
            if !settings.enabled {
                publish_runtime.clear();
                record_bridge_skip();
                continue;
            }

            let host_override = settings.server_host.as_ref().map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
            let host = match host_override {
                Some(host) => Some(host),
                None => default_nt4_server_host_from_team_file().await,
            };
            let Some(host) = host else {
                publish_runtime.clear();
                record_bridge_skip();
                continue;
            };
            let port = settings.server_port.unwrap_or(5810);

            let hostname = lib_net::get_hostname().ok().map(|value| value.to_string_lossy().trim().to_string()).unwrap_or_default();
            let client_name = client_name_from_hostname(&hostname);
            let target = (host.clone(), port, client_name.clone());
            if publish_runtime.last_target.as_ref() != Some(&target) {
                if let Some((prev_host, prev_port, _)) = publish_runtime.last_target.as_ref() {
                    let _ = crate::nt4::pool().disconnect(prev_host, *prev_port).await;
                }
                let _ = crate::nt4::pool().disconnect(&host, port).await;
                publish_runtime.last_target = Some(target);
                publish_runtime.last_entry_id = None;
                publish_runtime.publishers.clear();
                record_bridge_reconnect(Some(format!("{host}:{port}")));
            }

            let entry = match crate::nt4::pool().get_or_connect(&host, port, &client_name).await {
                Ok(entry) => entry,
                Err(err) => {
                    record_bridge_publish_failure(format!("{host}:{port}"), None);
                    warn!(%err, "nt4 publish connect failed");
                    continue;
                }
            };
            if entry.wait_ready(Duration::from_millis(1500)).await.is_err() {
                let _ = crate::nt4::pool().disconnect(&host, port).await;
                publish_runtime.last_entry_id = None;
                publish_runtime.publishers.clear();
                record_bridge_publish_failure(format!("{host}:{port}"), Some(entry.id()));
                continue;
            }
            if publish_runtime.last_entry_id != Some(entry.id()) {
                publish_runtime.last_entry_id = Some(entry.id());
                publish_runtime.publishers.clear();
            }

            let prefix = publish_prefix_from_hostname(&hostname);
            let device_ipv4 = best_local_ipv4().await;
            let default_api_url = device_ipv4.map(|ip| format!("http://{ip}:{api_port}")).unwrap_or_default();
            let api_url = settings.public_api_url.clone().filter(|value| !value.trim().is_empty()).unwrap_or(default_api_url);
            let ui_url = device_ipv4.map(|ip| format!("http://{ip}:{api_port}")).unwrap_or_default();
            let payloads = build_bridge_payloads(&handles, &api_url, &prefix).await;

            let handle = entry.handle().clone();
            if let Err(err) = publish_bridge_payloads(&handle, &mut publish_runtime, &prefix, &hostname, device_ipv4, &api_url, &ui_url, &payloads).await {
                record_bridge_publish_failure(format!("{host}:{port}"), Some(entry.id()));
                warn!(
                    target_host = %host,
                    target_port = port,
                    %err,
                    "nt4 publish failed; forcing reconnect"
                );
                let _ = crate::nt4::pool().disconnect(&host, port).await;
                publish_runtime.last_entry_id = None;
                publish_runtime.publishers.clear();
                record_bridge_reconnect(Some(format!("{host}:{port}")));
            } else {
                record_bridge_publish_success(format!("{host}:{port}"), entry.id());
            }
        }
    });
}
