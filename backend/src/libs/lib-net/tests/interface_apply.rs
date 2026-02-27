use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use lib_net::interface::{DhcpManager, IpAssignment, IpMode, NetlinkAdapter, NetworkInterfaceSettings, set_interface_with_providers};
use lib_net::{Error, Result};
use netlink_packet_route::{AddressFamily, link::LinkMessage};

#[derive(Default)]
struct RecordingNetlink {
    interfaces: Mutex<HashMap<String, u32>>,
    operations: Mutex<Vec<Operation>>,
}

impl RecordingNetlink {
    fn with_interface(name: &str, index: u32) -> Self {
        let mut map = HashMap::new();
        map.insert(name.to_string(), index);
        Self { interfaces: Mutex::new(map), operations: Mutex::new(Vec::new()) }
    }

    fn record(&self, op: Operation) {
        self.operations.lock().unwrap().push(op);
    }

    fn operations(&self) -> Vec<Operation> {
        self.operations.lock().unwrap().clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Operation {
    RemoveAddresses { index: u32, family: AddressFamily },
    AddAddress { index: u32, addr: IpAddr, prefix: u8 },
    SetGateway { index: u32, gateway: IpAddr },
    RemoveGateway { index: u32, family: AddressFamily },
}

#[async_trait]
impl NetlinkAdapter for RecordingNetlink {
    async fn resolve_link(&self, name: &str) -> Result<LinkMessage> {
        let interfaces = self.interfaces.lock().unwrap();
        let Some(&index) = interfaces.get(name) else {
            return Err(Error::InterfaceNotFound(name.to_string()));
        };
        let mut link = LinkMessage::default();
        link.header.index = index;
        Ok(link)
    }

    async fn remove_addresses(&self, if_index: u32, family: AddressFamily) -> Result<()> {
        self.record(Operation::RemoveAddresses { index: if_index, family });
        Ok(())
    }

    async fn add_address(&self, if_index: u32, addr: IpAddr, prefix: u8) -> Result<()> {
        self.record(Operation::AddAddress { index: if_index, addr, prefix });
        Ok(())
    }

    async fn set_gateway(&self, if_index: u32, gateway: IpAddr) -> Result<()> {
        self.record(Operation::SetGateway { index: if_index, gateway });
        Ok(())
    }

    async fn remove_gateway(&self, if_index: u32, family: AddressFamily) -> Result<()> {
        self.record(Operation::RemoveGateway { index: if_index, family });
        Ok(())
    }
}

#[derive(Default)]
struct RecordingDhcp {
    events: Mutex<Vec<DhcpAction>>,
}

impl RecordingDhcp {
    fn events(&self) -> Vec<DhcpAction> {
        self.events.lock().unwrap().clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DhcpAction {
    Start(String),
    Stop(String),
}

#[async_trait]
impl DhcpManager for RecordingDhcp {
    async fn start(&self, interface: &str) -> Result<()> {
        self.events.lock().unwrap().push(DhcpAction::Start(interface.to_string()));
        Ok(())
    }

    async fn stop(&self, interface: &str) -> Result<()> {
        self.events.lock().unwrap().push(DhcpAction::Stop(interface.to_string()));
        Ok(())
    }
}

#[test]
fn static_ipv4_configuration_records_expected_operations() {
    let netlink = Arc::new(RecordingNetlink::with_interface("eth0", 7));
    let dhcp = Arc::new(RecordingDhcp::default());

    let settings = NetworkInterfaceSettings {
        name: "eth0".into(),
        mode: IpMode::Static,
        ipv6_mode: IpMode::Static,
        ipv4: vec![IpAssignment { address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)), prefix: 24 }],
        gateways: vec![IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))],
        ..NetworkInterfaceSettings::default()
    };

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(set_interface_with_providers(&settings, netlink.clone(), dhcp.clone())).expect("apply settings");

    let operations = netlink.operations();
    assert_eq!(
        operations,
        vec![
            Operation::RemoveAddresses { index: 7, family: AddressFamily::Inet },
            Operation::AddAddress { index: 7, addr: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)), prefix: 24 },
            Operation::SetGateway { index: 7, gateway: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)) },
        ],
    );

    let events = dhcp.events();
    assert_eq!(events, vec![DhcpAction::Stop(String::from("eth0"))]);
}

#[test]
fn dynamic_ipv4_configuration_triggers_dhcp_start_and_flushes_routes() {
    let netlink = Arc::new(RecordingNetlink::with_interface("wlan0", 11));
    let dhcp = Arc::new(RecordingDhcp::default());

    let settings = NetworkInterfaceSettings { name: "wlan0".into(), mode: IpMode::Dynamic, ipv6_mode: IpMode::Dynamic, ..NetworkInterfaceSettings::default() };

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(set_interface_with_providers(&settings, netlink.clone(), dhcp.clone())).expect("apply settings");

    let operations = netlink.operations();
    assert_eq!(
        operations,
        vec![
            Operation::RemoveAddresses { index: 11, family: AddressFamily::Inet },
            Operation::RemoveGateway { index: 11, family: AddressFamily::Inet },
            Operation::RemoveAddresses { index: 11, family: AddressFamily::Inet6 },
            Operation::RemoveGateway { index: 11, family: AddressFamily::Inet6 },
        ],
    );

    let events = dhcp.events();
    assert_eq!(events, vec![DhcpAction::Start(String::from("wlan0"))]);
}
