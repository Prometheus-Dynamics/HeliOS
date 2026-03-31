use std::net::{IpAddr, Ipv4Addr};

use lib_net::interface::{DnsConfig, IpAssignment, IpMode, NetworkInterfaceSettings};

use super::config::{render_networkd_config, write_atomic_durable};

#[test]
fn render_networkd_config_static_ipv4_uses_assignments_gateway_and_dns() {
    let settings = NetworkInterfaceSettings {
        name: "eth0".into(),
        mode: IpMode::Static,
        ipv6_mode: IpMode::Dynamic,
        ipv4: vec![IpAssignment { address: IpAddr::V4(Ipv4Addr::new(10, 12, 34, 56)), prefix: 24 }],
        gateways: vec![IpAddr::V4(Ipv4Addr::new(10, 12, 34, 1))],
        dns: DnsConfig { servers: vec![IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))], search: vec!["lan.local".into()] },
        ..NetworkInterfaceSettings::default()
    };

    let rendered = render_networkd_config(&settings);
    assert!(rendered.contains("Name=eth0\n"));
    assert!(rendered.contains("Address=10.12.34.56/24\n"));
    assert!(rendered.contains("Gateway=10.12.34.1\n"));
    assert!(rendered.contains("DNS=1.1.1.1\n"));
    assert!(rendered.contains("Domains=lan.local\n"));
    assert!(rendered.contains("DHCP=ipv6\n"));
}

#[test]
fn write_atomic_durable_replaces_existing_content_without_temp_files() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("eth0.network");

    write_atomic_durable(&path, b"first").expect("write first version");
    write_atomic_durable(&path, b"second").expect("write second version");

    let written = std::fs::read_to_string(&path).expect("read final file");
    assert_eq!(written, "second");

    let leftovers = std::fs::read_dir(dir.path()).expect("list dir").filter_map(|entry| entry.ok()).filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-")).count();
    assert_eq!(leftovers, 0);
}
