use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use lib_net::interface::{NetworkInterfaceSettings, get_interfaces};
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant, timeout};
use tracing::{info, warn};

use crate::http::error::ApiError;
use crate::http::persisted_files;

use super::types::TeamNumber;

#[derive(Default)]
struct DeviceState {
    team: Option<TeamNumber>,
}

#[derive(Default)]
pub(crate) struct DeviceNetworkState {
    team: RwLock<DeviceState>,
}

impl DeviceNetworkState {
    pub(crate) async fn team_value(&self) -> Result<Option<u32>, ApiError> {
        let mut team_value = {
            let state = self.team.read().await;
            state.team.map(|team| team.0)
        };

        if team_value.is_none() {
            match read_team_file().await {
                Ok(value) => team_value = value,
                Err(err) => warn!(error = ?err, "failed to load team file"),
            }
            if let Some(value) = team_value {
                self.team.write().await.team = Some(TeamNumber::try_from(value)?);
            }
        }

        Ok(team_value)
    }

    pub(crate) async fn set_team_value(&self, team_number: Option<u32>) -> Result<(), ApiError> {
        match team_number {
            None => {
                self.team.write().await.team = None;
                clear_team_file().await?;
            }
            Some(value) => {
                let team = TeamNumber::try_from(value)?;
                self.team.write().await.team = Some(team);
                write_team_file(value).await?;
            }
        }
        Ok(())
    }

    pub(crate) fn spawn_team_autodetect_task(self: Arc<Self>) {
        tokio::spawn(async move {
            let deadline = Instant::now() + Duration::from_secs(75);
            let mut tick = tokio::time::interval(Duration::from_secs(4));

            loop {
                tick.tick().await;
                if Instant::now() > deadline {
                    break;
                }

                if self.team.read().await.team.is_some() {
                    break;
                }

                if let Ok(Some(team)) = read_team_file().await {
                    if let Ok(valid) = TeamNumber::try_from(team) {
                        self.team.write().await.team = Some(valid);
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

                self.team.write().await.team = Some(TeamNumber(team));
                info!(team, "team autodetect: persisted team number");
                break;
            }
        });
    }
}

async fn read_team_file() -> Result<Option<u32>, ApiError> {
    let path = persisted_files::team_file_path();
    match persisted_files::read_team_number().await {
        Ok(team) => Ok(team),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(ApiError::internal(format!("failed to read team file {}: {err}", path.display()))),
    }
}

async fn write_team_file(team: u32) -> Result<(), ApiError> {
    let path = persisted_files::team_file_path();
    persisted_files::write_team_number(team).await.map_err(|err| ApiError::internal(format!("failed to write team file {}: {err}", path.display())))?;
    Ok(())
}

async fn clear_team_file() -> Result<(), ApiError> {
    let path = persisted_files::team_file_path();
    persisted_files::clear_team_number().await.map_err(|err| ApiError::internal(format!("failed to remove team file {}: {err}", path.display())))
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
        if let Some(IpAddr::V4(ip)) = iface.address {
            if let Some(bytes) = candidate_team_bytes_from_ipv4(ip) {
                candidates.insert(bytes);
            }
        }
        if let Some(IpAddr::V4(ip)) = iface.gateway {
            if let Some(bytes) = candidate_team_bytes_from_ipv4(ip) {
                candidates.insert(bytes);
            }
        }
        for assignment in &iface.ipv4 {
            if let IpAddr::V4(ip) = assignment.address {
                if let Some(bytes) = candidate_team_bytes_from_ipv4(ip) {
                    candidates.insert(bytes);
                }
            }
        }
        for gateway in &iface.gateways {
            if let IpAddr::V4(ip) = *gateway {
                if let Some(bytes) = candidate_team_bytes_from_ipv4(ip) {
                    candidates.insert(bytes);
                }
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

    for (a, b) in list {
        if probe_nt4_port(a, b, 140).await {
            return team_from_bytes(a, b);
        }
    }

    None
}
