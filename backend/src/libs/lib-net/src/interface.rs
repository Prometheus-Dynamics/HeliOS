mod dhcp;
mod dns;
#[cfg(test)]
mod tests;

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::Path,
    sync::Arc,
};

use async_trait::async_trait;
use tracing::{info, warn};
use utoipa::ToSchema;

use futures::TryStreamExt;
use rtnetlink::{
    Handle, RouteMessageBuilder, new_connection,
    packet_route::{
        AddressFamily,
        address::AddressAttribute,
        link::{BondMode, InfoBond, InfoData, InfoKind, InfoVlan, LinkAttribute, LinkInfo, LinkLayerType, LinkMessage},
        route::{RouteAddress, RouteAttribute, RouteMessage, RouteProtocol},
    },
};
use serde::{Deserialize, Serialize};
use tokio::task;
use tokio::time::Duration;

use self::dhcp::{start_dhcp_client, stop_dhcp_client};
use self::dns::{apply_dns_config, read_dns_config};

pub use super::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default, PartialEq, Eq)]
pub enum IpMode {
    #[default]
    Dynamic,
    Static,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(default)]
pub struct IpAssignment {
    #[schema(value_type = String, example = "192.168.1.10")]
    pub address: IpAddr,
    #[schema(example = 24)]
    pub prefix: u8,
}

impl Default for IpAssignment {
    fn default() -> Self {
        Self { address: IpAddr::V4(Ipv4Addr::UNSPECIFIED), prefix: 0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default, PartialEq, Eq)]
#[serde(default)]
pub struct DnsConfig {
    #[schema(value_type = Vec<String>, example = json!( ["8.8.8.8", "1.1.1.1"] ))]
    pub servers: Vec<IpAddr>,
    #[schema(example = json!( ["lan.local"] ))]
    pub search: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default, PartialEq, Eq)]
pub struct VlanConfig {
    #[schema(example = 100)]
    pub id: u16,
    #[schema(example = "eth0")]
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default, PartialEq, Eq)]
#[serde(default)]
pub struct BondConfig {
    #[schema(example = "active-backup")]
    pub mode: Option<String>,
    #[schema(example = "bond0")]
    pub master: Option<String>,
    #[schema(example = json!( ["eth1", "eth2"] ))]
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default, PartialEq, Eq)]
#[serde(default)]
pub struct NetworkInterfaceSettings {
    #[schema(example = "eth0")]
    pub name: String,
    #[schema(example = "Static")]
    pub mode: IpMode,
    /// IPv4 address used when static mode is active
    #[serde(default)]
    #[schema(value_type = String, example = "10.63.90.28")]
    pub address: Option<IpAddr>,
    /// Subnet mask associated with `address`
    #[serde(default)]
    #[schema(value_type = String, example = "255.255.255.0")]
    pub netmask: Option<IpAddr>,
    /// Default gateway in static mode
    #[serde(default)]
    #[schema(value_type = String, example = "10.63.90.1")]
    pub gateway: Option<IpAddr>,
    #[serde(default)]
    pub ipv4: Vec<IpAssignment>,
    #[schema(example = "Dynamic")]
    #[serde(default)]
    pub ipv6_mode: IpMode,
    #[serde(default)]
    pub ipv6: Vec<IpAssignment>,
    #[serde(default)]
    #[schema(value_type = String, example = "fe80::1")]
    pub ipv6_gateway: Option<IpAddr>,
    #[serde(default)]
    pub dns: DnsConfig,
    #[serde(default)]
    pub vlan: Option<VlanConfig>,
    #[serde(default)]
    pub bond: Option<BondConfig>,
    #[serde(default)]
    #[schema(example = "aa:bb:cc:dd:ee:ff")]
    pub mac: Option<String>,
    #[serde(default)]
    #[schema(value_type = Vec<String>, example = json!( ["10.63.90.1", "fe80::1"] ))]
    pub gateways: Vec<IpAddr>,
}
impl NetworkInterfaceSettings {
    pub fn new_v4(name: &str, address: Ipv4Addr, netmask: Ipv4Addr, gateway: Ipv4Addr) -> Self {
        let prefix = ipv4_netmask_to_prefix(netmask);
        Self {
            name: name.into(),
            mode: IpMode::Static,
            address: Some(IpAddr::V4(address)),
            netmask: Some(IpAddr::V4(netmask)),
            gateway: Some(IpAddr::V4(gateway)),
            ipv4: vec![IpAssignment { address: IpAddr::V4(address), prefix }],
            gateways: vec![IpAddr::V4(gateway)],
            ..Self::default()
        }
    }
}

