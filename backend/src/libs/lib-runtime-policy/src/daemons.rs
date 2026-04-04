use crate::{BoolPolicy, BoundedU64Policy, BoundedUsizePolicy, OptionalBoundedU64Policy, OptionalStringPolicy, PathPolicy, StringPolicy};
use std::path::PathBuf;
use std::time::Duration;

fn resolve_csv_list(raw: String) -> Vec<String> {
    raw.split(',').map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootButtonDaemonPolicy {
    pub keys: StringPolicy,
    pub hold_secs: BoundedU64Policy,
    pub cooldown_secs: BoundedU64Policy,
    pub device_hint: OptionalStringPolicy,
    pub reset_command: StringPolicy,
    pub discovery_retry_ms: BoundedU64Policy,
    pub idle_poll_ms: BoundedU64Policy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedBootButtonDaemonPolicy {
    pub key_tokens: Vec<String>,
    pub hold_duration: Duration,
    pub cooldown: Duration,
    pub device_hint: Option<String>,
    pub reset_command: String,
    pub discovery_retry: Duration,
    pub idle_poll: Duration,
}

impl BootButtonDaemonPolicy {
    pub fn resolve(self) -> ResolvedBootButtonDaemonPolicy {
        let key_tokens = resolve_csv_list(self.keys.resolve());
        ResolvedBootButtonDaemonPolicy {
            key_tokens,
            hold_duration: Duration::from_secs(self.hold_secs.resolve()),
            cooldown: Duration::from_secs(self.cooldown_secs.resolve()),
            device_hint: self.device_hint.resolve(),
            reset_command: self.reset_command.resolve(),
            discovery_retry: Duration::from_millis(self.discovery_retry_ms.resolve()),
            idle_poll: Duration::from_millis(self.idle_poll_ms.resolve()),
        }
    }
}

pub const HELIOS_BOOT_BUTTON_DAEMON_POLICY: BootButtonDaemonPolicy = BootButtonDaemonPolicy {
    keys: StringPolicy { env_var: "HELIOS_BOOT_BUTTON_KEYS", default: "KEY_RESTART,KEY_CONFIG,KEY_POWER" },
    hold_secs: BoundedU64Policy { env_var: "HELIOS_BOOT_BUTTON_HOLD_SECS", default: 5, min: 1, max: 300 },
    cooldown_secs: BoundedU64Policy { env_var: "HELIOS_BOOT_BUTTON_COOLDOWN_SECS", default: 15, min: 0, max: 3600 },
    device_hint: OptionalStringPolicy { env_var: "HELIOS_BOOT_BUTTON_DEVICE" },
    reset_command: StringPolicy { env_var: "HELIOS_BOOT_BUTTON_RESET_CMD", default: "/usr/local/bin/helios-network-reset.sh" },
    discovery_retry_ms: BoundedU64Policy { env_var: "HELIOS_BOOT_BUTTON_DISCOVERY_RETRY_MS", default: 5_000, min: 50, max: 60_000 },
    idle_poll_ms: BoundedU64Policy { env_var: "HELIOS_BOOT_BUTTON_IDLE_POLL_MS", default: 25, min: 1, max: 5_000 },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsbRecoveryDaemonPolicy {
    pub ports: StringPolicy,
    pub baud: BoundedU64Policy,
    pub staging_dir: PathPolicy,
    pub reboot_normal_cmd: StringPolicy,
    pub reboot_bootloader_cmd: OptionalStringPolicy,
    pub device_id: OptionalStringPolicy,
    pub updater_socket: PathPolicy,
    pub updater_journal_path: PathPolicy,
    pub max_line_bytes: BoundedUsizePolicy,
    pub pending_reset_idle_ms: BoundedU64Policy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedUsbRecoveryDaemonPolicy {
    pub ports: Vec<String>,
    pub baud: u32,
    pub staging_dir: PathBuf,
    pub reboot_normal_cmd: String,
    pub reboot_bootloader_cmd: Option<String>,
    pub device_id: Option<String>,
    pub updater_socket: PathBuf,
    pub updater_journal_path: PathBuf,
    pub max_line_bytes: usize,
    pub pending_reset_idle: Duration,
}

impl UsbRecoveryDaemonPolicy {
    pub fn resolve(self) -> ResolvedUsbRecoveryDaemonPolicy {
        let ports = resolve_csv_list(self.ports.resolve());
        ResolvedUsbRecoveryDaemonPolicy {
            ports,
            baud: self.baud.resolve().min(u32::MAX as u64) as u32,
            staging_dir: self.staging_dir.resolve(),
            reboot_normal_cmd: self.reboot_normal_cmd.resolve(),
            reboot_bootloader_cmd: self.reboot_bootloader_cmd.resolve(),
            device_id: self.device_id.resolve(),
            updater_socket: self.updater_socket.resolve(),
            updater_journal_path: self.updater_journal_path.resolve(),
            max_line_bytes: self.max_line_bytes.resolve(),
            pending_reset_idle: Duration::from_millis(self.pending_reset_idle_ms.resolve()),
        }
    }
}

pub const HELIOS_USB_RECOVERY_DAEMON_POLICY: UsbRecoveryDaemonPolicy = UsbRecoveryDaemonPolicy {
    ports: StringPolicy { env_var: "HELIOS_USB_RECOVERY_PORTS", default: "/dev/ttyGS0,/dev/ttyGS1" },
    baud: BoundedU64Policy { env_var: "HELIOS_USB_RECOVERY_BAUD", default: 115_200, min: 1, max: u32::MAX as u64 },
    staging_dir: PathPolicy { env_var: "HELIOS_USB_RECOVERY_STAGING_DIR", default: "/var/lib/helios/usb-recovery" },
    reboot_normal_cmd: StringPolicy { env_var: "HELIOS_USB_RECOVERY_REBOOT_NORMAL_CMD", default: "systemctl reboot" },
    reboot_bootloader_cmd: OptionalStringPolicy { env_var: "HELIOS_USB_RECOVERY_REBOOT_BOOTLOADER_CMD" },
    device_id: OptionalStringPolicy { env_var: "HELIOS_USB_RECOVERY_DEVICE_ID" },
    updater_socket: PathPolicy { env_var: "HELIOS_USB_RECOVERY_UPDATER_SOCKET", default: "/run/helios/updater.sock" },
    updater_journal_path: PathPolicy { env_var: "HELIOS_USB_RECOVERY_UPDATER_JOURNAL_PATH", default: "/var/lib/helios/journal/ipc/updater-usb-recoveryd.journal" },
    max_line_bytes: BoundedUsizePolicy { env_var: "HELIOS_USB_RECOVERY_MAX_LINE_BYTES", default: 1_048_576, min: 1024, max: 32 * 1024 * 1024 },
    pending_reset_idle_ms: BoundedU64Policy { env_var: "HELIOS_USB_RECOVERY_PENDING_RESET_IDLE_MS", default: 3_000, min: 100, max: 120_000 },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagnosticsDaemonPolicy {
    pub base_dir: PathPolicy,
    pub tar: BoolPolicy,
    pub keep: BoundedUsizePolicy,
    pub max_mb: OptionalBoundedU64Policy,
    pub max_cmd_bytes: BoundedUsizePolicy,
    pub cmd_timeout_secs: BoundedU64Policy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedDiagnosticsDaemonPolicy {
    pub base_dir: PathBuf,
    pub tar: bool,
    pub keep: usize,
    pub max_mb: Option<u64>,
    pub max_cmd_bytes: usize,
    pub cmd_timeout: Duration,
}

impl DiagnosticsDaemonPolicy {
    pub fn resolve(self) -> ResolvedDiagnosticsDaemonPolicy {
        ResolvedDiagnosticsDaemonPolicy {
            base_dir: self.base_dir.resolve(),
            tar: self.tar.resolve(),
            keep: self.keep.resolve(),
            max_mb: self.max_mb.resolve(),
            max_cmd_bytes: self.max_cmd_bytes.resolve(),
            cmd_timeout: Duration::from_secs(self.cmd_timeout_secs.resolve()),
        }
    }
}

pub const HELIOS_DIAGNOSTICS_DAEMON_POLICY: DiagnosticsDaemonPolicy = DiagnosticsDaemonPolicy {
    base_dir: PathPolicy { env_var: "HELIOS_DIAGNOSTICS_BASE_DIR", default: "/var/lib/helios/diagnostics" },
    tar: BoolPolicy { env_var: "HELIOS_DIAGNOSTICS_TAR", default: true },
    keep: BoundedUsizePolicy { env_var: "HELIOS_DIAGNOSTICS_KEEP", default: 8, min: 1, max: 1024 },
    max_mb: OptionalBoundedU64Policy { env_var: "HELIOS_DIAGNOSTICS_MAX_MB", min: 1, max: 1024 * 1024 },
    max_cmd_bytes: BoundedUsizePolicy { env_var: "HELIOS_DIAGNOSTICS_MAX_CMD_BYTES", default: 1_000_000, min: 1024, max: 64 * 1024 * 1024 },
    cmd_timeout_secs: BoundedU64Policy { env_var: "HELIOS_DIAGNOSTICS_CMD_TIMEOUT_SECS", default: 15, min: 1, max: 300 },
};
