use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use lib_runtime_policy::HELIOS_PERIPHERALS_USB_PROXY_POLICY;
use tokio::fs;
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::time::{interval, timeout};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
struct UsbProxyConfig {
    enabled: bool,
    usb_subnet_cidr: String,
    usb_bridge_interface: String,
    helios_usb_vendor_id: u16,
    helios_usb_product_id: u16,
    parent_gateway_ip: Ipv4Addr,
    parent_api_port: u16,
    force_default_route: bool,
}

impl UsbProxyConfig {
    fn from_env() -> Self {
        let policy = HELIOS_PERIPHERALS_USB_PROXY_POLICY.resolve();
        Self {
            enabled: policy.enabled,
            usb_subnet_cidr: policy.subnet,
            usb_bridge_interface: policy.bridge_interface,
            helios_usb_vendor_id: policy.vendor_id,
            helios_usb_product_id: policy.product_id,
            parent_gateway_ip: policy.gateway_ip,
            parent_api_port: policy.parent_api_port,
            force_default_route: policy.force_default_route,
        }
    }
}

pub(crate) fn spawn(shutdown: CancellationToken) {
    let cfg = UsbProxyConfig::from_env();
    if !cfg.enabled {
        info!("usb proxy disabled");
        return;
    }

    tokio::spawn(run_parent_proxy_loop(cfg.clone(), shutdown.child_token()));
    tokio::spawn(run_client_route_loop(cfg, shutdown));
}

async fn run_parent_proxy_loop(cfg: UsbProxyConfig, shutdown: CancellationToken) {
    let mut tick = interval(Duration::from_secs(5));
    let mut networkd_masquerade_uplink: Option<String> = None;
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = tick.tick() => {}
        }

        let Some(uplink_iface) = default_route_interface().await else {
            debug!("usb proxy: no default route (no uplink), skipping NAT");
            continue;
        };

        let Ok(peer_ifaces) = find_helios_usb_peer_interfaces(cfg.helios_usb_vendor_id, cfg.helios_usb_product_id).await else {
            continue;
        };
        if peer_ifaces.is_empty() {
            if networkd_masquerade_uplink.take().is_some()
                && let Err(err) = disable_networkd_masquerade().await
            {
                warn!(%err, "usb proxy: failed to disable networkd masquerade");
            }
            continue;
        }

        if let Err(err) = ensure_ipv4_forwarding_enabled().await {
            warn!(%err, "usb proxy: failed to enable ipv4 forwarding");
            continue;
        }

        if command_available("iptables").await {
            for peer_iface in peer_ifaces {
                if let Err(err) = ensure_iptables_nat_rules(&cfg.usb_subnet_cidr, &peer_iface, &uplink_iface).await {
                    warn!(%err, peer_iface, uplink_iface, "usb proxy: failed to apply iptables NAT rules");
                    continue;
                }
                debug!(peer_iface, uplink_iface, "usb proxy: iptables NAT rules ensured");
            }
        } else if networkd_masquerade_uplink.as_deref() != Some(uplink_iface.as_str()) {
            if let Err(err) = enable_networkd_masquerade(&uplink_iface).await {
                warn!(%err, uplink_iface, "usb proxy: failed to enable networkd masquerade");
                continue;
            }
            networkd_masquerade_uplink = Some(uplink_iface.clone());
            info!(uplink_iface, "usb proxy: enabled networkd masquerade on uplink");
        }
    }
}

async fn run_client_route_loop(cfg: UsbProxyConfig, shutdown: CancellationToken) {
    let mut tick = interval(Duration::from_secs(5));
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = tick.tick() => {}
        }

        if !interface_has_carrier(&cfg.usb_bridge_interface).await.unwrap_or(false) {
            continue;
        }

        if !probe_parent_api(cfg.parent_gateway_ip, cfg.parent_api_port).await {
            continue;
        }

        if let Err(err) = ensure_default_route_via_usb(&cfg.usb_bridge_interface, cfg.parent_gateway_ip, cfg.force_default_route).await {
            warn!(%err, "usb proxy: failed to ensure client default route");
        }
    }
}

async fn probe_parent_api(parent_ip: Ipv4Addr, port: u16) -> bool {
    let addr = SocketAddr::new(IpAddr::V4(parent_ip), port);
    timeout(Duration::from_millis(750), TcpStream::connect(addr)).await.is_ok_and(|res| res.is_ok())
}

