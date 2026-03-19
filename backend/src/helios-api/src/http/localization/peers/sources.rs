use once_cell::sync::Lazy;
use reqwest::header::ACCEPT;
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use url::Url;

use crate::http::localization::peers::custom as localization_peer_custom;
use crate::http::peers::{PeerInfo, PeerIntegrationKind, PeerIntegrationMapping};
use helios_engine::localization::types::{LocalizationPipelineSource, PipelineOutputSample};

static PEER_HTTP: Lazy<reqwest::Client> = Lazy::new(|| crate::http::reqwest_client::build_http_client("HeliOS/localization-peers").expect("reqwest client"));
static PHOTONVISION_CACHE: Lazy<Mutex<HashMap<String, PhotonvisionCacheEntry>>> = Lazy::new(|| Mutex::new(HashMap::new()));
const PHOTONVISION_CACHE_TTL: Duration = Duration::from_secs(2);

#[derive(Clone)]
struct PhotonvisionCacheEntry {
    updated_at: Instant,
    snapshot: Option<crate::nt4::photonvision::PhotonvisionNt4Snapshot>,
}

async fn photonvision_snapshot_cached(host: &str) -> Option<crate::nt4::photonvision::PhotonvisionNt4Snapshot> {
    let now = Instant::now();
    {
        let cache = PHOTONVISION_CACHE.lock().await;
        if let Some(entry) = cache.get(host)
            && now.duration_since(entry.updated_at) <= PHOTONVISION_CACHE_TTL
        {
            return entry.snapshot.clone();
        }
    }

    let snapshot = crate::nt4::photonvision::snapshot(host, 5810, 600).await.ok();
    let mut cache = PHOTONVISION_CACHE.lock().await;
    cache.insert(host.to_string(), PhotonvisionCacheEntry { updated_at: now, snapshot: snapshot.clone() });
    snapshot
}

pub(crate) async fn list_peer_sources(peer: &PeerInfo) -> Vec<LocalizationPipelineSource> {
    let mut sources = Vec::new();
    let allowlist = build_allowlist(&peer.integration.localization_outputs);

    match peer.integration.kind {
        PeerIntegrationKind::LimelightOs => {
            if allowlist.is_empty() || allowlist.iter().any(|entry| entry.output_key.as_deref() == Some("tag_poses")) {
                sources.push(build_peer_source(
                    peer,
                    PeerSourceArgs { stream_suffix: None, output_key: "tag_poses", pipeline_label: "Peer", label: None, camera_uid: None, camera_path: None, data_type: None },
                ));
            }
        }
        PeerIntegrationKind::Photonvision => {
            let host = resolve_peer_host(peer);
            if let Some(host) = host
                && let Some(snapshot) = photonvision_snapshot_cached(&host).await
            {
                let allowed_entries: Vec<&PeerOutputEntry> = allowlist.iter().filter(|entry| entry.output_key.as_deref() == Some("tag_poses")).collect();
                let allow_all = allowed_entries.is_empty() || allowed_entries.iter().any(|entry| entry.stream_suffix.is_none());
                let allowed_camera_names: HashSet<String> = allowed_entries.iter().filter_map(|entry| entry.stream_suffix.clone()).collect();

                for camera in snapshot.cameras {
                    if !allow_all && !allowed_camera_names.contains(&camera.name) {
                        continue;
                    }
                    let camera_uid = format!("peer:{}:{}", peer.id, camera.name);
                    let camera_path = format!("{host}:5810");
                    sources.push(build_peer_source(
                        peer,
                        PeerSourceArgs {
                            stream_suffix: Some(camera.name.as_str()),
                            output_key: "tag_poses",
                            pipeline_label: "Peer",
                            label: None,
                            camera_uid: Some(camera_uid.as_str()),
                            camera_path: Some(camera_path.as_str()),
                            data_type: None,
                        },
                    ));
                }
            } else if allowlist.is_empty() {
                sources.push(build_peer_source(
                    peer,
                    PeerSourceArgs { stream_suffix: None, output_key: "tag_poses", pipeline_label: "Peer", label: None, camera_uid: None, camera_path: None, data_type: None },
                ));
            } else {
                sources.extend(build_allowlist_sources(peer, &allowlist));
            }
        }
        PeerIntegrationKind::Helios => match fetch_helios_sources(peer).await {
            Ok(remote_sources) => {
                let filtered = if allowlist.is_empty() { remote_sources } else { filter_sources_allowlist(remote_sources, &allowlist) };
                sources.extend(map_helios_sources(peer, filtered));
            }
            Err(_) => {
                let with_stream: Vec<PeerOutputEntry> = allowlist.iter().filter(|entry| entry.stream_suffix.is_some()).cloned().collect();
                sources.extend(build_allowlist_sources(peer, &with_stream));
            }
        },
        PeerIntegrationKind::Custom => {
            let mut custom_allowlist = allowlist;
            if custom_allowlist.is_empty() {
                custom_allowlist = default_custom_outputs(peer.integration.custom.as_ref().and_then(|custom| custom.mapping.as_ref()));
            }
            sources.extend(build_allowlist_sources(peer, &custom_allowlist));
        }
    }

    sources
}