#[async_trait]
pub trait NetlinkAdapter: Send + Sync {
    async fn resolve_link(&self, name: &str) -> Result<LinkMessage>;
    async fn remove_addresses(&self, if_index: u32, family: AddressFamily) -> Result<()>;
    async fn add_address(&self, if_index: u32, addr: IpAddr, prefix: u8) -> Result<()>;
    async fn set_gateway(&self, if_index: u32, gateway: IpAddr) -> Result<()>;
    async fn remove_gateway(&self, if_index: u32, family: AddressFamily) -> Result<()>;
}

#[async_trait]
pub trait DhcpManager: Send + Sync {
    async fn start(&self, interface: &str) -> Result<()>;
    async fn stop(&self, interface: &str) -> Result<()>;
}

#[derive(Clone)]
struct RealNetlinkAdapter {
    handle: Handle,
}

impl RealNetlinkAdapter {
    fn new(handle: Handle) -> Self {
        Self { handle }
    }
}

#[derive(Clone, Default)]
struct SystemDhcpManager;

#[async_trait]
impl NetlinkAdapter for RealNetlinkAdapter {
    async fn resolve_link(&self, name: &str) -> Result<LinkMessage> {
        let mut stream = self.handle.link().get().match_name(name.to_string()).execute();
        let next = stream.try_next().await.map_err(|e| Error::FailedToIterateInterface(format!("failed to read link {name}: {e}")))?;
        next.ok_or_else(|| Error::InterfaceNotFound(name.to_string()))
    }

    async fn remove_addresses(&self, if_index: u32, family: AddressFamily) -> Result<()> {
        remove_addresses_from_handle(&self.handle, if_index, family).await
    }

    async fn add_address(&self, if_index: u32, addr: IpAddr, prefix: u8) -> Result<()> {
        assign_static_ip(&self.handle, if_index, addr, prefix).await
    }

    async fn set_gateway(&self, if_index: u32, gateway: IpAddr) -> Result<()> {
        add_default_route(&self.handle, if_index, gateway).await
    }

    async fn remove_gateway(&self, if_index: u32, family: AddressFamily) -> Result<()> {
        remove_default_route(&self.handle, if_index, family).await
    }
}

#[async_trait]
impl DhcpManager for SystemDhcpManager {
    async fn start(&self, interface: &str) -> Result<()> {
        start_dhcp_client(interface).await
    }

    async fn stop(&self, interface: &str) -> Result<()> {
        stop_dhcp_client(interface).await
    }
}

fn ipv4_netmask_to_prefix(mask: Ipv4Addr) -> u8 {
    mask.octets().iter().fold(0u32, |acc, &octet| (acc << 8) | octet as u32).count_ones() as u8
}

fn ipv4_prefix_to_netmask(prefix: u8) -> Ipv4Addr {
    if prefix == 0 {
        return Ipv4Addr::new(0, 0, 0, 0);
    }
    let mask = (!0u32).checked_shl((32 - prefix as u32) & 31).unwrap_or(!0u32);
    Ipv4Addr::from(mask)
}

fn format_mac(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
}

fn resolve_interface_name(name: &str) -> String {
    if name == "usb0" && Path::new("/sys/class/net/usbbr0").exists() {
        return "usbbr0".to_string();
    }
    name.to_string()
}

fn should_include_without_addresses(settings: &NetworkInterfaceSettings) -> bool {
    let name = settings.name.as_str();
    settings.vlan.is_some() || settings.bond.is_some() || name.starts_with("eth") || name.starts_with("en") || name.starts_with("wl") || name.starts_with("ww")
}

