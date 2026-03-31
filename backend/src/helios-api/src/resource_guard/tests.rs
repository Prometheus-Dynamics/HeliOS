use super::system::parse_mem_available_kb;

#[test]
fn parse_meminfo_available() {
    let data = "MemTotal:       7925412 kB\nMemFree:         288120 kB\nMemAvailable:   1330772 kB\n";
    assert_eq!(parse_mem_available_kb(data), Some(1_330_772));
}

#[test]
fn parse_meminfo_missing_available() {
    let data = "MemTotal: 1024 kB\nMemFree: 123 kB\n";
    assert_eq!(parse_mem_available_kb(data), None);
}
