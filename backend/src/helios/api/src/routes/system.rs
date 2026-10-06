//! Health, identity, device facts, metrics, services, processes and reboot.

use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::{
    API_VERSION, SharedState, VERSION,
    config::MANAGED_UNITS,
    error::{ApiError, ApiResult},
    host::{self, DiskUsage, UnitStatus, now_ms, read_trimmed},
    orion::{enum_name, label_map},
};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Health {
    pub service: &'static str,
    pub status: &'static str,
    pub api_version: &'static str,
    pub version: &'static str,
}

pub async fn health() -> Json<Health> {
    Json(Health { service: "helios-api", status: "ok", api_version: API_VERSION, version: VERSION })
}

// --- identity ----------------------------------------------------------------

const DEVICE_PACKAGE_ENV: &str = "/usr/lib/pd-device/device-package.env";

/// The device identity: the Raze device package's identity document (the same JSON as
/// `:5899/.well-known/pd-device`), with a `helios` section describing this API. When the device
/// package has not written it, the same fields are read from the system.
pub async fn identity(State(state): State<SharedState>) -> Json<serde_json::Value> {
    let path = state.config.pd_identity_path.clone();
    let node_id = state.config.node_id.clone();
    let doc = tokio::task::spawn_blocking(move || identity_document(&path, &node_id)).await.unwrap_or_else(|_| serde_json::json!({}));
    Json(doc)
}

pub fn identity_document(pd_identity_path: &std::path::Path, node_id: &str) -> serde_json::Value {
    let (mut doc, source) = match std::fs::read_to_string(pd_identity_path).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok()).filter(|v| v.is_object()) {
        Some(doc) => (doc, "pd-device"),
        None => (synthesized_identity(), "helios-api"),
    };
    if let Some(object) = doc.as_object_mut() {
        if let Some(methods) = object.get_mut("update_methods").and_then(|m| m.as_array_mut())
            && !methods.iter().any(|m| m == "helios-ota")
        {
            methods.push(serde_json::Value::from("helios-ota"));
        }
        object.insert(
            "helios".into(),
            serde_json::json!({
                "source": source,
                "api_version": API_VERSION,
                "version": VERSION,
                "node_id": node_id,
                "endpoints": {
                    "health": "/v1/health",
                    "metrics": "/v1/metrics",
                    "logs": "/v1/logs",
                    "events": "/v1/events",
                    "ota_upload": "/v1/update/uploads",
                    "ota_apply": "/v1/update/apply",
                    "ota_status": "/v1/update/status",
                    "ota_events": "/v1/update/events",
                }
            }),
        );
    }
    doc
}

fn synthesized_identity() -> serde_json::Value {
    let package = std::fs::read_to_string(DEVICE_PACKAGE_ENV).map(|text| host::parse_env(&text)).unwrap_or_default();
    let os = host::os_release();
    let serial = read_trimmed("/proc/device-tree/serial-number").map(|s| s.to_ascii_lowercase()).filter(|s| s.chars().all(|c| c.is_ascii_hexdigit()));
    let model = package.get("PD_DEVICE_MODEL").cloned().or_else(|| read_trimmed("/proc/device-tree/model"));
    serde_json::json!({
        "contract": 1,
        "model": model,
        "rev": package.get("PD_DEVICE_REV_DEFAULT"),
        "serial": serial,
        "hostname": hostname(),
        "os": { "name": os.get("ID"), "version": os.get("VERSION_ID").or_else(|| os.get("VERSION")) },
        "device_package": { "version": package.get("PD_DEVICE_PACKAGE_VERSION"), "commit": package.get("PD_DEVICE_PACKAGE_COMMIT").filter(|c| !c.is_empty()) },
        "update_methods": ["image-write"],
        "manage_url": serde_json::Value::Null,
        "macs": ethernet_macs(),
    })
}

fn hostname() -> Option<String> {
    read_trimmed("/proc/sys/kernel/hostname")
}

