use axum::{Json, http::StatusCode, response::IntoResponse};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant, timeout};
use tracing::{info, warn};
use utoipa::ToSchema;

use super::super::error::{ApiError, ApiResult};
use lib_net::interface::{IpAssignment, IpMode, NetworkInterfaceSettings, get_interfaces, set_interface};

const PERSIST_NETWORKD_DIR: &str = "/var/lib/helios/networkd";
const PERSIST_NETWORKD_PREFIX: &str = "00-helios-persisted";

#[derive(Default)]
struct DeviceState {
    team: Option<TeamNumber>,
}

static DEVICE_STATE: Lazy<RwLock<DeviceState>> = Lazy::new(|| RwLock::new(DeviceState::default()));

#[derive(Copy, Clone, Debug)]
struct TeamNumber(u32);

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TeamNumberPayload {
    pub team_number: Option<u32>,
}

impl TryFrom<u32> for TeamNumber {
    type Error = ApiError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value == 0 { Err(ApiError::bad_request("team must be greater than zero")) } else { Ok(TeamNumber(value)) }
    }
}

fn team_file_path() -> PathBuf {
    std::env::var_os("HELIOS_TEAM_FILE").map(PathBuf::from).unwrap_or_else(|| "/etc/helios/team".into())
}

async fn read_team_file() -> Result<Option<u32>, ApiError> {
    let path = team_file_path();
    let content = match tokio::fs::read_to_string(&path).await {
        Ok(data) => data,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(ApiError::internal(format!("failed to read team file {}: {err}", path.display()))),
    };
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed: u32 = trimmed.parse().map_err(|err| ApiError::internal(format!("invalid team file {}: {err}", path.display())))?;
    if parsed == 0 {
        return Ok(None);
    }
    Ok(Some(parsed))
}

async fn write_team_file(team: u32) -> Result<(), ApiError> {
    let path = team_file_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|err| ApiError::internal(format!("failed to create team directory {}: {err}", parent.display())))?;
    }
    tokio::fs::write(&path, format!("{team}\n")).await.map_err(|err| ApiError::internal(format!("failed to write team file {}: {err}", path.display())))?;
    Ok(())
}

async fn clear_team_file() -> Result<(), ApiError> {
    let path = team_file_path();
    match tokio::fs::remove_file(&path).await {
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(ApiError::internal(format!("failed to remove team file {}: {err}", path.display()))),
    }
}

#[utoipa::path(
    get,
    path = "/device/network",
    tag = "Device",
    responses((status = 200, description = "Network config", body = [NetworkInterfaceSettings]), (status = 502, description = "Network unavailable", body = super::super::error::ErrorBody))
)]
pub async fn network() -> ApiResult<impl IntoResponse> {
    let ifaces = get_interfaces().await.map_err(ApiError::from)?;
    Ok(Json(ifaces))
}

#[utoipa::path(
    post,
    path = "/device/network",
    tag = "Device",
    request_body = NetworkInterfaceSettings,
    responses((status = 204, description = "Network config updated"), (status = 400, description = "Invalid request", body = super::super::error::ErrorBody))
)]
pub async fn set_network(Json(payload): Json<NetworkInterfaceSettings>) -> ApiResult<impl IntoResponse> {
    set_interface(&payload).await.map_err(ApiError::from)?;
    persist_networkd_config(&payload).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/device/team",
    tag = "Device",
    responses((status = 200, description = "Team number", body = TeamNumberPayload))
)]
pub async fn team() -> ApiResult<impl IntoResponse> {
    let mut team_value = {
        let state = DEVICE_STATE.read().await;
        state.team.map(|team| team.0)
    };

    if team_value.is_none() {
        match read_team_file().await {
            Ok(value) => team_value = value,
            Err(err) => warn!(error = ?err, "failed to load team file"),
        }
        if let Some(value) = team_value {
            DEVICE_STATE.write().await.team = Some(TeamNumber::try_from(value)?);
        }
    }

    Ok(Json(TeamNumberPayload { team_number: team_value }))
}

#[utoipa::path(
    post,
    path = "/device/team",
    tag = "Device",
    request_body(content = TeamNumberPayload, content_type = "application/json"),
    responses((status = 204, description = "Team updated"), (status = 400, description = "Invalid request", body = super::super::error::ErrorBody))
)]
pub async fn set_team(Json(payload): Json<TeamNumberPayload>) -> ApiResult<impl IntoResponse> {
    match payload.team_number {
        None => {
            DEVICE_STATE.write().await.team = None;
            clear_team_file().await?;
            Ok(StatusCode::NO_CONTENT)
        }
        Some(value) => {
            let team = TeamNumber::try_from(value)?;
            DEVICE_STATE.write().await.team = Some(team);
            write_team_file(value).await?;
            Ok(StatusCode::NO_CONTENT)
        }
    }
}