pub(crate) async fn fetch_peer_output_value(peer: &PeerInfo, stream_suffix: Option<&str>, output_key: &str) -> Result<JsonValue, String> {
    let sample = fetch_peer_output_sample(peer, stream_suffix, output_key).await?;
    Ok(sample.value)
}

pub(crate) async fn fetch_peer_output_sample(peer: &PeerInfo, stream_suffix: Option<&str>, output_key: &str) -> Result<PipelineOutputSample, String> {
    match peer.integration.kind {
        PeerIntegrationKind::LimelightOs => fetch_limelight_tag_poses(peer).await.map(|value| PipelineOutputSample { data_type: None, value }),
        PeerIntegrationKind::Photonvision => fetch_photonvision_tag_poses(peer, stream_suffix).await.map(|value| PipelineOutputSample { data_type: None, value }),
        PeerIntegrationKind::Helios => fetch_helios_output(peer, stream_suffix, output_key).await,
        PeerIntegrationKind::Custom => fetch_custom_output(peer, output_key).await,
    }
}

pub(crate) fn parse_peer_stream_id(value: &str) -> (String, Option<String>) {
    let mut parts = value.splitn(2, ':');
    let peer_id = parts.next().unwrap_or_default().to_string();
    let camera = parts.next().map(|s| s.to_string()).filter(|s| !s.trim().is_empty());
    (peer_id, camera)
}