fn ethernet_macs() -> BTreeMap<String, String> {
    let Ok(entries) = std::fs::read_dir("/sys/class/net") else { return BTreeMap::new() };
    entries
        .filter_map(Result::ok)
        .filter(|entry| read_trimmed(entry.path().join("type")).as_deref() == Some("1"))
        .filter_map(|entry| Some((entry.file_name().to_string_lossy().into_owned(), read_trimmed(entry.path().join("address"))?.to_ascii_lowercase())))
        .filter(|(name, mac)| name != "lo" && mac != "00:00:00:00:00:00")
        .collect()
}

// --- device --------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DeviceOs {
    pub name: Option<String>,
    pub version: Option<String>,
    pub pretty_name: Option<String>,
    pub build_id: Option<String>,
    pub kernel: Option<String>,
}

fn device_os_now() -> DeviceOs {
    let os = host::os_release();
    DeviceOs {
        name: os.get("NAME").cloned(),
        version: os.get("VERSION_ID").or_else(|| os.get("VERSION")).cloned().or_else(|| read_trimmed("/etc/helios/version")),
        pretty_name: os.get("PRETTY_NAME").cloned(),
        build_id: read_trimmed("/etc/helios/build-id"),
        kernel: read_trimmed("/proc/sys/kernel/osrelease"),
    }
}