async fn ensure_default_route_via_usb(usb_iface: &str, gateway: Ipv4Addr, force: bool) -> Result<(), String> {
    let current = default_route_interface().await;
    if current.is_some_and(|dev| dev != usb_iface) && !force {
        return Ok(());
    }

    run_ok("ip", &["-4", "route", "replace", "default", "via", &gateway.to_string(), "dev", usb_iface, "metric", "50"]).await.map_err(|e| format!("ip route replace failed: {e}"))?;

    info!(usb_iface, gateway = %gateway, "usb proxy: installed default route via parent");
    Ok(())
}

async fn ensure_iptables_nat_rules(usb_subnet: &str, usb_iface: &str, uplink_iface: &str) -> Result<(), String> {
    iptables_ensure(&["-t", "nat"], &["-C", "POSTROUTING", "-s", usb_subnet, "-o", uplink_iface, "-j", "MASQUERADE"], &["-A", "POSTROUTING", "-s", usb_subnet, "-o", uplink_iface, "-j", "MASQUERADE"])
        .await?;

    iptables_ensure(
        &[],
        &["-C", "FORWARD", "-i", uplink_iface, "-o", usb_iface, "-m", "state", "--state", "RELATED,ESTABLISHED", "-j", "ACCEPT"],
        &["-A", "FORWARD", "-i", uplink_iface, "-o", usb_iface, "-m", "state", "--state", "RELATED,ESTABLISHED", "-j", "ACCEPT"],
    )
    .await?;

    iptables_ensure(&[], &["-C", "FORWARD", "-i", usb_iface, "-o", uplink_iface, "-j", "ACCEPT"], &["-A", "FORWARD", "-i", usb_iface, "-o", uplink_iface, "-j", "ACCEPT"]).await?;

    Ok(())
}

async fn enable_networkd_masquerade(uplink_iface: &str) -> Result<(), String> {
    fs::create_dir_all("/run/systemd/network").await.map_err(|e| format!("failed to create /run/systemd/network: {e}"))?;
    let path = Path::new("/run/systemd/network/90-helios-usb-proxy-uplink.network");
    let content = format!("[Match]\nName={uplink_iface}\n\n[Network]\nIPMasquerade=ipv4\nIPForward=ipv4\n");
    if fs::read_to_string(path).await.ok().as_deref() != Some(content.as_str()) {
        fs::write(path, content).await.map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    }

    if command_available("networkctl").await {
        let _ = run_ok("networkctl", &["reload"]).await;
        let _ = run_ok("networkctl", &["reconfigure", uplink_iface]).await;
    }
    Ok(())
}

async fn disable_networkd_masquerade() -> Result<(), String> {
    let path = Path::new("/run/systemd/network/90-helios-usb-proxy-uplink.network");
    if fs::metadata(path).await.is_ok() {
        fs::remove_file(path).await.map_err(|e| format!("failed to remove {}: {e}", path.display()))?;
        if command_available("networkctl").await {
            let _ = run_ok("networkctl", &["reload"]).await;
        }
    }
    Ok(())
}

async fn iptables_ensure(prefix: &[&str], check_args: &[&str], add_args: &[&str]) -> Result<(), String> {
    let mut check: Vec<&str> = Vec::with_capacity(1 + prefix.len() + check_args.len());
    check.extend_from_slice(prefix);
    check.extend_from_slice(check_args);

    let check_ok = run_status("iptables", &check).await.map_err(|e| format!("iptables check failed: {e}"))?;
    if check_ok {
        return Ok(());
    }

    let mut add: Vec<&str> = Vec::with_capacity(1 + prefix.len() + add_args.len());
    add.extend_from_slice(prefix);
    add.extend_from_slice(add_args);
    run_ok("iptables", &add).await.map_err(|e| format!("iptables add failed: {e}"))?;
    Ok(())
}

async fn ensure_ipv4_forwarding_enabled() -> Result<(), String> {
    if !command_available("sysctl").await {
        return Err("sysctl not found".into());
    }

    run_ok("sysctl", &["-w", "net.ipv4.ip_forward=1"]).await.map_err(|e| format!("sysctl failed: {e}"))?;
    Ok(())
}

