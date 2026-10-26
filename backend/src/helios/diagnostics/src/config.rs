use std::path::PathBuf;

use helios_layout::{BoardContract, Layout};

/// Service binaries in the layout's bin directory.
const SERVICE_BINARIES: &[&str] = &["orion-node", "orionctl", "helios-engine", "helios-peripherals", "helios-api"];

/// The owner image's units: Orion, HeliOS's services, and the image's own sshd and governor.
const IMAGE_UNITS: &[&str] = &["orion-node.service", "helios-engine.service", "helios-peripherals.service", "helios-api.service", "sshd.service", "helios-set-governor.service"];
/// A guest's units: HeliOS's own (the host runs its own sshd and governor).
const GUEST_UNITS: &[&str] = &["helios-orion.service", "helios-engine.service", "helios-peripherals.service", "helios-api.service"];

/// Where the image keeps things (see gaia/configs/README.md, "Read-only root"): the root
/// filesystem is a read-only EROFS slot, `/data` (p7) holds everything kept across updates and
/// is bind-mounted where the services expect it. On a guest the same roots sit under the guest
/// prefix, and the board's files are absent.
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
    /// The host's release file, shown in the snapshot.
    pub os_release_path: PathBuf,
    pub persistent_machine_id_path: PathBuf,
    /// sshd's host keys (`HostKey` in sshd_config), on the data partition.
    pub ssh_host_key_dir: PathBuf,
    /// The device package's files; `None` on a guest, which has none of its own.
    pub board: Option<BoardContract>,
    pub startup_preset_path: PathBuf,
    /// Service binaries that must be present and linked.
    pub expected_binaries: Vec<PathBuf>,
    /// Helper files that must be present and executable.
    pub expected_executables: Vec<PathBuf>,
    /// The systemd units the health report checks.
    pub services: Vec<String>,
    /// Orion's unit: its health decides the report's status.
    pub orion_unit: String,
    /// Paths (globs allowed) the failure snapshot copies.
    pub snapshot_files: Vec<String>,
    pub snapshot_tar: bool,
    pub snapshot_keep: usize,
    pub max_cmd_bytes: usize,
}

impl DiagnosticsConfig {
    /// The diagnostics of the device whose layout this is.
    pub fn from_layout(layout: &Layout) -> Self {
        let board = layout.board().cloned();
        let (units, orion_unit) = if board.is_some() { (IMAGE_UNITS, "orion-node.service") } else { (GUEST_UNITS, "helios-orion.service") };
        let mut expected_executables = layout.image_helpers();
        let mut snapshot_files = vec![
            layout.os_release_file().display().to_string(),
            layout.image_env_file().display().to_string(),
            layout.hostname_file().display().to_string(),
            layout.team_file().display().to_string(),
            layout.peers_file().display().to_string(),
            layout.lemnos_board_file().display().to_string(),
            format!("{}/*.network", layout.networkd_dir().display()),
        ];
        if let Some(board) = &board {
            expected_executables.push(board.update_health.clone());
            expected_executables.push(board.update_pre_reboot.clone());
            snapshot_files.push(board.identity_file.display().to_string());
            snapshot_files.push(board.update_status.display().to_string());
            snapshot_files.push(board.device_package_env.display().to_string());
            snapshot_files.push(format!("{}/*", board.etc_dir.display()));
            snapshot_files.push(format!("{}/*/*", board.etc_dir.display()));
        }
        Self {
            base_dir: layout.diagnostics_dir(),
            plugin_dir: layout.shipped_plugin_dir(),
            orion_run_dir: layout.orion_run_dir().to_path_buf(),
            data_mount: layout.data_dir().to_path_buf(),
            writable_store_mount: layout.state_dir().to_path_buf(),
            journal_mount: layout.journal_dir(),
            machine_id_path: layout.machine_id_file(),
            os_release_path: layout.os_release_file(),
            persistent_machine_id_path: layout.persistent_machine_id_file(),
            ssh_host_key_dir: layout.ssh_host_key_dir(),
            board,
            startup_preset_path: layout.startup_file(),
            expected_binaries: SERVICE_BINARIES.iter().map(|name| layout.binary(name)).collect(),
            expected_executables,
            services: units.iter().map(|unit| unit.to_string()).collect(),
            orion_unit: orion_unit.to_string(),
            snapshot_files,
            snapshot_tar: true,
            snapshot_keep: 10,
            max_cmd_bytes: 1_000_000,
        }
    }
}

impl Default for DiagnosticsConfig {
    /// The image's diagnostics.
    fn default() -> Self {
        Self::from_layout(&Layout::image())
    }
}
