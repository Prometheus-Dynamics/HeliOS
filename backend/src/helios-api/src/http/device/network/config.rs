use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

use lib_net::interface::{IpAssignment, IpMode, NetworkInterfaceSettings};
use uuid::Uuid;

use crate::http::error::ApiError;

const PERSIST_NETWORKD_DIR: &str = "/var/lib/helios/networkd";
const PERSIST_NETWORKD_PREFIX: &str = "00-helios-persisted";

pub(super) fn render_networkd_config(settings: &NetworkInterfaceSettings) -> String {
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
        if v4_assignments.is_empty() {
            if let (Some(IpAddr::V4(addr)), Some(IpAddr::V4(mask))) = (settings.address, settings.netmask) {
                let prefix = ipv4_netmask_to_prefix(mask);
                v4_assignments.push(IpAssignment { address: IpAddr::V4(addr), prefix });
            }
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

pub(super) async fn persist_networkd_config(settings: &NetworkInterfaceSettings) -> Result<(), ApiError> {
    let file_name = format!("{PERSIST_NETWORKD_PREFIX}-{}.network", sanitize_iface_name(&settings.name));
    let content = render_networkd_config(settings);

    let persist_dir = PathBuf::from(PERSIST_NETWORKD_DIR);
    create_dir_all_durable(&persist_dir).await?;
    let persist_path = persist_dir.join(&file_name);
    write_atomic(&persist_path, &content).await?;

    let systemd_dir = PathBuf::from("/etc/systemd/network");
    create_dir_all_durable(&systemd_dir).await?;
    let systemd_path = systemd_dir.join(&file_name);
    write_atomic(&systemd_path, &content).await?;

    Ok(())
}

fn sanitize_iface_name(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

fn ipv4_netmask_to_prefix(mask: Ipv4Addr) -> u8 {
    mask.octets().iter().fold(0u32, |acc, &octet| (acc << 8) | octet as u32).count_ones() as u8
}

fn resolve_networkd_name(name: &str) -> String {
    if name == "usb0" && Path::new("/sys/class/net/usbbr0").exists() {
        return "usbbr0".to_string();
    }
    name.to_string()
}

async fn write_atomic(path: &Path, content: &str) -> Result<(), ApiError> {
    let path_buf = path.to_path_buf();
    let path_display = path_buf.display().to_string();
    let content = content.to_owned();
    tokio::task::spawn_blocking(move || write_atomic_durable(&path_buf, content.as_bytes()))
        .await
        .map_err(|err| ApiError::internal(format!("failed to join network config write for {path_display}: {err}")))?
        .map_err(|err| ApiError::internal(format!("failed to persist {path_display}: {err}")))
}

pub(super) fn write_atomic_durable(path: &Path, content: &[u8]) -> std::io::Result<()> {
    use std::fs::{self, OpenOptions};
    use std::io::{Error, ErrorKind, Write};

    let file_name = path.file_name().ok_or_else(|| Error::new(ErrorKind::InvalidInput, "network config path missing filename"))?.to_string_lossy();
    let tmp_path = path.with_file_name(format!("{file_name}.tmp-{}", Uuid::new_v4()));

    let mut tmp = OpenOptions::new().create(true).truncate(true).write(true).open(&tmp_path)?;
    tmp.write_all(content)?;
    tmp.sync_all()?;
    drop(tmp);

    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }

    sync_parent_dir(path)?;
    Ok(())
}

fn sync_parent_dir(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        sync_dir(parent)?;
    }
    Ok(())
}

fn sync_dir(path: &Path) -> std::io::Result<()> {
    let dir = std::fs::File::open(path)?;
    dir.sync_all()
}

async fn create_dir_all_durable(path: &Path) -> Result<(), ApiError> {
    let path_buf = path.to_path_buf();
    let path_display = path_buf.display().to_string();
    tokio::task::spawn_blocking(move || create_dir_all_durable_blocking(&path_buf))
        .await
        .map_err(|err| ApiError::internal(format!("failed to join directory create for {path_display}: {err}")))?
        .map_err(|err| ApiError::internal(format!("failed to create {path_display}: {err}")))
}

fn create_dir_all_durable_blocking(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        return Ok(());
    }

    std::fs::create_dir_all(path)?;
    sync_parent_dir(path)
}
