use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use tempfile::Builder as TempDirBuilder;

const CONFIRM_REQUEST_FILE: &str = "confirm-request.env";
const CONFIRM_RESULT_FILE: &str = "confirm-result.env";

#[derive(Debug, Clone)]
pub struct BootConfirmConfig {
    pub local_ota_dir: PathBuf,
    pub layout_env_path: PathBuf,
    pub mounts_path: PathBuf,
    pub boot_mount_dir: PathBuf,
    pub boot_label_dir: PathBuf,
    pub temp_parent_dir: PathBuf,
    pub mount_program: PathBuf,
    pub umount_program: PathBuf,
    pub sync_program: PathBuf,
}

impl Default for BootConfirmConfig {
    fn default() -> Self {
        Self {
            local_ota_dir: PathBuf::from("/var/lib/helios/ota"),
            layout_env_path: PathBuf::from("/etc/helios/storage-layout.env"),
            mounts_path: PathBuf::from("/proc/mounts"),
            boot_mount_dir: PathBuf::from("/boot"),
            boot_label_dir: PathBuf::from("/dev/disk/by-label"),
            temp_parent_dir: PathBuf::from("/tmp"),
            mount_program: PathBuf::from("mount"),
            umount_program: PathBuf::from("umount"),
            sync_program: PathBuf::from("sync"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootConfirmOutcome {
    NoRequest,
    Confirmed { update_id: String, selector: String },
    Mismatch { update_id: String, expected: String, active: Option<String> },
}

#[derive(Debug, thiserror::Error)]
pub enum BootConfirmError {
    #[error("confirm request is missing {key}")]
    MissingRequestKey { key: &'static str },
    #[error("required command failed: {command}: {message}")]
    CommandFailed { command: String, message: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
struct ConfirmRequest {
    update_id: String,
    workload_id: String,
    expected_selector: String,
}

#[derive(Debug, Clone, Default)]
struct StorageLayout {
    boot_label: Option<String>,
    boot_partition: Option<u32>,
}

#[derive(Debug, Clone)]
struct ConfirmResult {
    request: ConfirmRequest,
    status: &'static str,
    active: Option<String>,
    reserve: Option<String>,
}

pub fn confirm_boot(config: &BootConfirmConfig) -> Result<BootConfirmOutcome, BootConfirmError> {
    let request_path = config.local_ota_dir.join(CONFIRM_REQUEST_FILE);
    let request = match read_confirm_request(&request_path)? {
        Some(request) => request,
        None => return Ok(BootConfirmOutcome::NoRequest),
    };

    let active = read_marker(&config.local_ota_dir.join("active"))?;
    let reserve = read_marker(&config.local_ota_dir.join("reserve"))?;
    let status = if active.as_deref() == Some(request.expected_selector.as_str()) {
        remove_file_ok(&config.local_ota_dir.join("pending"))?;
        "confirmed"
    } else {
        "mismatch"
    };

    let result = ConfirmResult { request, status, active, reserve };
    write_result_dir(&config.local_ota_dir, &result)?;
    mirror_boot_ota(config, &result)?;
    sync_path(config, &config.local_ota_dir)?;

    Ok(match result.status {
        "confirmed" => BootConfirmOutcome::Confirmed { update_id: result.request.update_id, selector: result.active.unwrap_or_else(|| result.request.expected_selector) },
        _ => BootConfirmOutcome::Mismatch { update_id: result.request.update_id, expected: result.request.expected_selector, active: result.active },
    })
}

fn read_confirm_request(path: &Path) -> Result<Option<ConfirmRequest>, BootConfirmError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(BootConfirmError::Io(error)),
    };
    let values = parse_env(&contents);
    let update_id = required_env(&values, "HELIOS_UPDATE_ID")?;
    let expected_selector = required_env(&values, "HELIOS_UPDATE_EXPECTED_SELECTOR")?;
    let workload_id = values.get("HELIOS_UPDATE_WORKLOAD_ID").cloned().unwrap_or_else(|| update_id.clone());
    Ok(Some(ConfirmRequest { update_id, workload_id, expected_selector }))
}

fn required_env(values: &BTreeMap<String, String>, key: &'static str) -> Result<String, BootConfirmError> {
    values.get(key).filter(|value| !value.is_empty()).cloned().ok_or(BootConfirmError::MissingRequestKey { key })
}

fn mirror_boot_ota(config: &BootConfirmConfig, result: &ConfirmResult) -> Result<(), BootConfirmError> {
    if is_mounted(&config.mounts_path, &config.boot_mount_dir)? {
        let boot_ota_dir = config.boot_mount_dir.join("helios/ota");
        write_result_dir(&boot_ota_dir, result)?;
        sync_path(config, &config.boot_mount_dir)?;
        return Ok(());
    }

    let Some(boot_device) = find_boot_device(config)? else {
        return Ok(());
    };

    let mount_root = TempDirBuilder::new().prefix("helios-updater-boot-confirm-").tempdir_in(&config.temp_parent_dir)?;
    mount_filesystem(config, &boot_device, mount_root.path())?;
    let result_write = write_result_dir(&mount_root.path().join("helios/ota"), result);
    let sync_result = sync_path(config, mount_root.path());
    let unmount_result = unmount_filesystem(config, mount_root.path());

    match (result_write, sync_result, unmount_result) {
        (Err(error), _, _) => Err(error),
        (_, Err(error), _) => Err(error),
        (_, _, Err(error)) => Err(error),
        _ => Ok(()),
    }
}

fn write_result_dir(dir: &Path, result: &ConfirmResult) -> Result<(), BootConfirmError> {
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!("{CONFIRM_RESULT_FILE}.tmp"));
    fs::write(
        &tmp,
        format!(
            "HELIOS_UPDATE_CONFIRM_VERSION=1\nHELIOS_UPDATE_ID={}\nHELIOS_UPDATE_WORKLOAD_ID={}\nHELIOS_UPDATE_CONFIRM_STATUS={}\nHELIOS_UPDATE_CONFIRMED_SELECTOR={}\n",
            result.request.update_id,
            result.request.workload_id,
            result.status,
            result.active.as_deref().unwrap_or("")
        ),
    )?;
    fs::rename(tmp, dir.join(CONFIRM_RESULT_FILE))?;

    if let Some(active) = &result.active {
        fs::write(dir.join("active"), format!("{active}\n"))?;
    }
    if let Some(reserve) = &result.reserve {
        fs::write(dir.join("reserve"), format!("{reserve}\n"))?;
    }
    if result.status == "confirmed" {
        remove_file_ok(&dir.join("pending"))?;
    }
    Ok(())
}

fn find_boot_device(config: &BootConfirmConfig) -> Result<Option<PathBuf>, BootConfirmError> {
    let layout = read_storage_layout(&config.layout_env_path)?;
    let boot_label = layout.boot_label.unwrap_or_else(|| "BOOT".into());
    if !boot_label.is_empty() {
        let label_path = config.boot_label_dir.join(&boot_label);
        if label_path.exists() {
            return Ok(Some(canonicalize(&label_path)));
        }
    }

    let Some(data_source) = mount_source_for(&config.mounts_path, Path::new("/var/lib/helios"))? else {
        return Ok(None);
    };
    let data_source = canonicalize(&data_source);
    let boot_partition = layout.boot_partition.unwrap_or(1);
    Ok(Some(partition_device(&partition_base(&data_source), boot_partition)))
}

fn read_storage_layout(path: &Path) -> Result<StorageLayout, BootConfirmError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(StorageLayout::default()),
        Err(error) => return Err(BootConfirmError::Io(error)),
    };
    let values = parse_env(&contents);
    let boot_partition = values.get("HELIOS_LAYOUT_BOOT_PARTITION").and_then(|value| value.parse::<u32>().ok());
    Ok(StorageLayout { boot_label: values.get("HELIOS_LAYOUT_BOOT_LABEL").cloned(), boot_partition })
}

fn read_marker(path: &Path) -> Result<Option<String>, BootConfirmError> {
    match fs::read_to_string(path) {
        Ok(contents) => {
            let value = contents.trim().to_string();
            Ok((!value.is_empty()).then_some(value))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(BootConfirmError::Io(error)),
    }
}

fn parse_env(contents: &str) -> BTreeMap<String, String> {
    contents
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let (key, value) = trimmed.split_once('=')?;
            Some((key.trim().to_string(), value.trim().trim_matches('"').trim_matches('\'').to_string()))
        })
        .collect()
}

fn is_mounted(mounts_path: &Path, mountpoint: &Path) -> Result<bool, BootConfirmError> {
    Ok(mount_source_for(mounts_path, mountpoint)?.is_some())
}

fn mount_source_for(mounts_path: &Path, mountpoint: &Path) -> Result<Option<PathBuf>, BootConfirmError> {
    let contents = fs::read_to_string(mounts_path)?;
    let mountpoint = mountpoint.to_string_lossy();
    Ok(contents.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let source = fields.next()?;
        let target = fields.next()?;
        (target == mountpoint).then(|| PathBuf::from(source))
    }))
}

