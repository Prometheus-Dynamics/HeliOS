use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use futures::StreamExt;
use reqwest::header::{ACCEPT, RANGE};
use url::Url;

use crate::http::AppState;

use super::state::PeersServiceState;
use super::types::*;

pub(super) fn discovered_api_port(kind: &PeerIntegrationKind, seen_ports: &std::collections::BTreeSet<u16>) -> u16 {
    if seen_ports.contains(&5800) {
        5800
    } else if seen_ports.contains(&5801) {
        5801
    } else {
        default_api_port(kind)
    }
}

pub(super) async fn infer_discovered_integration_kind(peers: &PeersServiceState, host: &str, seen_ports: &std::collections::BTreeSet<u16>, timeout_ms: u64) -> Option<PeerIntegrationKind> {
    if seen_ports.contains(&5800) {
        let api_base = format!("http://{host}:5800");
        if probe_helios_api(peers, &api_base, timeout_ms).await.ok {
            return Some(PeerIntegrationKind::Helios);
        }
    }

    if seen_ports.contains(&5801) || seen_ports.contains(&5800) {
        let limelight_management = probe_http_any(peers, &format!("http://{host}:5801"), timeout_ms).await;
        if limelight_management.ok {
            return Some(PeerIntegrationKind::LimelightOs);
        }
        let limelight_stream = probe_mjpegish(peers, &format!("http://{host}:5800/stream.mjpeg"), timeout_ms).await;
        if limelight_stream.ok {
            return Some(PeerIntegrationKind::LimelightOs);
        }
    }

    None
}

pub(super) fn default_integration_metadata(kind: PeerIntegrationKind) -> PeerIntegrationMetadata {
    PeerIntegrationMetadata { kind, management_url: None, stream_url: None, stream_urls: Vec::new(), localization_outputs: Vec::new(), camera_pose: None, custom: None }
}

pub(super) fn normalize_device_host(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = if trimmed.contains("://") { trimmed.to_string() } else { format!("http://{trimmed}") };
    match Url::parse(&candidate) {
        Ok(url) => url.host_str().map(|host| host.to_string()),
        Err(_) => Some(trimmed.to_string()),
    }
}

pub(super) fn apply_integration_defaults(integration: &mut PeerIntegrationMetadata, host: &str) {
    let defaults = integration_defaults(&integration.kind, host);
    if integration.management_url.is_none() {
        integration.management_url = defaults.management_url;
    }
    if integration.stream_url.is_none() {
        integration.stream_url = defaults.stream_url;
    }
    if integration.stream_urls.is_empty() && !defaults.stream_urls.is_empty() {
        integration.stream_urls = defaults.stream_urls;
    }
}

fn integration_defaults(kind: &PeerIntegrationKind, host: &str) -> IntegrationDefaults {
    match kind {
        PeerIntegrationKind::Photonvision => IntegrationDefaults {
            management_url: Some(format!("http://{host}:5800")),
            stream_url: Some(format!("http://{host}:1181/stream.mjpg")),
            stream_urls: vec![format!("http://{host}:1181/stream.mjpg")],
        },
        PeerIntegrationKind::LimelightOs => IntegrationDefaults {
            management_url: Some(format!("http://{host}:5801")),
            stream_url: Some(format!("http://{host}:5800/stream.mjpeg")),
            stream_urls: vec![format!("http://{host}:5800/stream.mjpeg")],
        },
        PeerIntegrationKind::Helios | PeerIntegrationKind::Custom => IntegrationDefaults { management_url: None, stream_url: None, stream_urls: Vec::new() },
    }
}

struct IntegrationDefaults {
    management_url: Option<String>,
    stream_url: Option<String>,
    stream_urls: Vec<String>,
}

pub(super) fn default_api_port(kind: &PeerIntegrationKind) -> u16 {
    match kind {
        PeerIntegrationKind::LimelightOs | PeerIntegrationKind::Photonvision | PeerIntegrationKind::Custom | PeerIntegrationKind::Helios => 5800,
    }
}

pub(super) fn normalize_api_base(raw: Option<&str>, device_ip: Option<&str>, kind: &PeerIntegrationKind) -> Result<String, String> {
    let trimmed = raw.unwrap_or_default().trim();
    if trimmed.is_empty() {
        let host = device_ip.and_then(normalize_device_host).ok_or_else(|| "api_base_url or device_ip is required".to_string())?;
        return Ok(format!("http://{host}:{}", default_api_port(kind)));
    }

    let candidate = if trimmed.contains("://") { trimmed.to_string() } else { format!("http://{trimmed}") };
    let url = Url::parse(&candidate).map_err(|err| format!("invalid api_base_url: {err}"))?;
    match url.scheme() {
        "http" | "https" => {}
        _ => return Err("api_base_url must be http or https".into()),
    }
    let origin = url.origin().ascii_serialization();
    let path = url.path().trim_end_matches('/');
    if path.is_empty() || path == "/" { Ok(origin) } else { Ok(format!("{origin}{path}")) }
}