fn candidate_team_bytes_from_ipv4(ip: Ipv4Addr) -> Option<(u8, u8)> {
    let [a0, a1, a2, _] = ip.octets();
    if a0 != 10 {
        return None;
    }
    if a1 == 0 && a2 == 0 {
        return None;
    }
    Some((a1, a2))
}

fn team_from_bytes(a: u8, b: u8) -> Option<u32> {
    let team = (a as u32) * 100 + (b as u32);
    if team == 0 || team > 25_599 { None } else { Some(team) }
}

async fn probe_nt4_port(a: u8, b: u8, timeout_ms: u64) -> bool {
    let addr = (Ipv4Addr::new(10, a, b, 2), 5810);
    matches!(timeout(Duration::from_millis(timeout_ms), TcpStream::connect(addr)).await, Ok(Ok(_)))
}

async fn infer_team_from_interfaces(ifaces: &[NetworkInterfaceSettings]) -> Option<u32> {
    let mut candidates: HashSet<(u8, u8)> = HashSet::new();

    for iface in ifaces {
        if let Some(IpAddr::V4(ip)) = iface.address
            && let Some(bytes) = candidate_team_bytes_from_ipv4(ip)
        {
            candidates.insert(bytes);
        }
        if let Some(IpAddr::V4(ip)) = iface.gateway
            && let Some(bytes) = candidate_team_bytes_from_ipv4(ip)
        {
            candidates.insert(bytes);
        }
        for assignment in &iface.ipv4 {
            if let IpAddr::V4(ip) = assignment.address
                && let Some(bytes) = candidate_team_bytes_from_ipv4(ip)
            {
                candidates.insert(bytes);
            }
        }
        for gateway in &iface.gateways {
            if let IpAddr::V4(ip) = *gateway
                && let Some(bytes) = candidate_team_bytes_from_ipv4(ip)
            {
                candidates.insert(bytes);
            }
        }
    }

    if candidates.is_empty() {
        return None;
    }

    let mut list = candidates.into_iter().collect::<Vec<_>>();
    list.sort();
    if list.len() == 1 {
        let (a, b) = list[0];
        return team_from_bytes(a, b);
    }

    // Multiple 10.x.y.* candidates (VPNs, containers, etc.). Prefer one that actually has a roboRIO/NT4 port.
    for (a, b) in list {
        if probe_nt4_port(a, b, 140).await {
            return team_from_bytes(a, b);
        }
    }

    None
}

pub fn spawn_team_autodetect_task() {
    tokio::spawn(async move {
        // One-time, best-effort bootstrap (avoids fighting user overrides later).
        let deadline = Instant::now() + Duration::from_secs(75);
        let mut tick = tokio::time::interval(Duration::from_secs(4));

        loop {
            tick.tick().await;
            if Instant::now() > deadline {
                break;
            }

            if DEVICE_STATE.read().await.team.is_some() {
                break;
            }

            if let Ok(Some(team)) = read_team_file().await {
                if let Ok(valid) = TeamNumber::try_from(team) {
                    DEVICE_STATE.write().await.team = Some(valid);
                }
                break;
            }

            let ifaces = match get_interfaces().await {
                Ok(ifaces) => ifaces,
                Err(err) => {
                    warn!(error = ?err, "team autodetect: failed to read interfaces");
                    continue;
                }
            };

            let Some(team) = infer_team_from_interfaces(&ifaces).await else {
                continue;
            };

            if TeamNumber::try_from(team).is_err() {
                continue;
            }

            if let Err(err) = write_team_file(team).await {
                warn!(error = ?err, team, "team autodetect: failed to write team file");
                continue;
            }

            DEVICE_STATE.write().await.team = Some(TeamNumber(team));
            info!(team, "team autodetect: persisted team number");
            break;
        }
    });
}