pub(crate) async fn fetch_limelight_tag_poses(peer: &PeerInfo) -> Result<JsonValue, String> {
    let host = resolve_peer_host(peer).ok_or_else(|| "peer api_base_url missing host".to_string())?;

    let custom_api = peer.integration.custom.as_ref().and_then(|custom| custom.api_endpoint.as_deref()).map(str::trim).filter(|value| !value.is_empty()).map(|value| value.to_string());

    let url = if let Some(value) = custom_api {
        if value.contains("://") {
            value
        } else if value.starts_with('/') {
            format!("http://{host}:5807{value}")
        } else {
            format!("http://{host}:5807/{value}")
        }
    } else {
        format!("http://{host}:5807/results")
    };

    let resp = PEER_HTTP.get(url).header(ACCEPT, "application/json").timeout(std::time::Duration::from_millis(1500)).send().await.map_err(|e| format!("limelight request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("limelight returned {}", resp.status()));
    }
    let payload: JsonValue = resp.json().await.map_err(|e| format!("invalid json: {e}"))?;

    let fiducials = payload.get("Results").and_then(|v| v.get("Fiducial")).and_then(|v| v.as_array()).cloned().unwrap_or_default();

    let mut detections = Vec::new();
    for entry in fiducials {
        let id = entry.get("fid").and_then(|v| v.as_i64()).unwrap_or(-1);
        if id < 0 {
            continue;
        }
        let translation = serde_json::json!({
            "x": entry.get("tx").and_then(|v| v.as_f64()).unwrap_or(0.0),
            "y": entry.get("ty").and_then(|v| v.as_f64()).unwrap_or(0.0),
            "z": entry.get("tz").and_then(|v| v.as_f64()).unwrap_or(0.0),
        });
        let rotation = serde_json::json!({
            "roll": entry.get("rx").and_then(|v| v.as_f64()).unwrap_or(0.0),
            "pitch": entry.get("ry").and_then(|v| v.as_f64()).unwrap_or(0.0),
            "yaw": entry.get("rz").and_then(|v| v.as_f64()).unwrap_or(0.0),
        });
        detections.push(serde_json::json!({
            "id": id,
            "translation": translation,
            "rotation": rotation,
            "area": entry.get("ta").and_then(|v| v.as_f64()).unwrap_or(0.0),
            "ambiguity": entry.get("ambiguity").and_then(|v| v.as_f64()).unwrap_or(0.0),
        }));
    }

    Ok(serde_json::json!({
        "detections": detections,
        "stats": {
            "source": "limelight",
            "host": host,
        }
    }))
}

pub(crate) async fn fetch_photonvision_tag_poses(peer: &PeerInfo, stream_suffix: Option<&str>) -> Result<JsonValue, String> {
    let host = resolve_peer_host(peer).ok_or_else(|| "peer api_base_url missing host".to_string())?;
    let camera = stream_suffix.unwrap_or("front");
    crate::nt4::photonvision::fetch_tag_poses(&host, 5810, camera, 700).await
}

async fn fetch_helios_sources(peer: &PeerInfo) -> Result<Vec<LocalizationPipelineSource>, String> {
    let base = peer.api_base_url.trim();
    if base.is_empty() {
        return Err("peer api_base_url missing".to_string());
    }
    let url = build_peer_url(base, "/localization/sources")?;
    let resp = PEER_HTTP.get(url).header(ACCEPT, "application/json").timeout(std::time::Duration::from_millis(1500)).send().await.map_err(|e| format!("helios request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("helios returned {}", resp.status()));
    }
    let payload: Vec<LocalizationPipelineSource> = resp.json().await.map_err(|e| format!("invalid json: {e}"))?;
    Ok(payload)
}

async fn fetch_helios_output(peer: &PeerInfo, stream_suffix: Option<&str>, output_key: &str) -> Result<PipelineOutputSample, String> {
    let base = peer.api_base_url.trim();
    if base.is_empty() {
        return Err("peer api_base_url missing".to_string());
    }

    let remote_stream_id = stream_suffix
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            if let Some(scoped) = value.strip_prefix("peer:") {
                let (scoped_peer_id, scoped_stream) = parse_peer_stream_id(scoped);
                if scoped_peer_id == peer.id {
                    return scoped_stream.unwrap_or(scoped_peer_id);
                }
            }
            value.to_string()
        })
        .ok_or_else(|| "peer stream id missing".to_string())?;
    let path = format!("/localization/streams/{remote_stream_id}/outputs/{output_key}");
    let url = build_peer_url(base, &path)?;

    let resp = PEER_HTTP.get(url).header(ACCEPT, "application/json").timeout(std::time::Duration::from_millis(1500)).send().await.map_err(|e| format!("helios request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("helios returned {}", resp.status()));
    }
    let payload: PipelineOutputSample = resp.json().await.map_err(|e| format!("invalid json: {e}"))?;
    Ok(payload)
}

async fn fetch_custom_output(peer: &PeerInfo, output_key: &str) -> Result<PipelineOutputSample, String> {
    let custom = peer.integration.custom.as_ref().ok_or_else(|| "custom integration missing".to_string())?;
    let endpoint = custom.api_endpoint.as_deref().unwrap_or("");
    let api_endpoint = localization_peer_custom::resolve_custom_base_url(&peer.api_base_url, endpoint);
    let url = localization_peer_custom::build_custom_url(&api_endpoint, output_key)?;
    let resp = PEER_HTTP.get(url).header(ACCEPT, "application/json").timeout(std::time::Duration::from_millis(1500)).send().await.map_err(|e| format!("custom request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("custom returned {}", resp.status()));
    }
    let payload: JsonValue = resp.json().await.map_err(|e| format!("invalid json: {e}"))?;
    if let Some(mapping) = custom.mapping.as_ref()
        && let Some(remapped) = localization_peer_custom::apply_custom_mapping(&payload, mapping)
    {
        return Ok(PipelineOutputSample { data_type: None, value: remapped });
    }
    Ok(PipelineOutputSample { data_type: None, value: payload })
}

fn resolve_peer_host(peer: &PeerInfo) -> Option<String> {
    let base = peer.api_base_url.trim();
    if base.is_empty() {
        return None;
    }
    Url::parse(base).ok().and_then(|url| url.host_str().map(|value| value.to_string()))
}

fn build_allowlist(outputs: &[String]) -> Vec<PeerOutputEntry> {
    let mut allowlist = Vec::new();
    for output in outputs {
        if let Some(entry) = PeerOutputEntry::parse(output) {
            allowlist.push(entry);
        }
    }
    allowlist
}

fn build_allowlist_sources(peer: &PeerInfo, allowlist: &[PeerOutputEntry]) -> Vec<LocalizationPipelineSource> {
    allowlist
        .iter()
        .map(|entry| {
            build_peer_source(
                peer,
                PeerSourceArgs {
                    stream_suffix: entry.stream_suffix.as_deref(),
                    output_key: entry.output_key.as_deref().unwrap_or("tag_poses"),
                    pipeline_label: "Peer",
                    label: entry.label.as_deref(),
                    camera_uid: entry.camera_uid.as_deref(),
                    camera_path: entry.camera_path.as_deref(),
                    data_type: entry.data_type.as_ref(),
                },
            )
        })
        .collect()
}

fn default_custom_outputs(mapping: Option<&PeerIntegrationMapping>) -> Vec<PeerOutputEntry> {
    let mut entries = Vec::new();
    if mapping.and_then(|mapping| mapping.pose.as_ref()).is_some() {
        entries.push(PeerOutputEntry {
            stream_suffix: None,
            output_key: Some("pose".to_string()),
            label: Some("Pose".to_string()),
            camera_uid: None,
            camera_path: None,
            data_type: Some(serde_json::json!({
                "type": "pose",
            })),
        });
    }
    if mapping.and_then(|mapping| mapping.aruco.as_ref()).is_some() {
        entries.push(PeerOutputEntry {
            stream_suffix: None,
            output_key: Some("tag_poses".to_string()),
            label: Some("ArUco tags".to_string()),
            camera_uid: None,
            camera_path: None,
            data_type: Some(serde_json::json!({
                "type": "aruco",
            })),
        });
    }
    entries
}

fn filter_sources_allowlist(sources: Vec<LocalizationPipelineSource>, allowlist: &[PeerOutputEntry]) -> Vec<LocalizationPipelineSource> {
    sources.into_iter().filter(|source| allowlist.iter().any(|entry| entry.matches(source))).collect()
}

fn map_helios_sources(peer: &PeerInfo, sources: Vec<LocalizationPipelineSource>) -> Vec<LocalizationPipelineSource> {
    sources
        .into_iter()
        .map(|mut source| {
            let remote_stream_id = source.stream_id.clone();
            source.stream_id = format!("peer:{}:{remote_stream_id}", peer.id);
            source.id = format!("peer:{}:{}", peer.id, source.id);
            source.stream_label = format!("{} · {}", peer_label(peer), source.stream_label);
            source.pipeline_label = format!("{} (peer)", source.pipeline_label);
            let scoped_camera_uid = source.camera_uid.trim().to_string();
            let scoped_camera_uid = if scoped_camera_uid.is_empty() {
                format!("peer:{}:{remote_stream_id}", peer.id)
            } else if scoped_camera_uid.starts_with("peer:") {
                scoped_camera_uid
            } else {
                format!("peer:{}:{scoped_camera_uid}", peer.id)
            };
            source.camera_uid = scoped_camera_uid;
            source
        })
        .collect()
}

struct PeerSourceArgs<'a> {
    stream_suffix: Option<&'a str>,
    output_key: &'a str,
    pipeline_label: &'a str,
    label: Option<&'a str>,
    camera_uid: Option<&'a str>,
    camera_path: Option<&'a str>,
    data_type: Option<&'a JsonValue>,
}

fn build_peer_source(peer: &PeerInfo, args: PeerSourceArgs<'_>) -> LocalizationPipelineSource {
    let stream_id = if let Some(stream_suffix) = args.stream_suffix { format!("peer:{}:{}", peer.id, stream_suffix) } else { format!("peer:{}", peer.id) };
    let scoped_camera_uid = args
        .camera_uid
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| if value.starts_with("peer:") { value.to_string() } else { format!("peer:{}:{value}", peer.id) })
        .unwrap_or_else(|| format!("peer:{}", peer.id));

    LocalizationPipelineSource {
        id: format!("{stream_id}:{}", args.output_key),
        stream_id,
        stream_label: format!("{}{}", peer_label(peer), args.label.map(|value| format!(" · {value}")).unwrap_or_default()),
        camera_uid: scoped_camera_uid,
        camera_path: args.camera_path.map(|value| value.to_string()).unwrap_or_else(|| peer.api_base_url.clone()),
        pipeline_id: "peer".to_string(),
        pipeline_label: args.pipeline_label.to_string(),
        output_key: args.output_key.to_string(),
        data_type: args.data_type.cloned(),
    }
}

