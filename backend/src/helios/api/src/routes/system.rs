//! Health, identity, device facts, metrics, services, processes and reboot.

use std::collections::BTreeMap;

use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::{
    API_VERSION, SharedState, VERSION,
    auth::Caller,
    config::MANAGED_UNITS,
    error::{ApiError, ApiResult},
    host::{self, DiskUsage, UnitStatus, now_ms, read_trimmed},
    orion::{enum_name, label_map},
};
use orion::control_plane::{HostMetricsSnapshot, TypedConfigValue};

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
///
/// Identity is public so Atlas can discover the device. On a secured device a caller that is not
/// signed in gets only [`PUBLIC_IDENTITY_FIELDS`]; `helios.auth.mode` tells it to bring a token.
pub async fn identity(State(state): State<SharedState>, caller: Option<Extension<Caller>>) -> Json<serde_json::Value> {
    let path = state.config.pd_identity_path.clone();
    let node_id = state.config.node_id.clone();
    let mut doc = tokio::task::spawn_blocking(move || identity_document(&path, &node_id)).await.unwrap_or_else(|_| serde_json::json!({}));
    let mode = state.auth.mode();
    let authenticated = caller.is_some_and(|Extension(caller)| caller.is_authenticated());
    if let Some(object) = doc.as_object_mut() {
        if !authenticated {
            object.retain(|key, _| PUBLIC_IDENTITY_FIELDS.contains(&key.as_str()));
        }
        if let Some(helios) = object.get_mut("helios").and_then(|h| h.as_object_mut()) {
            helios.insert("auth".into(), serde_json::json!({ "mode": mode.as_str(), "status": "/v1/auth/status" }));
        }
    }
    Json(doc)
}

/// What a signed-out caller sees of the identity on a secured device: enough for discovery, the
/// model, the OS and the update methods. MACs, endpoints and actions are left out.
pub const PUBLIC_IDENTITY_FIELDS: &[&str] = &["contract", "model", "rev", "serial", "hostname", "os", "device_package", "update_methods", "manage_url", "helios"];

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
    /// Busy fraction of all cores, 0..1, over Orion's latest CPU window.
    pub cpu: Option<f64>,
    pub cpu_cores: Vec<f64>,
    pub load: Option<[f64; 3]>,
    pub memory_total_bytes: Option<u64>,
    pub memory_available_bytes: Option<u64>,
    /// The hottest sensor Orion reports, degrees Celsius.
    pub temperature_c: Option<f64>,
    pub throttled: Option<u32>,
    pub disk: Option<DiskUsage>,
    pub uptime_s: Option<u64>,
    /// The same numbers as a flat list (`{"metrics": [...]}` is what Atlas reads).
    pub metrics: Vec<Reading>,
}

/// Orion's host metrics, as the API reads them: from an observability snapshot for
/// `GET /v1/metrics`, or from the node's `host.*` status keys for the event stream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostSample {
    /// Busy share of all CPUs, per mille.
    pub cpu_busy_milli: Option<u32>,
    /// The same per CPU, in kernel CPU order.
    pub cpu_core_busy_milli: Vec<u32>,
    /// 1, 5 and 15 minute load averages x 1000.
    pub load_milli: Option<[u64; 3]>,
    pub memory_total_bytes: Option<u64>,
    pub memory_available_bytes: Option<u64>,
    pub uptime_seconds: Option<u64>,
    /// Every sensor's reading, millidegrees Celsius.
    pub temperatures_milli_c: Vec<i32>,
}

impl HostSample {
    pub fn from_snapshot(host: &HostMetricsSnapshot) -> Self {
        Self {
            cpu_busy_milli: host.cpu_busy_milli,
            cpu_core_busy_milli: host.cpu_core_busy_milli.clone(),
            load_milli: host.load_1_milli.zip(host.load_5_milli).zip(host.load_15_milli).map(|((one, five), fifteen)| [one, five, fifteen]),
            memory_total_bytes: host.memory_total_bytes,
            memory_available_bytes: host.memory_available_bytes,
            uptime_seconds: host.uptime_seconds,
            temperatures_milli_c: host.temperatures.iter().map(|t| t.millidegrees_c).collect(),
        }
    }

    /// From the `host.*` keys orion-node publishes under `node/<id>` (Orion `docs/host-facts.md`).
    pub fn from_status(keys: &BTreeMap<String, TypedConfigValue>) -> Self {
        let uint = |key: &str| match keys.get(key) {
            Some(TypedConfigValue::UInt(value)) => Some(*value),
            Some(TypedConfigValue::Int(value)) => u64::try_from(*value).ok(),
            _ => None,
        };
        let cores: BTreeMap<usize, u32> = keys
            .iter()
            .filter_map(|(key, value)| {
                let index = key.strip_prefix("host.cpu")?.strip_suffix("_busy_milli")?.parse().ok()?;
                let TypedConfigValue::UInt(milli) = value else { return None };
                Some((index, u32::try_from(*milli).ok()?))
            })
            .collect();
        let temperatures_milli_c = keys
            .iter()
            .filter(|(key, _)| key.starts_with("host.temperature."))
            .filter_map(|(_, value)| match value {
                TypedConfigValue::Int(milli) => i32::try_from(*milli).ok(),
                TypedConfigValue::UInt(milli) => i32::try_from(*milli).ok(),
                _ => None,
            })
            .collect();
        Self {
            cpu_busy_milli: uint("host.cpu_busy_milli").and_then(|milli| u32::try_from(milli).ok()),
            cpu_core_busy_milli: cores.into_values().collect(),
            load_milli: uint("host.load1_milli").zip(uint("host.load5_milli")).zip(uint("host.load15_milli")).map(|((one, five), fifteen)| [one, five, fifteen]),
            memory_total_bytes: uint("host.memory_total_bytes"),
            memory_available_bytes: uint("host.memory_available_bytes"),
            uptime_seconds: uint("host.uptime_seconds"),
            temperatures_milli_c,
        }
    }
}

/// `GET /v1/metrics`: Orion's host metrics (a fresh observability snapshot), plus the firmware
/// throttle flags and disk usage, which HeliOS reads itself.
pub async fn metrics_now(state: &SharedState) -> Metrics {
    let host = state.orion.observability().await.ok().map(|snapshot| HostSample::from_snapshot(&snapshot.host));
    metrics_from(host.as_ref())
}

/// The metrics object for a host sample (`None` when Orion is not reachable).
pub fn metrics_from(host: Option<&HostSample>) -> Metrics {
    let cpu = host.and_then(|h| h.cpu_busy_milli).map(|milli| f64::from(milli) / 1000.0);
    let cpu_cores = host.map(|h| h.cpu_core_busy_milli.iter().map(|milli| f64::from(*milli) / 1000.0).collect()).unwrap_or_default();
    let load = host.and_then(|h| h.load_milli).map(|load| load.map(|milli| milli as f64 / 1000.0));
    let memory_total_bytes = host.and_then(|h| h.memory_total_bytes);
    let memory_available_bytes = host.and_then(|h| h.memory_available_bytes);
    let uptime_s = host.and_then(|h| h.uptime_seconds);
    // The hottest sensor.
    let temperature_c = host.and_then(|h| h.temperatures_milli_c.iter().copied().max()).map(|milli| f64::from(milli) / 1000.0);
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

/// The full helios-diagnostics health report (`helios-diagnostics doctor`).
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