pub(super) fn derive_endpoints(api_base_url: &str) -> Vec<PeerEndpoint> {
    let candidate = if api_base_url.contains("://") { api_base_url.to_string() } else { format!("http://{api_base_url}") };
    if let Ok(url) = Url::parse(&candidate)
        && let Some(host) = url.host_str()
    {
        return vec![PeerEndpoint { host: host.to_string(), port: url.port() }];
    }
    Vec::new()
}

#[utoipa::path(
    post,
    path = "/peers/integrations/photonvision/streams",
    tag = "Peers",
    request_body = PhotonvisionDiscoverStreamsRequest,
    responses(
        (status = 200, description = "Discovered streams", body = PhotonvisionDiscoverStreamsResponse),
        (status = 400, description = "Invalid payload", body = PeerError)
    )
)]
pub(crate) async fn photonvision_discover_streams(State(state): State<AppState>, Json(req): Json<PhotonvisionDiscoverStreamsRequest>) -> impl IntoResponse {
    let host = match normalize_device_host(req.host.as_str()) {
        Some(value) => value,
        None => {
            return (StatusCode::BAD_REQUEST, Json(PeerError { error: "host is required".to_string() })).into_response();
        }
    };
    let base_port = req.base_port.unwrap_or(1181);
    let max_streams = req.max_streams.unwrap_or(8).clamp(1, 32);
    let timeout_ms = req.timeout_ms.unwrap_or(900).clamp(100, 10_000);

    let streams = discover_photonvision_streams(state.services.peers.inner(), &host, base_port, max_streams, timeout_ms).await;
    (StatusCode::OK, Json(PhotonvisionDiscoverStreamsResponse { host, streams })).into_response()
}

#[utoipa::path(
    post,
    path = "/peers/probe",
    tag = "Peers",
    request_body = PeerProbeRequest,
    responses(
        (status = 200, description = "Probe result", body = PeerProbeResponse),
        (status = 400, description = "Invalid payload", body = PeerError)
    )
)]
pub(crate) async fn probe_peer(State(state): State<AppState>, Json(req): Json<PeerProbeRequest>) -> impl IntoResponse {
    let peers = state.services.peers.inner();
    let timeout_ms = req.timeout_ms.unwrap_or(1200).clamp(100, 15_000);

    let mut api = None;
    let mut management = None;
    let mut stream = None;
    let mut streams = Vec::new();
    let mut photonvision = None;
    let mut nt4 = None;

    if let Some(api_base_url) = req.api_base_url.as_deref().filter(|value| !value.trim().is_empty()) {
        match normalize_api_base(Some(api_base_url), None, &req.kind) {
            Ok(base) => {
                api = Some(match req.kind {
                    PeerIntegrationKind::Helios => probe_helios_api(peers, &base, timeout_ms).await,
                    _ => probe_http_any(peers, &base, timeout_ms).await,
                });
            }
            Err(err) => {
                api = Some(ProbeResult { url: api_base_url.to_string(), ok: false, status: None, content_type: None, latency_ms: None, error: Some(err) });
            }
        }
    } else if let Some(device_ip) = req.device_ip.as_deref().and_then(normalize_device_host) {
        let base = format!("http://{device_ip}:{}", default_api_port(&req.kind));
        api = Some(match req.kind {
            PeerIntegrationKind::Helios => probe_helios_api(peers, &base, timeout_ms).await,
            _ => probe_http_any(peers, &base, timeout_ms).await,
        });
    }

    if let Some(management_url) = req.management_url.as_deref().filter(|value| !value.trim().is_empty()) {
        management = Some(probe_http_any(peers, management_url, timeout_ms).await);
    } else if let Some(device_ip) = req.device_ip.as_deref().and_then(normalize_device_host) {
        let defaults = integration_defaults(&req.kind, device_ip.as_str());
        if let Some(url) = defaults.management_url {
            management = Some(probe_http_any(peers, &url, timeout_ms).await);
        }
    }

    let mut candidate_stream_urls: Vec<String> = Vec::new();
    if let Some(value) = req.stream_url.as_deref().filter(|value| !value.trim().is_empty()) {
        candidate_stream_urls.push(value.trim().to_string());
    }
    candidate_stream_urls.extend(req.stream_urls.iter().filter_map(|url| {
        let trimmed = url.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
    }));

    if candidate_stream_urls.is_empty()
        && let Some(device_ip) = req.device_ip.as_deref().and_then(normalize_device_host)
    {
        let defaults = integration_defaults(&req.kind, device_ip.as_str());
        if let Some(url) = defaults.stream_url {
            candidate_stream_urls.push(url);
        }
        candidate_stream_urls.extend(defaults.stream_urls);
    }

    if matches!(req.kind, PeerIntegrationKind::Photonvision) {
        let host = req
            .device_ip
            .as_deref()
            .and_then(normalize_device_host)
            .or_else(|| req.management_url.as_deref().and_then(|value| Url::parse(value).ok()).and_then(|url| url.host_str().map(|host| host.to_string())))
            .or_else(|| req.api_base_url.as_deref().and_then(|value| Url::parse(value).ok()).and_then(|url| url.host_str().map(|host| host.to_string())));
        if let Some(host) = host {
            let discovered = discover_photonvision_streams(peers, &host, 1181, 8, timeout_ms.min(2500)).await;
            let discovered_urls = discovered.iter().map(|stream| stream.url.clone()).collect::<Vec<_>>();
            if !discovered_urls.is_empty() {
                candidate_stream_urls = merge_urls(candidate_stream_urls, discovered_urls);
            }
            photonvision = Some(PhotonvisionDiscoverStreamsResponse { host: host.clone(), streams: discovered });
        }
    }

    let nt4_host = req
        .network_table
        .as_deref()
        .and_then(normalize_device_host)
        .or_else(|| req.device_ip.as_deref().and_then(normalize_device_host))
        .or_else(|| req.api_base_url.as_deref().and_then(|value| Url::parse(value).ok()).and_then(|url| url.host_str().map(|host| host.to_string())));
    if let Some(nt4_host) = nt4_host {
        nt4 = Some(probe_nt4(state.services.runtime.nt4_pool(), &nt4_host, 5810, timeout_ms.min(2500)).await);
    }

    if let Some(first) = candidate_stream_urls.first() {
        stream = Some(probe_mjpegish(peers, first, timeout_ms).await);
    }
    for url in candidate_stream_urls {
        streams.push(probe_mjpegish(peers, &url, timeout_ms).await);
    }

    (StatusCode::OK, Json(PeerProbeResponse { kind: req.kind, api, management, stream, streams, photonvision, nt4 })).into_response()
}