fn sanitize_iface_name(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

fn ipv4_netmask_to_prefix(mask: Ipv4Addr) -> u8 {
    mask.octets().iter().fold(0u32, |acc, &octet| (acc << 8) | octet as u32).count_ones() as u8
}

fn resolve_networkd_name(name: &str) -> String {
    if name == "usb0" && std::path::Path::new("/sys/class/net/usbbr0").exists() {
        return "usbbr0".to_string();
    }
    name.to_string()
}

fn render_networkd_config(settings: &NetworkInterfaceSettings) -> String {
    let interface_name = resolve_networkd_name(&settings.name);
    let mut out = String::new();
    out.push_str("[Match]\n");
    out.push_str(&format!("Name={interface_name}\n\n"));
    out.push_str("[Network]\n");

    let v4_dynamic = matches!(settings.mode, IpMode::Dynamic);
    let v6_dynamic = matches!(settings.ipv6_mode, IpMode::Dynamic);
    match (v4_dynamic, v6_dynamic) {
        (true, true) => out.push_str("DHCP=yes\n"),
        (true, false) => out.push_str("DHCP=ipv4\n"),
        (false, true) => out.push_str("DHCP=ipv6\n"),
        (false, false) => {}
    }

    if !v4_dynamic {
        let mut v4_assignments: Vec<IpAssignment> = settings.ipv4.iter().filter(|assignment| assignment.address.is_ipv4()).cloned().collect();
        if v4_assignments.is_empty()
            && let (Some(IpAddr::V4(addr)), Some(IpAddr::V4(mask))) = (settings.address, settings.netmask)
        {
            let prefix = ipv4_netmask_to_prefix(mask);
            v4_assignments.push(IpAssignment { address: IpAddr::V4(addr), prefix });
        }
        for assignment in v4_assignments {
            out.push_str(&format!("Address={}/{prefix}\n", assignment.address, prefix = assignment.prefix));
        }

        let v4_gateway = settings.gateway.filter(|ip| ip.is_ipv4()).or_else(|| settings.gateways.iter().find(|ip| ip.is_ipv4()).copied());
        if let Some(gateway) = v4_gateway {
            out.push_str(&format!("Gateway={gateway}\n"));
        }
    }

    if !v6_dynamic {
        let v6_assignments: Vec<IpAssignment> = settings.ipv6.iter().filter(|assignment| assignment.address.is_ipv6()).cloned().collect();
        for assignment in v6_assignments {
            out.push_str(&format!("Address={}/{prefix}\n", assignment.address, prefix = assignment.prefix));
        }

        let v6_gateway = settings.ipv6_gateway.filter(|ip| ip.is_ipv6()).or_else(|| settings.gateways.iter().find(|ip| ip.is_ipv6()).copied());
        if let Some(gateway) = v6_gateway {
            out.push_str(&format!("Gateway={gateway}\n"));
        }
    }

    if !settings.dns.servers.is_empty() {
        let servers = settings.dns.servers.iter().map(|ip| ip.to_string()).collect::<Vec<_>>().join(" ");
        out.push_str(&format!("DNS={servers}\n"));
    }
    if !settings.dns.search.is_empty() {
        out.push_str(&format!("Domains={}\n", settings.dns.search.join(" ")));
    }

    if interface_name == "usbbr0" {
        out.push_str("ConfigureWithoutCarrier=yes\n");
    }

    out
}

async fn write_atomic(path: &PathBuf, content: &str) -> Result<(), ApiError> {
    let file_name = path.file_name().ok_or_else(|| ApiError::internal("network config path missing filename"))?.to_string_lossy();
    let tmp_path = path.with_file_name(format!("{file_name}.tmp"));
    tokio::fs::write(&tmp_path, content).await.map_err(|err| ApiError::internal(format!("failed to write {tmp_path:?}: {err}")))?;
    tokio::fs::rename(&tmp_path, path).await.map_err(|err| ApiError::internal(format!("failed to rename {tmp_path:?}: {err}")))?;
    Ok(())
}

async fn persist_networkd_config(settings: &NetworkInterfaceSettings) -> Result<(), ApiError> {
    let file_name = format!("{PERSIST_NETWORKD_PREFIX}-{}.network", sanitize_iface_name(&settings.name));
    let content = render_networkd_config(settings);

    let persist_dir = PathBuf::from(PERSIST_NETWORKD_DIR);
    tokio::fs::create_dir_all(&persist_dir).await.map_err(|err| ApiError::internal(format!("failed to create {PERSIST_NETWORKD_DIR}: {err}")))?;
    let persist_path = persist_dir.join(&file_name);
    write_atomic(&persist_path, &content).await?;

    let systemd_dir = PathBuf::from("/etc/systemd/network");
    tokio::fs::create_dir_all(&systemd_dir).await.map_err(|err| ApiError::internal(format!("failed to create /etc/systemd/network: {err}")))?;
    let systemd_path = systemd_dir.join(&file_name);
    write_atomic(&systemd_path, &content).await?;

    Ok(())
}