pub async fn device_os() -> Json<DeviceOs> {
    Json(device_os_now())
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OrionStatus {
    pub reachable: bool,
    pub desired_revision: Option<u64>,
    pub observed_revision: Option<u64>,
    pub peers_ready: Option<u64>,
    pub peers_configured: Option<u64>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Device {
    pub node_id: String,
    pub hostname: Option<String>,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub architecture: String,
    pub os: DeviceOs,
    pub uptime_s: Option<f64>,
    pub api_version: &'static str,
    pub api: &'static str,
    pub orion: OrionStatus,
    pub clock: Option<serde_json::Value>,
}

pub async fn device(State(state): State<SharedState>) -> Json<Device> {
    let orion = match state.orion.observability().await {
        Ok(snapshot) => OrionStatus {
            reachable: true,
            desired_revision: Some(snapshot.desired_revision.get()),
            observed_revision: Some(snapshot.observed_revision.get()),
            peers_ready: Some(snapshot.ready_peer_count),
            peers_configured: Some(snapshot.configured_peer_count),
            message: None,
        },
        Err(error) => OrionStatus { reachable: false, desired_revision: None, observed_revision: None, peers_ready: None, peers_configured: None, message: Some(error.message) },
    };
    let clock = match state.orion.view().await {
        Ok(view) => view.nodes.get(&state.config.node_id).and_then(|node| node.clock.as_ref()).map(clock_json),
        Err(_) => None,
    };
    Json(Device {
        node_id: state.config.node_id.clone(),
        hostname: hostname(),
        model: read_trimmed("/proc/device-tree/model"),
        serial: read_trimmed("/proc/device-tree/serial-number").map(|s| s.to_ascii_lowercase()),
        architecture: std::env::consts::ARCH.to_string(),
        os: device_os_now(),
        uptime_s: read_trimmed("/proc/uptime").and_then(|text| text.split_whitespace().next().and_then(|v| v.parse().ok())),
        api_version: API_VERSION,
        api: VERSION,
        orion,
        clock,
    })
}

fn clock_json(clock: &orion::control_plane::NodeClockFacts) -> serde_json::Value {
    serde_json::json!({
        "source": enum_name(&clock.source),
        "synchronized": clock.synchronized,
        "offset_us": clock.offset_ns.map(|ns| ns as f64 / 1000.0),
        "stratum": clock.stratum,
        "timebase": clock.timebase,
    })
}

/// Orion's nodes: this device and the peers it knows.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NodeSummary {
    pub id: String,
    pub local: bool,
    pub health: String,
    pub schedulable: bool,
    pub labels: BTreeMap<String, String>,
    pub hostname: Option<String>,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub image_version: Option<String>,
    pub kernel: Option<String>,
    pub architecture: Option<String>,
    pub clock: Option<serde_json::Value>,
}

pub async fn nodes(State(state): State<SharedState>) -> ApiResult<Json<Vec<NodeSummary>>> {
    let view = state.orion.view().await?;
    Ok(Json(
        view.nodes
            .values()
            .map(|node| {
                let host = node.host.as_ref();
                NodeSummary {
                    id: node.node_id.to_string(),
                    local: node.node_id.as_str() == state.config.node_id,
                    health: enum_name(node.health),
                    schedulable: node.schedulable,
                    labels: label_map(&node.labels),
                    hostname: host.and_then(|h| h.hostname.clone()),
                    os_name: host.and_then(|h| h.os_name.clone()),
                    os_version: host.and_then(|h| h.os_version.clone()),
                    image_version: host.and_then(|h| h.image_version.clone()),
                    kernel: host.and_then(|h| h.kernel_release.clone()),
                    architecture: host.and_then(|h| h.architecture.clone()),
                    clock: node.clock.as_ref().map(clock_json),
                }
            })
            .collect(),
    ))
}

// --- metrics -------------------------------------------------------------------

/// One reading in the form Atlas's metrics reader accepts.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Reading {
    pub id: &'static str,
    pub label: &'static str,
    pub value: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warn_above: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Metrics {
    pub at_ms: u64,
    /// Busy fraction of all cores, 0..1, since the previous sample.
    pub cpu: Option<f64>,
    pub cpu_cores: Vec<f64>,
    pub load: Option<[f64; 3]>,
    pub memory_total_bytes: Option<u64>,
    pub memory_available_bytes: Option<u64>,
    pub temperature_c: Option<f64>,
    pub throttled: Option<u32>,
    pub disk: Option<DiskUsage>,
    pub uptime_s: Option<u64>,
    /// The same numbers as a flat list (`{"metrics": [...]}` is what Atlas reads).
    pub metrics: Vec<Reading>,
}

pub async fn metrics_now(state: &SharedState) -> Metrics {
    let host_metrics = state.orion.observability().await.ok().map(|snapshot| snapshot.host);
    let (cpu, cpu_cores) = match state.cpu.sample() {
        Some((total, cores)) => (Some(total), cores),
        None => (None, Vec::new()),
    };
    let load = host_metrics.as_ref().and_then(|h| Some([h.load_1_milli? as f64 / 1000.0, h.load_5_milli? as f64 / 1000.0, h.load_15_milli? as f64 / 1000.0]));
    let memory_total_bytes = host_metrics.as_ref().and_then(|h| h.memory_total_bytes);
    let memory_available_bytes = host_metrics.as_ref().and_then(|h| h.memory_available_bytes);
    let uptime_s = host_metrics.as_ref().and_then(|h| h.uptime_seconds);
    let temperature_c = host::temperature_c();
    let disk = host::disk_usage("/var/lib/helios").or_else(|| host::disk_usage("/"));
    let mut readings = Vec::new();
    if let Some(cpu) = cpu {
        readings.push(Reading { id: "cpu", label: "CPU", value: (cpu * 1000.0).round() / 10.0, unit: Some("%"), warn_above: Some(90.0) });
    }
    if let (Some(total), Some(available)) = (memory_total_bytes, memory_available_bytes) {
        readings.push(Reading {
            id: "memory",
            label: "Memory used",
            value: (total.saturating_sub(available) as f64 / (1 << 20) as f64).round(),
            unit: Some("MiB"),
            warn_above: Some(total as f64 / (1 << 20) as f64 * 0.9),
        });
    }
    if let Some(temp) = temperature_c {
        readings.push(Reading { id: "temperature", label: "SoC temperature", value: (temp * 10.0).round() / 10.0, unit: Some("°C"), warn_above: Some(80.0) });
    }
    if let Some(disk) = &disk {
        readings.push(Reading {
            id: "disk",
            label: "Data used",
            value: (disk.used_bytes as f64 / (1 << 20) as f64).round(),
            unit: Some("MiB"),
            warn_above: Some(disk.total_bytes as f64 / (1 << 20) as f64 * 0.9),
        });
    }
    if let Some(load) = load {
        readings.push(Reading { id: "load1", label: "Load (1 min)", value: load[0], unit: None, warn_above: None });
    }
    Metrics { at_ms: now_ms(), cpu, cpu_cores, load, memory_total_bytes, memory_available_bytes, temperature_c, throttled: host::throttled(), disk, uptime_s, metrics: readings }
}

pub async fn metrics(State(state): State<SharedState>) -> Json<Metrics> {
    Json(metrics_now(&state).await)
}

/// The full helios-diagnostics health report (`heliosctl doctor`).
pub async fn health_report() -> ApiResult<Json<serde_json::Value>> {
    let report = tokio::task::spawn_blocking(|| helios_diagnostics::collect_health_report(&helios_diagnostics::config::DiagnosticsConfig::default()))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?
        .map_err(|error| ApiError::internal(format!("health report failed: {error:#}")))?;
    serde_json::to_value(report).map(Json).map_err(|error| ApiError::internal(error.to_string()))
}

// --- services ------------------------------------------------------------------

pub async fn services() -> ApiResult<Json<Vec<UnitStatus>>> {
    let mut out = Vec::new();
    for unit in MANAGED_UNITS {
        out.push(host::unit_status(unit).await?);
    }
    Ok(Json(out))
}

fn managed_unit(name: &str) -> ApiResult<&'static str> {
    let wanted = if name.ends_with(".service") { name.to_string() } else { format!("{name}.service") };
    MANAGED_UNITS.iter().copied().find(|unit| *unit == wanted).ok_or_else(|| ApiError::not_found(format!("{name} is not a HeliOS service")))
}