async fn default_route_interface() -> Option<String> {
    let output = run_capture("ip", &["-4", "route", "show", "default"]).await.ok()?;
    parse_ip_route_default_dev(&output)
}

fn parse_ip_route_default_dev(output: &str) -> Option<String> {
    // Example: "default via 192.168.0.1 dev end0 proto dhcp src 192.168.0.124 metric 202\n"
    let mut tokens = output.split_whitespace();
    while let Some(tok) = tokens.next() {
        if tok == "dev" {
            return tokens.next().map(str::to_string);
        }
    }
    None
}

async fn find_helios_usb_peer_interfaces(vendor: u16, product: u16) -> Result<Vec<String>, std::io::Error> {
    let mut matches = Vec::new();
    let mut entries = fs::read_dir("/sys/class/net").await?;
    while let Some(entry) = entries.next_entry().await? {
        let Some(iface) = entry.file_name().to_str().map(str::to_owned) else { continue };
        if iface == "lo" {
            continue;
        }
        if !interface_has_carrier(&iface).await.unwrap_or(false) {
            continue;
        }
        if usb_device_matches(&iface, vendor, product).await.unwrap_or(false) {
            matches.push(iface);
        }
    }
    Ok(matches)
}

async fn usb_device_matches(iface: &str, vendor: u16, product: u16) -> Result<bool, std::io::Error> {
    let base = PathBuf::from("/sys/class/net").join(iface).join("device");
    let vendor_path = base.join("idVendor");
    let product_path = base.join("idProduct");
    if !(vendor_path.exists() && product_path.exists()) {
        return Ok(false);
    }
    let Some(read_vendor) = read_hex_u16(&vendor_path).await else {
        return Ok(false);
    };
    let Some(read_product) = read_hex_u16(&product_path).await else {
        return Ok(false);
    };
    Ok(read_vendor == vendor && read_product == product)
}

async fn interface_has_carrier(interface: &str) -> Result<bool, std::io::Error> {
    let carrier_path = PathBuf::from("/sys/class/net").join(interface).join("carrier");
    if carrier_path.exists() {
        let contents = fs::read_to_string(carrier_path).await?;
        return Ok(contents.trim() == "1");
    }
    let operstate_path = PathBuf::from("/sys/class/net").join(interface).join("operstate");
    if operstate_path.exists() {
        let contents = fs::read_to_string(operstate_path).await?;
        return Ok(matches!(contents.trim(), "up" | "unknown"));
    }
    Ok(false)
}

async fn read_hex_u16(path: &Path) -> Option<u16> {
    let raw = fs::read_to_string(path).await.ok()?;
    parse_hex_u16(raw.trim())
}

fn parse_hex_u16(input: &str) -> Option<u16> {
    let trimmed = input.trim();
    let stripped = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    u16::from_str_radix(stripped, 16).ok()
}

async fn command_available(name: &str) -> bool {
    if name.contains('/') {
        return tokio::fs::metadata(name).await.is_ok();
    }

    let path = std::env::var_os("PATH").unwrap_or_default();
    for entry in std::env::split_paths(&path) {
        let candidate = entry.join(name);
        if tokio::fs::metadata(&candidate).await.is_ok() {
            return true;
        }
    }
    false
}

async fn run_status(program: &str, args: &[&str]) -> Result<bool, String> {
    let status = Command::new(program).args(args).status().await.map_err(|e| e.to_string())?;
    Ok(status.success())
}

async fn run_ok(program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program).args(args).status().await.map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err(format!("{program} exited with status {status}")) }
}

async fn run_capture(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program).args(args).output().await.map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!("{program} exited with status {}", output.status));
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_route_device() {
        let out = "default via 192.168.0.1 dev end0 proto dhcp src 192.168.0.124 metric 202\n";
        assert_eq!(parse_ip_route_default_dev(out).as_deref(), Some("end0"));
    }

    #[test]
    fn parses_hex_u16_with_prefix() {
        assert_eq!(parse_hex_u16("0x1209"), Some(0x1209));
        assert_eq!(parse_hex_u16("1209"), Some(0x1209));
        assert_eq!(parse_hex_u16("f001"), Some(0xF001));
    }
}