async fn probe_nt4(pool: &crate::nt4::pool::Nt4ClientPool, host: &str, port: u16, timeout_ms: u64) -> Nt4PeerProbe {
    let settings = crate::http::device::nt4::load_settings().await;
    if !settings.subscriptions_enabled {
        return Nt4PeerProbe { host: host.to_string(), port, ok: false, roots: Vec::new(), error: Some("nt4 subscriptions are disabled in device settings".into()) };
    }

    let entry = match pool.get_or_connect(host, port, "HeliOS-peers-probe").await {
        Ok(entry) => entry,
        Err(err) => {
            return Nt4PeerProbe { host: host.to_string(), port, ok: false, roots: Vec::new(), error: Some(err) };
        }
    };
    if let Err(err) = entry.wait_ready(std::time::Duration::from_millis(timeout_ms)).await {
        let _ = pool.disconnect(host, port).await;
        return Nt4PeerProbe { host: host.to_string(), port, ok: false, roots: Vec::new(), error: Some(err) };
    }
    let topics = entry.list_topics_prefix("/", std::time::Duration::from_millis(250)).await.unwrap_or_default();
    let mut roots =
        topics.iter().filter_map(|topic| topic.trim_start_matches('/').split('/').next()).filter(|seg| !seg.is_empty() && *seg != ".schema").map(|seg| format!("/{seg}")).collect::<Vec<_>>();
    roots.sort();
    roots.dedup();
    Nt4PeerProbe { host: host.to_string(), port, ok: true, roots, error: None }
}

fn merge_urls(mut seed: Vec<String>, additions: Vec<String>) -> Vec<String> {
    for url in additions {
        if !seed.iter().any(|existing| existing == &url) {
            seed.push(url);
        }
    }
    seed
}

async fn discover_photonvision_streams(peers: &PeersServiceState, host: &str, base_port: u16, max_streams: u16, timeout_ms: u64) -> Vec<DiscoveredStream> {
    let mut discovered = Vec::new();
    let paths = ["/stream.mjpg", "/?action=stream"];
    for idx in 0..max_streams {
        let port = base_port.saturating_add(idx);
        for path in paths {
            let url = format!("http://{host}:{port}{path}");
            let result = probe_mjpegish(peers, &url, timeout_ms).await;
            if result.ok {
                discovered.push(DiscoveredStream { url: result.url, port, status: result.status.unwrap_or(200), content_type: result.content_type });
                break;
            }
        }
    }
    discovered
}

