use std::path::PathBuf;

/// Where the image keeps things (see gaia/configs/README.md, "Read-only root"): the root
/// filesystem is a read-only EROFS slot, `/data` (p7) holds everything kept across updates and
/// is bind-mounted where the services expect it.
#[derive(Debug, Clone)]
pub struct DiagnosticsConfig {
    pub base_dir: PathBuf,
    pub plugin_dir: PathBuf,
    pub orion_run_dir: PathBuf,
    /// The data partition (p7), kept across A/B updates.
    pub data_mount: PathBuf,
    /// HeliOS state, bind-mounted from the data partition.
    pub writable_store_mount: PathBuf,
    pub journal_mount: PathBuf,
    pub machine_id_path: PathBuf,
    pub persistent_machine_id_path: PathBuf,
    /// sshd's host keys (`HostKey` in sshd_config), on the data partition.
    pub ssh_host_key_dir: PathBuf,
    /// The device package updater's state.
    pub update_status_path: PathBuf,
    pub startup_preset_path: PathBuf,
    pub snapshot_tar: bool,
    pub snapshot_keep: usize,
    pub max_cmd_bytes: usize,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self {
            base_dir: PathBuf::from("/var/lib/helios/diagnostics"),
            plugin_dir: PathBuf::from("/usr/lib/helios/plugins/daedalus"),
            orion_run_dir: PathBuf::from("/run/orion"),
            data_mount: PathBuf::from("/data"),
            writable_store_mount: PathBuf::from("/var/lib/helios"),
            journal_mount: PathBuf::from("/var/log/journal"),
            machine_id_path: PathBuf::from("/etc/machine-id"),
            persistent_machine_id_path: PathBuf::from("/data/identity/machine-id"),
            ssh_host_key_dir: PathBuf::from("/data/ssh"),
            update_status_path: PathBuf::from("/run/pd-device/update.json"),
            startup_preset_path: PathBuf::from("/etc/helios/startup.toml"),
            snapshot_tar: true,
            snapshot_keep: 10,
            max_cmd_bytes: 1_000_000,
        }
    }
}
