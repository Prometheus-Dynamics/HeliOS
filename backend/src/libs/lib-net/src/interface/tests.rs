use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use super::*;
use rtnetlink::packet_route::link::{BondMode, LinkMessage};

#[test]
fn parse_link_info_extracts_vlan_and_bond() {
    let mut link = LinkMessage::default();
    link.header.index = 10;
    link.attributes.push(LinkAttribute::IfName("eth0.100".into()));
    link.attributes.push(LinkAttribute::Link(2));
    link.attributes.push(LinkAttribute::Address(vec![0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]));
    link.attributes.push(LinkAttribute::LinkInfo(vec![LinkInfo::Kind(InfoKind::Vlan), LinkInfo::Data(InfoData::Vlan(vec![InfoVlan::Id(100)]))]));
    link.attributes.push(LinkAttribute::LinkInfo(vec![LinkInfo::Kind(InfoKind::Bond), LinkInfo::Data(InfoData::Bond(vec![InfoBond::Mode(BondMode::ActiveBackup)]))]));

    let mut settings = NetworkInterfaceSettings { mac: Some(format_mac(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff])), ..NetworkInterfaceSettings::default() };
    parse_link_info_attributes(&link, &mut settings);

    let vlan = settings.vlan.expect("vlan config");
    assert_eq!(vlan.id, 100);
    assert_eq!(vlan.parent.as_deref(), Some("ifindex:2"));

    let bond = settings.bond.expect("bond config");
    assert_eq!(bond.mode.as_deref(), Some("active-backup"));
}

#[test]
fn dns_config_roundtrip() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("resolv.conf");
    super::dns::set_test_dns_path(Some(path.clone()));

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let config = DnsConfig { servers: vec![IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), IpAddr::V6(Ipv6Addr::LOCALHOST)], search: vec!["lan.local".into()] };
        super::dns::apply_dns_config(&config).await.expect("write dns");
        let read = super::dns::read_dns_config().await.expect("read dns");
        assert_eq!(config.servers, read.servers);
        assert_eq!(config.search, read.search);
    });

    super::dns::set_test_dns_path(None);
}

#[test]
fn unaddressed_uplink_interfaces_are_kept() {
    let end0 = NetworkInterfaceSettings { name: "end0".into(), ..NetworkInterfaceSettings::default() };
    assert!(should_include_without_addresses(&end0));

    let usb0 = NetworkInterfaceSettings { name: "usb0".into(), ..NetworkInterfaceSettings::default() };
    assert!(!should_include_without_addresses(&usb0));

    let vlan = NetworkInterfaceSettings { name: "vlan100".into(), vlan: Some(VlanConfig { id: 100, parent: Some("end0".into()) }), ..NetworkInterfaceSettings::default() };
    assert!(should_include_without_addresses(&vlan));
}