fn peer_label(peer: &PeerInfo) -> String {
    peer.alias.clone().filter(|value| !value.trim().is_empty()).unwrap_or_else(|| peer.id.clone())
}

fn build_peer_url(base: &str, path: &str) -> Result<String, String> {
    let base = base.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("peer api_base_url missing".to_string());
    }
    let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
    let candidate = if base.ends_with("/v1") { format!("{base}{path}") } else { format!("{base}/v1{path}") };
    Url::parse(&candidate).map(|url| url.to_string()).map_err(|err| format!("invalid url: {err}"))
}

#[derive(Debug, Clone)]
struct PeerOutputEntry {
    stream_suffix: Option<String>,
    output_key: Option<String>,
    label: Option<String>,
    camera_uid: Option<String>,
    camera_path: Option<String>,
    data_type: Option<JsonValue>,
}

impl PeerOutputEntry {
    fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        let mut parts = raw.splitn(2, ':');
        let output_key = parts.next().map(|value| value.to_string());
        let stream_suffix = parts.next().map(|value| value.to_string()).filter(|value| !value.trim().is_empty());
        Some(Self { stream_suffix, output_key, label: None, camera_uid: None, camera_path: None, data_type: None })
    }

    fn matches(&self, source: &LocalizationPipelineSource) -> bool {
        if let Some(output_key) = self.output_key.as_ref()
            && output_key != &source.output_key
        {
            return false;
        }
        if let Some(suffix) = self.stream_suffix.as_ref() {
            let suffix = suffix.trim();
            let expected = format!(":{suffix}");
            if !source.stream_id.ends_with(&expected) {
                return false;
            }
        }
        true
    }
}