async fn probe_http_any(peers: &PeersServiceState, url: &str, timeout_ms: u64) -> ProbeResult {
    let url = url.trim();
    if url.is_empty() {
        return ProbeResult { url: url.to_string(), ok: false, status: None, content_type: None, latency_ms: None, error: Some("url is required".into()) };
    }
    let start = std::time::Instant::now();
    let request = peers.probe_client().get(url).timeout(std::time::Duration::from_millis(timeout_ms));
    match request.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let content_type = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).map(|value| value.to_string());
            ProbeResult { url: url.to_string(), ok: resp.status().is_success(), status: Some(status), content_type, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: None }
        }
        Err(err) => ProbeResult { url: url.to_string(), ok: false, status: None, content_type: None, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: Some(err.to_string()) },
    }
}

async fn probe_mjpegish(peers: &PeersServiceState, url: &str, timeout_ms: u64) -> ProbeResult {
    let url = url.trim();
    if url.is_empty() {
        return ProbeResult { url: url.to_string(), ok: false, status: None, content_type: None, latency_ms: None, error: Some("url is required".into()) };
    }
    let start = std::time::Instant::now();
    let request = peers.probe_client().get(url).timeout(std::time::Duration::from_millis(timeout_ms)).header(ACCEPT, "multipart/x-mixed-replace, image/jpeg, */*").header(RANGE, "bytes=0-1023");
    match request.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let content_type = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).map(|value| value.to_string());
            if !resp.status().is_success() {
                return ProbeResult { url: url.to_string(), ok: false, status: Some(status), content_type, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: None };
            }
            let looks_like_stream = content_type.as_deref().is_some_and(|value| {
                let value = value.to_ascii_lowercase();
                value.contains("multipart") || value.contains("mjpeg") || value.contains("image/jpeg")
            });
            if looks_like_stream {
                return ProbeResult { url: url.to_string(), ok: true, status: Some(status), content_type, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: None };
            }

            let mut ok = false;
            if let Some(Ok(bytes)) = resp.bytes_stream().next().await {
                ok = bytes.windows(2).any(|pair| pair == [0xff, 0xd8]);
            }
            ProbeResult { url: url.to_string(), ok, status: Some(status), content_type, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: None }
        }
        Err(err) => ProbeResult { url: url.to_string(), ok: false, status: None, content_type: None, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: Some(err.to_string()) },
    }
}

async fn probe_helios_api(peers: &PeersServiceState, base: &str, timeout_ms: u64) -> ProbeResult {
    let base = base.trim_end_matches('/');
    let candidate = if base.ends_with("/v1") { format!("{base}/device/hostname") } else { format!("{base}/v1/device/hostname") };
    let start = std::time::Instant::now();
    let request = peers.probe_client().get(&candidate).timeout(std::time::Duration::from_millis(timeout_ms)).header(ACCEPT, "application/json, */*;q=0.1");
    match request.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let content_type = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).map(|value| value.to_string());
            if !resp.status().is_success() {
                return ProbeResult { url: candidate, ok: false, status: Some(status), content_type, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: None };
            }

            let is_json = content_type.as_deref().is_some_and(|value| value.to_ascii_lowercase().contains("application/json"));
            if !is_json {
                return ProbeResult { url: candidate, ok: false, status: Some(status), content_type, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: None };
            }

            let body_ok = resp
                .bytes()
                .await
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|value| value.get("hostname").and_then(|hostname| hostname.as_str()).map(|hostname| !hostname.trim().is_empty()))
                .unwrap_or(false);
            ProbeResult { url: candidate, ok: body_ok, status: Some(status), content_type, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: None }
        }
        Err(err) => ProbeResult { url: candidate, ok: false, status: None, content_type: None, latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0), error: Some(err.to_string()) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_api_base_uses_device_host_default() {
        let actual = normalize_api_base(None, Some("10.0.0.5"), &PeerIntegrationKind::Helios).unwrap();
        assert_eq!(actual, "http://10.0.0.5:5800");
    }

    #[test]
    fn normalize_api_base_preserves_non_root_path() {
        let actual = normalize_api_base(Some("https://robot.local/api"), None, &PeerIntegrationKind::Helios).unwrap();
        assert_eq!(actual, "https://robot.local/api");
    }

    #[test]
    fn derive_endpoints_extracts_host_and_port() {
        let endpoints = derive_endpoints("http://10.0.0.5:5800");
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].host, "10.0.0.5");
        assert_eq!(endpoints[0].port, Some(5800));
    }

    #[test]
    fn merge_urls_keeps_first_occurrence() {
        let merged = merge_urls(vec!["http://a".into(), "http://b".into()], vec!["http://b".into(), "http://c".into()]);
        assert_eq!(merged, vec!["http://a", "http://b", "http://c"]);
    }
}