fn mount_filesystem(config: &BootConfirmConfig, device: &Path, mountpoint: &Path) -> Result<(), BootConfirmError> {
    let output = Command::new(&config.mount_program).arg("-t").arg("vfat").arg("-o").arg("rw").arg(device).arg(mountpoint).output()?;
    if output.status.success() { Ok(()) } else { Err(BootConfirmError::CommandFailed { command: format!("mount {}", device.display()), message: command_message(&output) }) }
}

fn unmount_filesystem(config: &BootConfirmConfig, mountpoint: &Path) -> Result<(), BootConfirmError> {
    let output = Command::new(&config.umount_program).arg(mountpoint).output()?;
    if output.status.success() { Ok(()) } else { Err(BootConfirmError::CommandFailed { command: format!("umount {}", mountpoint.display()), message: command_message(&output) }) }
}

fn sync_path(config: &BootConfirmConfig, path: &Path) -> Result<(), BootConfirmError> {
    let output = Command::new(&config.sync_program).arg(path).output()?;
    if output.status.success() { Ok(()) } else { Err(BootConfirmError::CommandFailed { command: format!("sync {}", path.display()), message: command_message(&output) }) }
}

fn command_message(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() { stdout } else { format!("exit status {}", output.status) }
}

fn canonicalize(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn partition_base(device: &Path) -> PathBuf {
    let dev = device.display().to_string();
    if let Some(base) = strip_partition_suffix(&dev) { PathBuf::from(base) } else { device.to_path_buf() }
}

fn strip_partition_suffix(device: &str) -> Option<String> {
    if let Some(prefix) = device.strip_prefix("/dev/") {
        if let Some(index) = prefix.rfind('p') {
            let (base, suffix) = prefix.split_at(index);
            if suffix[1..].chars().all(|ch| ch.is_ascii_digit()) && base.chars().last().is_some_and(|ch| ch.is_ascii_digit()) {
                return Some(format!("/dev/{base}"));
            }
        }
        let trimmed = prefix.trim_end_matches(|ch: char| ch.is_ascii_digit());
        if trimmed.len() != prefix.len() {
            return Some(format!("/dev/{trimmed}"));
        }
    }
    None
}

fn partition_device(base: &Path, partition: u32) -> PathBuf {
    let base = base.display().to_string();
    let suffix = if base.chars().last().is_some_and(|ch| ch.is_ascii_digit()) { format!("p{partition}") } else { partition.to_string() };
    PathBuf::from(format!("{base}{suffix}"))
}

fn remove_file_ok(path: &Path) -> Result<(), BootConfirmError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(BootConfirmError::Io(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_config(root: &Path) -> BootConfirmConfig {
        BootConfirmConfig {
            local_ota_dir: root.join("ota"),
            layout_env_path: root.join("storage-layout.env"),
            mounts_path: root.join("mounts"),
            boot_mount_dir: root.join("boot"),
            boot_label_dir: root.join("labels"),
            temp_parent_dir: root.to_path_buf(),
            mount_program: PathBuf::from("false"),
            umount_program: PathBuf::from("false"),
            sync_program: PathBuf::from("true"),
        }
    }

    #[test]
    fn confirm_boot_noops_without_request() {
        let temp = tempdir().expect("temp");
        fs::write(temp.path().join("mounts"), "").expect("mounts");
        let outcome = confirm_boot(&test_config(temp.path())).expect("confirm");
        assert_eq!(outcome, BootConfirmOutcome::NoRequest);
    }

    #[test]
    fn confirm_boot_records_success_and_clears_pending() {
        let temp = tempdir().expect("temp");
        let config = test_config(temp.path());
        fs::create_dir_all(&config.local_ota_dir).expect("ota");
        fs::write(temp.path().join("mounts"), "").expect("mounts");
        fs::write(config.local_ota_dir.join(CONFIRM_REQUEST_FILE), "HELIOS_UPDATE_ID=update-1\nHELIOS_UPDATE_WORKLOAD_ID=workload-1\nHELIOS_UPDATE_EXPECTED_SELECTOR=ROOT_B\n").expect("request");
        fs::write(config.local_ota_dir.join("active"), "ROOT_B\n").expect("active");
        fs::write(config.local_ota_dir.join("reserve"), "ROOT_A\n").expect("reserve");
        fs::write(config.local_ota_dir.join("pending"), "ROOT_B\n").expect("pending");

        let outcome = confirm_boot(&config).expect("confirm");

        assert_eq!(outcome, BootConfirmOutcome::Confirmed { update_id: "update-1".into(), selector: "ROOT_B".into() });
        assert!(!config.local_ota_dir.join("pending").exists());
        let result = fs::read_to_string(config.local_ota_dir.join(CONFIRM_RESULT_FILE)).expect("result");
        assert!(result.contains("HELIOS_UPDATE_CONFIRM_STATUS=confirmed"));
        assert!(result.contains("HELIOS_UPDATE_CONFIRMED_SELECTOR=ROOT_B"));
    }

    #[test]
    fn confirm_boot_records_mismatch_and_keeps_pending() {
        let temp = tempdir().expect("temp");
        let config = test_config(temp.path());
        fs::create_dir_all(&config.local_ota_dir).expect("ota");
        fs::write(temp.path().join("mounts"), "").expect("mounts");
        fs::write(config.local_ota_dir.join(CONFIRM_REQUEST_FILE), "HELIOS_UPDATE_ID=update-1\nHELIOS_UPDATE_EXPECTED_SELECTOR=ROOT_B\n").expect("request");
        fs::write(config.local_ota_dir.join("active"), "ROOT_A\n").expect("active");
        fs::write(config.local_ota_dir.join("pending"), "ROOT_B\n").expect("pending");

        let outcome = confirm_boot(&config).expect("confirm");

        assert_eq!(outcome, BootConfirmOutcome::Mismatch { update_id: "update-1".into(), expected: "ROOT_B".into(), active: Some("ROOT_A".into()) });
        assert!(config.local_ota_dir.join("pending").exists());
        let result = fs::read_to_string(config.local_ota_dir.join(CONFIRM_RESULT_FILE)).expect("result");
        assert!(result.contains("HELIOS_UPDATE_CONFIRM_STATUS=mismatch"));
    }

    #[test]
    fn derives_boot_partition_from_data_mount() {
        let temp = tempdir().expect("temp");
        let config = test_config(temp.path());
        fs::write(&config.layout_env_path, "HELIOS_LAYOUT_BOOT_PARTITION=1\n").expect("layout");
        fs::write(&config.mounts_path, "/dev/mmcblk0p4 /var/lib/helios ext4 rw 0 0\n").expect("mounts");

        assert_eq!(find_boot_device(&config).expect("device"), Some(PathBuf::from("/dev/mmcblk0p1")));
    }

    #[test]
    fn uses_boot_label_before_data_mount_derivation() {
        let temp = tempdir().expect("temp");
        let config = test_config(temp.path());
        fs::create_dir_all(&config.boot_label_dir).expect("labels");
        fs::write(config.boot_label_dir.join("BOOT"), "").expect("label");
        fs::write(&config.mounts_path, "/dev/mmcblk0p4 /var/lib/helios ext4 rw 0 0\n").expect("mounts");

        assert_eq!(find_boot_device(&config).expect("device"), Some(config.boot_label_dir.join("BOOT")));
    }
}