pub async fn restart_service(Path(unit): Path<String>) -> ApiResult<(StatusCode, Json<UnitStatus>)> {
    let unit = managed_unit(&unit)?;
    if unit == "helios-api.service" {
        // Restarting ourselves: answer first.
        tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let _ = host::systemctl(&["restart", "helios-api.service"]).await;
        });
        return Ok((StatusCode::ACCEPTED, Json(host::unit_status(unit).await?)));
    }
    host::systemctl(&["restart", unit]).await?;
    Ok((StatusCode::OK, Json(host::unit_status(unit).await?)))
}

#[derive(Debug, Serialize)]
pub struct Accepted {
    pub accepted: bool,
    pub message: String,
}

pub async fn reboot() -> (StatusCode, Json<Accepted>) {
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if let Err(error) = host::systemctl(&["reboot"]).await {
            tracing::error!(error = %error.message, "reboot failed");
        }
    });
    (StatusCode::ACCEPTED, Json(Accepted { accepted: true, message: "rebooting".into() }))
}

pub async fn safe_mode() -> ApiError {
    ApiError::not_available(
        "safe mode is not available on this device",
        "a safe-mode target in the HeliOS image (stop helios-engine and helios-peripherals, keep orion-node, the updater and the API) plus a boot flag the updater honours",
    )
}

// --- processes -----------------------------------------------------------------

pub async fn processes() -> ApiResult<Json<Vec<host::ProcessInfo>>> {
    tokio::task::spawn_blocking(host::processes).await.map(Json).map_err(|error| ApiError::internal(error.to_string()))
}

#[derive(Debug, Deserialize)]
pub struct SignalRequest {
    pub signal: String,
}

pub async fn signal_process(Path(pid): Path<u32>, Json(request): Json<SignalRequest>) -> ApiResult<Json<Accepted>> {
    let signal = host::signal_number(&request.signal).ok_or_else(|| ApiError::bad_request(format!("unsupported signal {}", request.signal)))?;
    host::send_signal(pid, signal)?;
    Ok(Json(Accepted { accepted: true, message: format!("SIG{} sent to {pid}", request.signal.trim_start_matches("SIG")) }))
}

pub async fn process_affinity() -> ApiError {
    ApiError::not_available("changing CPU affinity is not available", "a policy for pinning HeliOS services (systemd CPUAffinity drop-ins owned by the image), so pins survive restarts")
}

pub async fn process_nice() -> ApiError {
    ApiError::not_available("changing process priority is not available", "a policy for service priorities (systemd Nice= drop-ins owned by the image), so changes survive restarts")
}
