use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct DiagnosticsConfig {
    pub base_dir: PathBuf,
    pub managed_bin_dir: PathBuf,
    pub plugin_dir: PathBuf,
    pub orion_run_dir: PathBuf,
    pub writable_store_mount: PathBuf,
    pub journal_mount: PathBuf,
    pub persistent_journal_dir: PathBuf,
    pub machine_id_path: PathBuf,
    pub persistent_machine_id_path: PathBuf,
    pub ssh_host_key_dir: PathBuf,
    pub persistent_ssh_host_key_dir: PathBuf,
    pub overlay_root: PathBuf,
    pub startup_preset_path: PathBuf,
    pub snapshot_tar: bool,
    pub snapshot_keep: usize,
    pub max_cmd_bytes: usize,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self {
            base_dir: PathBuf::from("/var/lib/helios/diagnostics"),
            managed_bin_dir: PathBuf::from("/var/lib/helios/bin"),
            plugin_dir: PathBuf::from("/usr/lib/helios/plugins/daedalus"),
            orion_run_dir: PathBuf::from("/run/orion"),
            writable_store_mount: PathBuf::from("/var/lib/helios"),
            journal_mount: PathBuf::from("/var/log/journal"),
            persistent_journal_dir: PathBuf::from("/var/lib/helios/journal"),
            machine_id_path: PathBuf::from("/etc/machine-id"),
            persistent_machine_id_path: PathBuf::from("/var/lib/helios/identity/machine-id"),
            ssh_host_key_dir: PathBuf::from("/etc/ssh"),
            persistent_ssh_host_key_dir: PathBuf::from("/var/lib/helios/identity/ssh"),
            overlay_root: PathBuf::from("/var/lib/helios/root-overlay"),
            startup_preset_path: PathBuf::from("/etc/helios/startup.toml"),
            snapshot_tar: true,
            snapshot_keep: 10,
            max_cmd_bytes: 1_000_000,
        }
    }
}