async fn retry_async<F, Fut, T>(label: &str, attempts: usize, mut op: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut delay = Duration::from_millis(100);
    let mut last_err: Option<Error> = None;
    for attempt in 1..=attempts {
        match op().await {
            Ok(val) => return Ok(val),
            Err(err) => {
                warn!(attempt, "{label} failed: {err}");
                last_err = Some(err);
                if attempt < attempts {
                    tokio::time::sleep(delay).await;
                    delay = std::cmp::min(delay * 2, Duration::from_secs(1));
                    continue;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| Error::FailedToApplyInterfaceSettings(format!("{label} failed"))))
}

async fn fetch_interface_settings(handle: &Handle, link: &LinkMessage) -> Result<Option<NetworkInterfaceSettings>> {
    let interface_index = link.header.index;

    let interface_name = link
        .attributes
        .iter()
        .find_map(|nla| match nla {
            LinkAttribute::IfName(name) => Some(name.clone()),
            _ => None,
        })
        .unwrap_or_else(|| format!("if{}", link.header.index));

    let mut settings = NetworkInterfaceSettings { name: interface_name.clone(), ..NetworkInterfaceSettings::default() };

    if let Some(mac) = link.attributes.iter().find_map(|nla| match nla {
        LinkAttribute::Address(bytes) => Some(format_mac(bytes)),
        _ => None,
    }) {
        settings.mac = Some(mac);
    }

    parse_link_info_attributes(link, &mut settings);

    let mut address_stream = handle.address().get().set_link_index_filter(interface_index).execute();
    while let Some(addr) = address_stream.try_next().await.map_err(|e| Error::FailedToIterateInterface(format!("failed to read addresses for {interface_name}: {e}")))? {
        let ip = addr.attributes.iter().find_map(|nla| match nla {
            AddressAttribute::Local(ip) | AddressAttribute::Address(ip) => Some(*ip),
            _ => None,
        });

        let Some(ip) = ip else { continue };
        let prefix = addr.header.prefix_len;
        match ip {
            IpAddr::V4(v4) => {
                settings.ipv4.push(IpAssignment { address: IpAddr::V4(v4), prefix });
            }
            IpAddr::V6(v6) => {
                settings.ipv6.push(IpAssignment { address: IpAddr::V6(v6), prefix });
            }
        }
    }

    if settings.ipv4.is_empty() && settings.ipv6.is_empty() && !should_include_without_addresses(&settings) {
        return Ok(None);
    }

    collect_route_information(handle, interface_index, &mut settings).await?;

    if settings.address.is_none()
        && let Some(first) = settings.ipv4.first()
    {
        settings.address = Some(first.address);
        if first.address.is_ipv4() {
            settings.netmask = Some(IpAddr::V4(ipv4_prefix_to_netmask(first.prefix)));
        }
    }
    if settings.gateway.is_none()
        && let Some(gw) = settings.gateways.iter().find(|ip| ip.is_ipv4())
    {
        settings.gateway = Some(*gw);
    }

    if let Some(gw) = settings.gateway
        && !settings.gateways.contains(&gw)
    {
        settings.gateways.push(gw);
    }
    if let Some(gw) = settings.ipv6_gateway
        && !settings.gateways.contains(&gw)
    {
        settings.gateways.push(gw);
    }
    settings.gateways.sort_by_key(|a| a.to_string());
    settings.gateways.dedup();

    Ok(Some(settings))
}

fn parse_link_info_attributes(link: &LinkMessage, settings: &mut NetworkInterfaceSettings) {
    let parent_hint = link
        .attributes
        .iter()
        .find_map(|attr| match attr {
            LinkAttribute::Link(idx) => Some(*idx),
            _ => None,
        })
        .map(|idx| format!("ifindex:{idx}"));

    for attr in &link.attributes {
        if let LinkAttribute::LinkInfo(infos) = attr {
            let mut current_kind: Option<InfoKind> = None;
            for info in infos {
                match info {
                    LinkInfo::Kind(kind) => current_kind = Some(kind.clone()),
                    LinkInfo::Data(data) => match (&current_kind, data) {
                        (Some(InfoKind::Vlan), InfoData::Vlan(items)) => {
                            let mut vlan_config = settings.vlan.clone().unwrap_or_default();
                            for item in items {
                                if let InfoVlan::Id(id) = item {
                                    vlan_config.id = *id;
                                }
                            }
                            if vlan_config.id != 0 {
                                if vlan_config.parent.is_none() {
                                    vlan_config.parent = parent_hint.clone();
                                }
                                settings.vlan = Some(vlan_config);
                            }
                        }
                        (Some(InfoKind::Bond), InfoData::Bond(items)) => {
                            let mut bond_config = settings.bond.clone().unwrap_or_default();
                            for item in items {
                                if let InfoBond::Mode(mode) = item {
                                    let mode_str = match *mode {
                                        BondMode::BalanceRr => "balance-rr".to_string(),
                                        BondMode::ActiveBackup => "active-backup".to_string(),
                                        BondMode::BalanceXor => "balance-xor".to_string(),
                                        BondMode::Broadcast => "broadcast".to_string(),
                                        BondMode::Ieee8023Ad => "802.3ad".to_string(),
                                        BondMode::BalanceTlb => "balance-tlb".to_string(),
                                        BondMode::BalanceAlb => "balance-alb".to_string(),
                                        BondMode::Other(code) => format!("other-{code}"),
                                        _ => "unknown".to_string(),
                                    };
                                    bond_config.mode = Some(mode_str);
                                }
                            }
                            settings.bond = Some(bond_config);
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
    }
}

async fn collect_route_information(handle: &Handle, interface_index: u32, settings: &mut NetworkInterfaceSettings) -> Result<()> {
    let ipv4_routes: Vec<RouteMessage> =
        handle.route().get(RouteMessageBuilder::<Ipv4Addr>::new().build()).execute().try_collect().await.map_err(|e| Error::NetlinkOperationFailed(format!("failed to collect ipv4 routes: {e}")))?;

    for route in ipv4_routes.iter().filter(|route| route.attributes.iter().any(|a| matches!(a, RouteAttribute::Oif(idx) if *idx == interface_index))) {
        if route.header.destination_prefix_length == 0 {
            for attr in &route.attributes {
                if let RouteAttribute::Gateway(RouteAddress::Inet(gw)) = attr {
                    settings.gateways.push(IpAddr::V4(*gw));
                    settings.gateway = Some(IpAddr::V4(*gw));
                }
            }

            settings.mode = match route.header.protocol {
                RouteProtocol::Static => IpMode::Static,
                RouteProtocol::Dhcp => IpMode::Dynamic,
                _ => settings.mode.clone(),
            };
        }
    }

    let ipv6_routes: Vec<RouteMessage> =
        handle.route().get(RouteMessageBuilder::<Ipv6Addr>::new().build()).execute().try_collect().await.map_err(|e| Error::NetlinkOperationFailed(format!("failed to collect ipv6 routes: {e}")))?;

    for route in ipv6_routes.iter().filter(|route| route.attributes.iter().any(|a| matches!(a, RouteAttribute::Oif(idx) if *idx == interface_index))) {
        if route.header.destination_prefix_length == 0 {
            for attr in &route.attributes {
                if let RouteAttribute::Gateway(RouteAddress::Inet6(gw)) = attr {
                    settings.gateways.push(IpAddr::V6(*gw));
                    settings.ipv6_gateway = Some(IpAddr::V6(*gw));
                }
            }

            settings.ipv6_mode = match route.header.protocol {
                RouteProtocol::Static => IpMode::Static,
                RouteProtocol::Dhcp => IpMode::Dynamic,
                _ => settings.ipv6_mode.clone(),
            };
        }
    }

    Ok(())
}
pub async fn get_interfaces() -> Result<Vec<NetworkInterfaceSettings>> {
    let (connection, handle, _) = new_connection().map_err(|e| Error::NetlinkConnectionFailed(e.to_string()))?;
    task::spawn(connection);

    let dns = read_dns_config().await.unwrap_or_default();

    let mut interfaces = Vec::new();
    let mut name_by_index = HashMap::new();

    let mut links = handle.link().get().execute();

    while let Some(link) = links.try_next().await.map_err(|e| Error::FailedToIterateInterface(format!("failed to iterate links: {e}")))? {
        if link.header.link_layer_type == LinkLayerType::Loopback {
            continue;
        }

        if let Some(name) = link.attributes.iter().find_map(|attr| match attr {
            LinkAttribute::IfName(name) => Some(name.clone()),
            _ => None,
        }) {
            name_by_index.insert(link.header.index, name);
        }

        if let Some(mut settings) = fetch_interface_settings(&handle, &link).await? {
            settings.dns = dns.clone();
            interfaces.push(settings);
        }
    }

    for iface in interfaces.iter_mut() {
        if let Some(ref mut vlan) = iface.vlan
            && let Some(mapped) = vlan.parent.clone().and_then(|parent| parent.strip_prefix("ifindex:").and_then(|s| s.parse::<u32>().ok())).and_then(|idx| name_by_index.get(&idx).cloned())
        {
            vlan.parent = Some(mapped);
        }
    }

    Ok(interfaces)
}
pub async fn get_interface(name: &str) -> Result<NetworkInterfaceSettings> {
    let interfaces = get_interfaces().await?;
    interfaces.into_iter().find(|iface| iface.name == name).ok_or_else(|| Error::InterfaceNotFound(name.to_string()))
}

/// Return all IP addresses of non-loopback interfaces on this host.
pub async fn get_local_addresses() -> Result<Vec<IpAddr>> {
    let interfaces = get_interfaces().await?;
    let mut addresses = Vec::new();
    for iface in interfaces {
        addresses.extend(iface.ipv4.into_iter().map(|ip| ip.address));
        addresses.extend(iface.ipv6.into_iter().map(|ip| ip.address));
    }
    Ok(addresses)
}

pub async fn set_interface(settings: &NetworkInterfaceSettings) -> Result<()> {
    let (connection, handle, _) = new_connection().map_err(|e| Error::NetlinkConnectionFailed(e.to_string()))?;
    task::spawn(connection);

    let netlink = Arc::new(RealNetlinkAdapter::new(handle));
    let dhcp = Arc::new(SystemDhcpManager);

    set_interface_with_providers(settings, netlink, dhcp).await
}

pub async fn set_interface_with_providers(settings: &NetworkInterfaceSettings, netlink: Arc<dyn NetlinkAdapter>, dhcp: Arc<dyn DhcpManager>) -> Result<()> {
    let netlink_for_lookup = netlink.clone();
    let name = resolve_interface_name(&settings.name);
    let name_for_lookup = name.clone();
    if name != settings.name {
        info!(requested = %settings.name, resolved = %name, "remapping interface settings to USB gadget bridge");
    }

    let link = retry_async("resolve interface", 3, move || {
        let adapter = netlink_for_lookup.clone();
        let name = name_for_lookup.clone();
        async move { adapter.resolve_link(&name).await }
    })
    .await?;

    let interface_index = link.header.index;

    let ipv4_assignments = if !settings.ipv4.is_empty() {
        settings.ipv4.clone()
    } else if let Some(addr) = settings.address {
        if !addr.is_ipv4() {
            return Err(Error::UnsupportedIpVersion("Expected IPv4 address for IPv4 configuration".into()));
        }
        let prefix = match settings.netmask {
            Some(IpAddr::V4(mask)) => ipv4_netmask_to_prefix(mask),
            Some(IpAddr::V6(_)) => {
                return Err(Error::FailedToApplyInterfaceSettings("IPv4 netmask must be an IPv4 address".into()));
            }
            None => {
                return Err(Error::FailedToApplyInterfaceSettings("Static IPv4 mode requires a netmask".into()));
            }
        };
        vec![IpAssignment { address: addr, prefix }]
    } else {
        Vec::new()
    };

    let ipv4_gateway = settings.gateways.iter().cloned().find(|ip| ip.is_ipv4()).or(settings.gateway).filter(|ip| ip.is_ipv4());

    match settings.mode {
        IpMode::Static => {
            if ipv4_assignments.is_empty() {
                return Err(Error::FailedToApplyInterfaceSettings("Static IPv4 mode requires at least one address".into()));
            }
            netlink.remove_addresses(interface_index, AddressFamily::Inet).await?;
            for assignment in &ipv4_assignments {
                netlink.add_address(interface_index, assignment.address, assignment.prefix).await?;
            }
            if let Some(gateway) = ipv4_gateway {
                netlink.set_gateway(interface_index, gateway).await?;
            } else {
                netlink.remove_gateway(interface_index, AddressFamily::Inet).await?;
            }
            dhcp.stop(&name).await?;
        }
        IpMode::Dynamic => {
            netlink.remove_addresses(interface_index, AddressFamily::Inet).await?;
            netlink.remove_gateway(interface_index, AddressFamily::Inet).await?;
            dhcp.start(&name).await?;
        }
    }

    let ipv6_assignments = if !settings.ipv6.is_empty() { settings.ipv6.clone() } else { Vec::new() };

    let ipv6_gateway = settings.ipv6_gateway.or_else(|| settings.gateways.iter().cloned().find(|ip| ip.is_ipv6()));

    match settings.ipv6_mode {
        IpMode::Static => {
            if !ipv6_assignments.is_empty() {
                netlink.remove_addresses(interface_index, AddressFamily::Inet6).await?;
                for assignment in &ipv6_assignments {
                    netlink.add_address(interface_index, assignment.address, assignment.prefix).await?;
                }
                if let Some(gateway) = ipv6_gateway {
                    netlink.set_gateway(interface_index, gateway).await?;
                }
            }
        }
        IpMode::Dynamic => {
            netlink.remove_addresses(interface_index, AddressFamily::Inet6).await?;
            netlink.remove_gateway(interface_index, AddressFamily::Inet6).await?;
        }
    }

    apply_dns_config(&settings.dns).await?;

    Ok(())
}

async fn add_default_route(handle: &Handle, if_index: u32, gateway: IpAddr) -> Result<()> {
    match gateway {
        IpAddr::V4(addr) => {
            let route = RouteMessageBuilder::<Ipv4Addr>::new().destination_prefix(Ipv4Addr::new(0, 0, 0, 0), 0).gateway(addr).output_interface(if_index).build();
            handle.route().add(route).execute().await.map_err(|e| Error::NetlinkOperationFailed(e.to_string()))?
        }
        IpAddr::V6(addr) => {
            let route = RouteMessageBuilder::<Ipv6Addr>::new().destination_prefix(Ipv6Addr::UNSPECIFIED, 0).gateway(addr).output_interface(if_index).build();
            handle.route().add(route).execute().await.map_err(|e| Error::NetlinkOperationFailed(e.to_string()))?
        }
    }
    Ok(())
}

async fn remove_addresses_from_handle(handle: &Handle, if_index: u32, family: AddressFamily) -> Result<()> {
    let mut addresses = handle.address().get().set_link_index_filter(if_index).execute();
    while let Some(addr) = addresses.try_next().await.map_err(|e| Error::NetlinkOperationFailed(format!("failed to read addresses: {e}")))? {
        if addr.header.family != family {
            continue;
        }
        handle.address().del(addr).execute().await.map_err(|e| Error::NetlinkOperationFailed(format!("failed to remove address: {e}")))?;
    }
    Ok(())
}

async fn assign_static_ip(handle: &Handle, if_index: u32, ip: IpAddr, prefix_len: u8) -> Result<()> {
    handle.address().add(if_index, ip, prefix_len).execute().await.map_err(|e| Error::NetlinkOperationFailed(format!("failed to assign address: {e}")))?;
    Ok(())
}

async fn remove_default_route(handle: &Handle, if_index: u32, family: AddressFamily) -> Result<()> {
    match family {
        AddressFamily::Inet => {
            let routes: Vec<RouteMessage> = handle
                .route()
                .get(RouteMessageBuilder::<Ipv4Addr>::new().build())
                .execute()
                .try_collect()
                .await
                .map_err(|e| Error::NetlinkOperationFailed(format!("failed to collect ipv4 routes: {e}")))?;
            for route in routes.into_iter().filter(|route| route.attributes.iter().any(|a| matches!(a, RouteAttribute::Oif(idx) if *idx == if_index))) {
                if route.header.destination_prefix_length == 0 {
                    handle.route().del(route).execute().await.map_err(|e| Error::NetlinkOperationFailed(format!("failed to delete route: {e}")))?;
                }
            }
        }
        AddressFamily::Inet6 => {
            let routes: Vec<RouteMessage> = handle
                .route()
                .get(RouteMessageBuilder::<Ipv6Addr>::new().build())
                .execute()
                .try_collect()
                .await
                .map_err(|e| Error::NetlinkOperationFailed(format!("failed to collect ipv6 routes: {e}")))?;
            for route in routes.into_iter().filter(|route| route.attributes.iter().any(|a| matches!(a, RouteAttribute::Oif(idx) if *idx == if_index))) {
                if route.header.destination_prefix_length == 0 {
                    handle.route().del(route).execute().await.map_err(|e| Error::NetlinkOperationFailed(format!("failed to delete route: {e}")))?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
