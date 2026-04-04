use std::path::PathBuf;

use super::super::config::{SensorsClientConfig, push_unique};

#[test]
fn push_unique_deduplicates_socket_candidates() {
    let mut paths = vec![PathBuf::from("/tmp/a.sock")];
    push_unique(&mut paths, PathBuf::from("/tmp/a.sock"));
    push_unique(&mut paths, PathBuf::from("/tmp/b.sock"));
    assert_eq!(paths, vec![PathBuf::from("/tmp/a.sock"), PathBuf::from("/tmp/b.sock")]);
}

#[test]
fn sensors_client_config_new_keeps_socket_and_journal_paths() {
    let config = SensorsClientConfig::new("/tmp/peripherals.sock", "/tmp/peripherals.journal");
    assert_eq!(lib_ipc::client::TransportConfig::socket_path(&config), std::path::Path::new("/tmp/peripherals.sock"));
    assert_eq!(lib_ipc::client::TransportConfig::journal_path(&config), std::path::Path::new("/tmp/peripherals.journal"));
}
