use std::{
    fs,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    os::unix::fs::FileTypeExt,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tempfile::Builder as TempDirBuilder;

use crate::{
    config::{DEFAULT_STORAGE_LAYOUT_MANIFEST_PATH, UpdaterConfig},
    manifest::{UpdateManifest, UpdatePayload},
    model::{HookPhase, SlotAction, SlotLayout},
    releases::ServiceReleaseManager,
    staging::{StagedArtifact, StagedBootAsset, StagedManifest},
};

const DEFAULT_BOOT_MOUNT_DIR: &str = "/boot";
const DEFAULT_BOOT_OTA_DIR: &str = "/boot/helios/ota";
const DEFAULT_LOCAL_OTA_DIR: &str = "/var/lib/helios/ota";
const DEFAULT_SYSTEM_LAYOUT_MANIFEST: &str = DEFAULT_STORAGE_LAYOUT_MANIFEST_PATH;
const PREPARED_DIR: &str = "prepared";
const REPARTITION_DIR: &str = "repartition";
const FAILED_DIR: &str = "failed";
const HOOKS_DIR: &str = "hooks";
const ACTIONS_DIR: &str = "actions";
const CONFIRM_REQUEST_FILE: &str = "confirm-request.env";
const CONFIRM_RESULT_FILE: &str = "confirm-result.env";
const ROOT_MOUNTS_PATH: &str = "/proc/mounts";
const UPDATER_SERVICE_NAME: &str = "helios-updater";
const IMAGE_OWNED_OVERLAY_PATHS_FILE: &str = "/etc/helios/image-owned-overlay-paths";
const ROOTFS_IMAGE_PARTITION: usize = 2;
const SECTOR_BYTES: u64 = 512;
const XZ_MAGIC: &[u8; 6] = b"\xfd7zXZ\0";

#[derive(Debug, Clone)]
pub struct UpdateExecutor {
    state_dir: PathBuf,
    boot_mount_dir: PathBuf,
    boot_ota_dir: PathBuf,
    local_ota_dir: PathBuf,
    system_layout_manifest_path: PathBuf,
    reboot_program: PathBuf,
    release_manager: ServiceReleaseManager,
    default_hook_timeout_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    BootPrepared { message: String },
    Completed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparedObservation {
    SwitchingBoot { message: String },
    AwaitingBootSuccess { message: String },
    RollingBack { message: String },
    Completed { message: String },
    RolledBack { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepartitionObservation {
    SwitchingBoot { message: String },
    Completed { message: String },
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum PreparedOutcome {
    Completed,
    RolledBack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum RepartitionOutcome {
    Completed { status: String },
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PreparedState {
    workload_id: String,
    message: String,
    prepared_boot_id: String,
    update_id: String,
    expected_selector: String,
    expected_active_device: Option<PathBuf>,
    previous_selector: String,
    previous_active_device: Option<PathBuf>,
    rollback_requested: bool,
    outcome: Option<PreparedOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RepartitionState {
    workload_id: String,
    requested_boot_id: String,
    action: String,
    update_id: String,
    outcome: Option<RepartitionOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailedState {
    pub workload_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RepartitionLayoutManifest {
    #[serde(default)]
    live_repartition: RepartitionPolicy,
    #[serde(default)]
    partitions: Vec<RepartitionLayoutPartition>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RepartitionPolicy {
    #[serde(default)]
    allow_destructive_data_borrow: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct RepartitionLayoutPartition {
    role: Option<String>,
    size_mib: Option<u64>,
    size_source: Option<RepartitionSizeSource>,
    max_mib: Option<u64>,
    fill_to_end: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RepartitionSizeSource {
    MirrorExisting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepartitionResult {
    update_id: String,
    status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ImagePartitionRange {
    offset_bytes: u64,
    size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConfirmResult {
    update_id: String,
    status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PendingAction {
    RestartService { name: String },
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateExecuteError {
    #[error("os-image update is missing an inactive slot device")]
    MissingInactiveDevice,
    #[error("os-image update is missing an inactive slot name")]
    MissingInactiveName,
    #[error("boot asset update requires a mounted boot directory at {path}")]
    MissingBootMount { path: PathBuf },
    #[error("boot asset path must be relative and stay inside the boot partition: {path}")]
    InvalidBootAssetPath { path: PathBuf },
    #[error("unsupported image format for {path}")]
    UnsupportedImageFormat { path: PathBuf },
    #[error("required command failed: {command}: {message}")]
    CommandFailed { command: String, message: String },
    #[error("slot scheme {scheme} is not supported for apply")]
    UnsupportedSlotScheme { scheme: String },
    #[error("hook {phase:?} is missing an executable command")]
    MissingHookCommand { phase: HookPhase },
    #[error("hook {phase:?} failed: {message}")]
    HookFailed { phase: HookPhase, message: String },
    #[error("payload update failed: {message}")]
    PayloadApplyFailed { message: String },
    #[error("repartition request is invalid: {message}")]
    InvalidRepartitionRequest { message: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Release(#[from] crate::releases::ServiceReleaseError),
}

impl UpdateExecutor {
    pub fn from_config(config: &UpdaterConfig) -> Self {
        Self {
            state_dir: config.updater_state_dir.clone(),
            boot_mount_dir: DEFAULT_BOOT_MOUNT_DIR.into(),
            boot_ota_dir: DEFAULT_BOOT_OTA_DIR.into(),
            local_ota_dir: DEFAULT_LOCAL_OTA_DIR.into(),
            system_layout_manifest_path: DEFAULT_SYSTEM_LAYOUT_MANIFEST.into(),
            reboot_program: PathBuf::from("systemctl"),
            release_manager: ServiceReleaseManager::new(config.service_releases_dir.clone(), config.service_bin_dir.clone()),
            default_hook_timeout_secs: config.hook_timeout_secs,
        }
    }

    pub fn is_prepared(&self, workload_id: &str) -> bool {
        self.prepared_marker_path(workload_id).exists()
    }

    pub fn failed_state(&self, workload_id: &str) -> Result<Option<FailedState>, UpdateExecuteError> {
        let path = self.failed_marker_path(workload_id);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(UpdateExecuteError::Io(error)),
        }
    }

    pub fn record_failed(&self, workload_id: &str, message: impl Into<String>) -> Result<(), UpdateExecuteError> {
        let path = self.failed_marker_path(workload_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let state = FailedState { workload_id: workload_id.to_string(), message: message.into() };
        fs::write(path, serde_json::to_vec_pretty(&state)?)?;
        Ok(())
    }

    pub fn ensure_image_managed_bins(&self, names: &[&str]) -> Result<(), UpdateExecuteError> {
        self.release_manager.ensure_image_defaults(names)?;
        Ok(())
    }

    pub fn observe_prepared(&self, workload_id: &str, slots: &SlotLayout) -> Result<Option<PreparedObservation>, UpdateExecuteError> {
        let Some(state) = self.load_prepared(workload_id)? else {
            return Ok(None);
        };

        if let Some(outcome) = state.outcome {
            return Ok(Some(match outcome {
                PreparedOutcome::Completed => PreparedObservation::Completed { message: state.message },
                PreparedOutcome::RolledBack => PreparedObservation::RolledBack { message: state.message },
            }));
        }

        if let Some(result) = self.confirm_result_for(&state.update_id)? {
            if result.status == "confirmed" {
                self.mark_outcome(workload_id, PreparedOutcome::Completed)?;
                let Some(updated) = self.load_prepared(workload_id)? else {
                    return Ok(Some(PreparedObservation::Completed { message: state.message }));
                };
                return Ok(Some(PreparedObservation::Completed { message: updated.message }));
            }
        }

        let pending_selector = self.pending_selector()?;
        let current_boot_id = current_boot_id()?;
        let rebooted = current_boot_id != state.prepared_boot_id;
        let current_active_device = slots.active_device.as_ref().map(|path| canonicalize_path(path));
        let expected_active_device = state.expected_active_device.as_ref().map(|path| canonicalize_path(path));

        let observation = if pending_selector.as_deref() == Some(state.expected_selector.as_str()) {
            if state.rollback_requested {
                PreparedObservation::RollingBack { message: state.message }
            } else if rebooted && expected_active_device.is_some() && current_active_device == expected_active_device {
                self.confirm_prepared_boot(slots, &state)?;
                PreparedObservation::Completed { message: state.message }
            } else if rebooted {
                PreparedObservation::AwaitingBootSuccess { message: state.message }
            } else {
                PreparedObservation::SwitchingBoot { message: state.message }
            }
        } else if rebooted {
            if expected_active_device.is_some() && current_active_device == expected_active_device {
                self.confirm_prepared_boot(slots, &state)?;
                PreparedObservation::Completed { message: state.message }
            } else {
                PreparedObservation::RolledBack { message: format!("{}; current_active_device={:?} expected_active_device={:?}", state.message, current_active_device, expected_active_device) }
            }
        } else {
            PreparedObservation::SwitchingBoot { message: state.message }
        };

        Ok(Some(observation))
    }

    pub fn apply_staged(&self, workload_id: &str, manifest: &UpdateManifest, slots: &SlotLayout, staged: &StagedManifest) -> Result<ApplyOutcome, UpdateExecuteError> {
        self.run_manifest_hooks(workload_id, manifest, HookPhase::Preinstall)?;
        match (&manifest.payload, &staged.artifact) {
            (UpdatePayload::OsImage(_), StagedArtifact::OsImage { path, boot_assets }) => {
                let message = self.apply_os_image(slots, path, boot_assets)?;
                self.run_manifest_hooks(workload_id, manifest, HookPhase::Preswitch)?;
                self.record_prepared(workload_id, slots, &message)?;
                self.request_reboot()?;
                Ok(ApplyOutcome::BootPrepared { message })
            }
            (UpdatePayload::PayloadUpdate(_), StagedArtifact::PayloadUpdate { staged_targets }) => self.apply_payload_update(workload_id, manifest, staged_targets),
            _ => Err(UpdateExecuteError::PayloadApplyFailed { message: "staged artifact does not match manifest payload".into() }),
        }
    }

    pub fn observe_repartition(&self, workload_id: &str, slots: &SlotLayout) -> Result<Option<RepartitionObservation>, UpdateExecuteError> {
        let Some(state) = self.load_repartition(workload_id)? else {
            return Ok(None);
        };

        if let Some(outcome) = &state.outcome {
            return Ok(Some(match outcome {
                RepartitionOutcome::Completed { status } => RepartitionObservation::Completed { message: format!("repartition request {} completed with status {}", state.action, status) },
                RepartitionOutcome::Failed { message } => RepartitionObservation::Failed { message: message.clone() },
            }));
        }

        if let Some(result) = self.repartition_result_for(&state.update_id, slots)? {
            let outcome = match result.status.as_str() {
                "applied" | "recovered" | "unchanged" => RepartitionOutcome::Completed { status: result.status },
                other => RepartitionOutcome::Failed { message: format!("repartition request {} returned unexpected status {}", state.action, other) },
            };
            self.mark_repartition_outcome(workload_id, outcome.clone())?;
            return Ok(Some(match outcome {
                RepartitionOutcome::Completed { status } => RepartitionObservation::Completed { message: format!("repartition request {} completed with status {}", state.action, status) },
                RepartitionOutcome::Failed { message } => RepartitionObservation::Failed { message },
            }));
        }

        Ok(Some(RepartitionObservation::SwitchingBoot { message: format!("repartition request {} pending reboot/apply", state.action) }))
    }

    pub fn request_repartition(&self, workload_id: &str, slot_action: &SlotAction, slots: &SlotLayout) -> Result<String, UpdateExecuteError> {
        self.validate_repartition_request(slot_action, slots)?;
        let update_id = sanitize_component(workload_id);
        let layout_bytes = self.render_repartition_layout(slot_action, slots)?;
        let request_body = format!("HELIOS_REPARTITION_UPDATE_ID={update_id}\nHELIOS_REPARTITION_ACTION={}", slot_action_label(slot_action));
        self.write_file(self.local_ota_dir.join("repartition-request.env"), &request_body)?;
        self.write_bytes(self.local_ota_dir.join("repartition-layout.toml"), &layout_bytes)?;
        self.with_boot_paths(slots, |_, boot_ota_dir| {
            self.write_file(boot_ota_dir.join("repartition-request.env"), &request_body)?;
            self.write_bytes(boot_ota_dir.join("repartition-layout.toml"), &layout_bytes)?;
            Ok(())
        })?;
        self.record_repartition(workload_id, slot_action, &update_id)?;
        self.request_reboot()?;
        Ok(format!("requested repartition action {} for workload {}", slot_action_label(slot_action), workload_id))
    }

    fn render_repartition_layout(&self, _slot_action: &SlotAction, slots: &SlotLayout) -> Result<Vec<u8>, UpdateExecuteError> {
        let mut layout = toml::from_str::<toml::Value>(&fs::read_to_string(&self.system_layout_manifest_path)?)
            .map_err(|error| UpdateExecuteError::InvalidRepartitionRequest { message: format!("failed to parse storage layout manifest {}: {error}", self.system_layout_manifest_path.display()) })?;

        if slots.scheme == "squashfs_ab" && slots.active_name == "ROOT_B" && slots.inactive_name.as_deref() == Some("ROOT_A") {
            swap_live_repartition_slots(&mut layout)?;
        }

        toml::to_string_pretty(&layout)
            .map(|rendered| rendered.into_bytes())
            .map_err(|error| UpdateExecuteError::InvalidRepartitionRequest { message: format!("failed to render storage layout manifest {}: {error}", self.system_layout_manifest_path.display()) })
    }

    pub fn finalize_completed(&self, workload_id: &str, manifest: &UpdateManifest) -> Result<String, UpdateExecuteError> {
        self.run_manifest_hooks(workload_id, manifest, HookPhase::Postboot)?;
        self.scrub_active_overlay_image_overrides()?;
        self.clear_staged_artifact(manifest)?;
        self.clear_repartition_metadata()?;
        self.clear_repartition_state(workload_id)?;
        self.mark_outcome(workload_id, PreparedOutcome::Completed)
    }

    pub fn finalize_rollback(&self, workload_id: &str, manifest: &UpdateManifest) -> Result<String, UpdateExecuteError> {
        self.run_manifest_hooks(workload_id, manifest, HookPhase::RollbackCleanup)?;
        self.scrub_active_overlay_image_overrides()?;
        self.clear_staged_artifact(manifest)?;
        self.clear_repartition_metadata()?;
        self.clear_repartition_state(workload_id)?;
        self.mark_outcome(workload_id, PreparedOutcome::RolledBack)
    }

    fn scrub_active_overlay_image_overrides(&self) -> Result<(), UpdateExecuteError> {
        let mounts = match fs::read_to_string(ROOT_MOUNTS_PATH) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(UpdateExecuteError::Io(error)),
        };
        let Some(upperdir) = overlay_upperdir_from_mounts(&mounts) else {
            return Ok(());
        };
        scrub_overlay_image_overrides(&upperdir, Path::new(IMAGE_OWNED_OVERLAY_PATHS_FILE))
    }

    fn clear_staged_artifact(&self, manifest: &UpdateManifest) -> Result<(), UpdateExecuteError> {
        let path = self.state_dir.join("staging").join(sanitize_component(&manifest.artifact_id));
        match fs::remove_dir_all(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(UpdateExecuteError::Io(error)),
        }
    }

    fn apply_os_image(&self, slots: &SlotLayout, staged_image: &Path, boot_assets: &[StagedBootAsset]) -> Result<String, UpdateExecuteError> {
        let target_device = slots.inactive_device.as_ref().ok_or(UpdateExecuteError::MissingInactiveDevice)?;
        let target_name = slots.inactive_name.as_deref().ok_or(UpdateExecuteError::MissingInactiveName)?;
        let boot_selector = self.boot_selector(slots, target_name)?;

        write_image_to_device(staged_image, target_device)?;
        validate_written_rootfs(slots, target_device)?;
        self.install_boot_assets(slots, boot_assets)?;

        match slots.scheme.as_str() {
            "squashfs_ab" => {
                self.write_boot_cmdline_root(slots, "/dev/helios-rootfs")?;
                self.switch_squashfs_slot(slots, target_name)?;
            }
            scheme => return Err(UpdateExecuteError::UnsupportedSlotScheme { scheme: scheme.to_string() }),
        }

        self.write_pending_markers(slots, &boot_selector)?;
        Ok(format!("prepared inactive slot {} at {} with {} boot assets", target_name, target_device.display(), boot_assets.len()))
    }

    fn apply_payload_update(&self, workload_id: &str, manifest: &UpdateManifest, staged_targets: &[crate::staging::StagedPayloadTarget]) -> Result<ApplyOutcome, UpdateExecuteError> {
        self.run_manifest_hooks(workload_id, manifest, HookPhase::Preswitch)?;
        let restart_services = systemd_available();

        let mut activated = Vec::with_capacity(staged_targets.len());
        let mut already_active = 0usize;
        for target in staged_targets {
            if self.release_manager.status(&target.name)?.active_revision.as_deref() == Some(target.revision.as_str()) {
                already_active += 1;
                continue;
            }
            let apply_result = (|| -> Result<(), UpdateExecuteError> {
                self.release_manager.activate(&target.name, &target.revision)?;
                self.restart_or_defer_service(&target.name, restart_services)?;
                Ok(())
            })();
            if let Err(error) = apply_result {
                self.rollback_payload_targets(&activated);
                let _ = self.run_manifest_hooks(workload_id, manifest, HookPhase::RollbackCleanup);
                return Err(UpdateExecuteError::PayloadApplyFailed { message: format!("failed activating target {} revision {}: {error}", target.name, target.revision) });
            }
            activated.push(target.name.clone());
        }

        if let Err(error) = self.run_manifest_hooks(workload_id, manifest, HookPhase::Postboot) {
            self.rollback_payload_targets(&activated);
            let _ = self.run_manifest_hooks(workload_id, manifest, HookPhase::RollbackCleanup);
            return Err(UpdateExecuteError::PayloadApplyFailed { message: format!("postboot hook failed after payload activation: {error}") });
        }

        Ok(ApplyOutcome::Completed {
            message: if restart_services {
                let deferred_restarts = staged_targets.iter().filter(|target| target.name == UPDATER_SERVICE_NAME).count();
                if deferred_restarts > 0 && already_active == 0 {
                    format!("activated {} payload targets; deferred {} self-restart{} until after publish", staged_targets.len(), deferred_restarts, if deferred_restarts == 1 { "" } else { "s" })
                } else if already_active > 0 {
                    format!("activated {} payload targets; {} already active", staged_targets.len().saturating_sub(already_active), already_active)
                } else {
                    format!("activated and restarted {} payload targets", staged_targets.len())
                }
            } else if already_active > 0 {
                format!("activated {} payload targets; {} already active", staged_targets.len().saturating_sub(already_active), already_active)
            } else {
                format!("activated {} payload targets", staged_targets.len())
            },
        })
    }

    fn switch_squashfs_slot(&self, slots: &SlotLayout, target_name: &str) -> Result<(), UpdateExecuteError> {
        let reserve_name = squashfs_reserve_name(target_name)?;

        self.write_file(self.local_ota_dir.join("active"), target_name)?;
        self.write_file(self.local_ota_dir.join("reserve"), reserve_name)?;
        self.with_boot_paths(slots, |_, boot_ota_dir| {
            self.write_file(boot_ota_dir.join("active"), target_name)?;
            self.write_file(boot_ota_dir.join("reserve"), reserve_name)?;
            Ok(())
        })?;
        Ok(())
    }

    fn confirm_prepared_boot(&self, slots: &SlotLayout, state: &PreparedState) -> Result<(), UpdateExecuteError> {
        match slots.scheme.as_str() {
            "squashfs_ab" => {
                let active_name = slots.active_name.as_str();
                let reserve_name = slots.inactive_name.as_deref().unwrap_or(squashfs_reserve_name(active_name)?);
                let body = format!(
                    "HELIOS_UPDATE_ID={}\nHELIOS_UPDATE_WORKLOAD_ID={}\nHELIOS_UPDATE_CONFIRM_STATUS=confirmed\nHELIOS_UPDATE_CONFIRMED_SELECTOR={}",
                    state.update_id, state.workload_id, active_name
                );

                self.write_file(self.local_ota_dir.join("active"), active_name)?;
                self.write_file(self.local_ota_dir.join("reserve"), reserve_name)?;
                self.write_file(self.local_ota_dir.join(CONFIRM_RESULT_FILE), &body)?;
                remove_file_ok(&self.local_ota_dir.join("pending"))?;

                self.with_boot_paths(slots, |_, boot_ota_dir| {
                    self.write_file(boot_ota_dir.join("active"), active_name)?;
                    self.write_file(boot_ota_dir.join("reserve"), reserve_name)?;
                    self.write_file(boot_ota_dir.join(CONFIRM_RESULT_FILE), &body)?;
                    remove_file_ok(&boot_ota_dir.join("pending"))?;
                    Ok(())
                })?;
                Ok(())
            }
            scheme => Err(UpdateExecuteError::UnsupportedSlotScheme { scheme: scheme.to_string() }),
        }
    }

    fn write_pending_markers(&self, slots: &SlotLayout, selector: &str) -> Result<(), UpdateExecuteError> {
        self.write_file(self.local_ota_dir.join("pending"), selector)?;
        self.with_boot_paths(slots, |_, boot_ota_dir| {
            self.write_file(boot_ota_dir.join("pending"), selector)?;
            Ok(())
        })?;
        Ok(())
    }

    fn record_prepared(&self, workload_id: &str, slots: &SlotLayout, message: &str) -> Result<(), UpdateExecuteError> {
        let inactive_name = slots.inactive_name.as_deref().ok_or(UpdateExecuteError::MissingInactiveName)?;
        let state = PreparedState {
            workload_id: workload_id.to_string(),
            message: message.to_string(),
            prepared_boot_id: current_boot_id()?,
            update_id: sanitize_component(workload_id),
            expected_selector: self.boot_selector(slots, inactive_name)?,
            expected_active_device: slots.inactive_device.clone(),
            previous_selector: self.current_selector(slots)?,
            previous_active_device: slots.active_device.clone(),
            rollback_requested: false,
            outcome: None,
        };

        let path = self.prepared_marker_path(workload_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_vec_pretty(&state)?)?;
        self.write_confirm_request(slots, &state)?;
        Ok(())
    }

    fn record_repartition(&self, workload_id: &str, slot_action: &SlotAction, update_id: &str) -> Result<(), UpdateExecuteError> {
        let state = RepartitionState {
            workload_id: workload_id.to_string(),
            requested_boot_id: current_boot_id()?,
            action: slot_action_label(slot_action).to_string(),
            update_id: update_id.to_string(),
            outcome: None,
        };
        let path = self.repartition_marker_path(workload_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_vec_pretty(&state)?)?;
        Ok(())
    }

    fn load_prepared(&self, workload_id: &str) -> Result<Option<PreparedState>, UpdateExecuteError> {
        let path = self.prepared_marker_path(workload_id);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(UpdateExecuteError::Io(error)),
        }
    }

    fn prepared_marker_path(&self, workload_id: &str) -> PathBuf {
        self.state_dir.join(PREPARED_DIR).join(format!("{}.json", sanitize_component(workload_id)))
    }

    fn failed_marker_path(&self, workload_id: &str) -> PathBuf {
        self.state_dir.join(FAILED_DIR).join(format!("{}.json", sanitize_component(workload_id)))
    }

    fn load_repartition(&self, workload_id: &str) -> Result<Option<RepartitionState>, UpdateExecuteError> {
        let path = self.repartition_marker_path(workload_id);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(UpdateExecuteError::Io(error)),
        }
    }

    fn repartition_marker_path(&self, workload_id: &str) -> PathBuf {
        self.state_dir.join(REPARTITION_DIR).join(format!("{}.json", sanitize_component(workload_id)))
    }

    fn pending_selector(&self) -> Result<Option<String>, UpdateExecuteError> {
        for path in [self.boot_ota_dir.join("pending"), self.local_ota_dir.join("pending")] {
            match fs::read_to_string(&path) {
                Ok(contents) => {
                    let value = contents.trim().to_string();
                    if !value.is_empty() {
                        return Ok(Some(value));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(UpdateExecuteError::Io(error)),
            }
        }
        Ok(None)
    }

    fn confirm_result_for(&self, update_id: &str) -> Result<Option<ConfirmResult>, UpdateExecuteError> {
        for path in [self.boot_ota_dir.join(CONFIRM_RESULT_FILE), self.local_ota_dir.join(CONFIRM_RESULT_FILE)] {
            match fs::read_to_string(&path) {
                Ok(contents) => {
                    let values = parse_shell_env(&contents);
                    if values.get("HELIOS_UPDATE_ID").map(String::as_str) == Some(update_id)
                        && let Some(status) = values.get("HELIOS_UPDATE_CONFIRM_STATUS")
                    {
                        return Ok(Some(ConfirmResult { update_id: update_id.to_string(), status: status.clone() }));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(UpdateExecuteError::Io(error)),
            }
        }
        Ok(None)
    }

    fn mark_outcome(&self, workload_id: &str, outcome: PreparedOutcome) -> Result<String, UpdateExecuteError> {
        let Some(mut state) = self.load_prepared(workload_id)? else {
            return Ok("no prepared state present".into());
        };
        if state.outcome.as_ref() == Some(&outcome) {
            return Ok(state.message);
        }
        state.outcome = Some(outcome.clone());
        let message = match outcome {
            PreparedOutcome::Completed => format!("completed {}", state.message),
            PreparedOutcome::RolledBack => format!("rolled back {}", state.message),
        };
        state.message = message.clone();
        let path = self.prepared_marker_path(workload_id);
        fs::write(path, serde_json::to_vec_pretty(&state)?)?;
        if matches!(outcome, PreparedOutcome::Completed | PreparedOutcome::RolledBack) {
            self.clear_confirm_metadata()?;
        }
        Ok(message)
    }

    pub fn rollback_prepared(&self, workload_id: &str, slots: &SlotLayout) -> Result<String, UpdateExecuteError> {
        let Some(mut state) = self.load_prepared(workload_id)? else {
            return Ok("no prepared state present".into());
        };
        if state.rollback_requested {
            return Ok(state.message);
        }

        match slots.scheme.as_str() {
            "squashfs_ab" => {
                let active_name = slots.active_name.as_str();
                let rollback_target = slots.inactive_name.as_deref().ok_or(UpdateExecuteError::MissingInactiveName)?;
                self.write_boot_cmdline_root(slots, "/dev/helios-rootfs")?;
                self.write_file(self.local_ota_dir.join("active"), rollback_target)?;
                self.write_file(self.local_ota_dir.join("reserve"), active_name)?;
                self.with_boot_paths(slots, |_, boot_ota_dir| {
                    self.write_file(boot_ota_dir.join("active"), rollback_target)?;
                    self.write_file(boot_ota_dir.join("reserve"), active_name)?;
                    Ok(())
                })?;
            }
            scheme => return Err(UpdateExecuteError::UnsupportedSlotScheme { scheme: scheme.to_string() }),
        }

        self.write_pending_markers(slots, &state.previous_selector)?;
        state.rollback_requested = true;
        state.expected_selector = state.previous_selector.clone();
        state.expected_active_device = state.previous_active_device.clone();
        state.message = format!("rollback requested for workload {}", workload_id);
        let path = self.prepared_marker_path(workload_id);
        fs::write(path, serde_json::to_vec_pretty(&state)?)?;
        self.write_confirm_request(slots, &state)?;
        self.request_reboot()?;
        Ok(state.message)
    }

    fn mark_repartition_outcome(&self, workload_id: &str, outcome: RepartitionOutcome) -> Result<(), UpdateExecuteError> {
        let Some(mut state) = self.load_repartition(workload_id)? else {
            return Ok(());
        };
        state.outcome = Some(outcome);
        let path = self.repartition_marker_path(workload_id);
        fs::write(path, serde_json::to_vec_pretty(&state)?)?;
        Ok(())
    }

    fn clear_repartition_state(&self, workload_id: &str) -> Result<(), UpdateExecuteError> {
        let path = self.repartition_marker_path(workload_id);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(UpdateExecuteError::Io(error)),
        }
    }

    fn repartition_result_for(&self, update_id: &str, slots: &SlotLayout) -> Result<Option<RepartitionResult>, UpdateExecuteError> {
        if let Some(result) = Self::parse_repartition_result_file(&self.local_ota_dir.join("repartition-result.env"), update_id)? {
            return Ok(Some(result));
        }

        self.with_boot_paths(slots, |_, boot_ota_dir| Self::parse_repartition_result_file(&boot_ota_dir.join("repartition-result.env"), update_id))
    }

    fn parse_repartition_result_file(path: &Path, update_id: &str) -> Result<Option<RepartitionResult>, UpdateExecuteError> {
        match fs::read_to_string(path) {
            Ok(contents) => {
                let values = parse_shell_env(&contents);
                if values.get("HELIOS_REPARTITION_UPDATE_ID").map(String::as_str) == Some(update_id)
                    && let Some(status) = values.get("HELIOS_REPARTITION_STATUS")
                {
                    Ok(Some(RepartitionResult { update_id: update_id.to_string(), status: status.clone() }))
                } else {
                    Ok(None)
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(UpdateExecuteError::Io(error)),
        }
    }

    fn validate_repartition_request(&self, slot_action: &SlotAction, slots: &SlotLayout) -> Result<(), UpdateExecuteError> {
        if matches!(slot_action, SlotAction::None) {
            return Ok(());
        }

        let manifest = toml::from_str::<RepartitionLayoutManifest>(&fs::read_to_string(&self.system_layout_manifest_path)?)
            .map_err(|error| UpdateExecuteError::InvalidRepartitionRequest { message: format!("failed to parse storage layout manifest {}: {error}", self.system_layout_manifest_path.display()) })?;

        if !manifest.live_repartition.allow_destructive_data_borrow {
            return Err(UpdateExecuteError::InvalidRepartitionRequest { message: "storage layout manifest does not allow destructive DATA borrow for live repartition".into() });
        }

        let has_slot_b = manifest.partitions.iter().any(|partition| partition.role.as_deref() == Some("slot_b"));
        let has_data = manifest.partitions.iter().any(|partition| partition.role.as_deref() == Some("data"));

        if !has_slot_b || !has_data {
            return Err(UpdateExecuteError::InvalidRepartitionRequest { message: "storage layout manifest must define slot_b and data partitions".into() });
        }

        let slot_b = manifest
            .partitions
            .iter()
            .find(|partition| partition.role.as_deref() == Some("slot_b"))
            .ok_or(UpdateExecuteError::InvalidRepartitionRequest { message: "storage layout manifest must define slot_b partition".into() })?;

        if slot_b.fill_to_end.unwrap_or(false) {
            return Err(UpdateExecuteError::InvalidRepartitionRequest { message: "slot_b cannot use fill_to_end sizing for live repartition".into() });
        }

        let projected_inactive_size_bytes = projected_slot_b_size_bytes(slot_b, slots)?;
        let required_size_bytes = match slot_action {
            SlotAction::None => 0,
            SlotAction::CreateInactive => {
                slots.required_size_bytes.ok_or(UpdateExecuteError::InvalidRepartitionRequest { message: "unable to determine required inactive-slot size from the active slot".into() })?
            }
            SlotAction::ResizeInactive { required_size_bytes } => *required_size_bytes,
        };

        if projected_inactive_size_bytes < required_size_bytes {
            return Err(UpdateExecuteError::InvalidRepartitionRequest {
                message: format!("storage layout can only produce inactive slot size {} bytes but update requires {} bytes", projected_inactive_size_bytes, required_size_bytes),
            });
        }

        Ok(())
    }

    fn rollback_payload_targets(&self, activated_targets: &[String]) {
        let restart_services = systemd_available();
        for name in activated_targets.iter().rev() {
            if self.release_manager.rollback(name).is_ok() {
                let _ = self.restart_or_defer_service(name, restart_services);
            }
        }
    }

    fn restart_or_defer_service(&self, name: &str, restart_services: bool) -> Result<(), UpdateExecuteError> {
        if !restart_services {
            return Ok(());
        }
        if name == UPDATER_SERVICE_NAME {
            self.enqueue_action(PendingAction::RestartService { name: name.to_string() })?;
            return Ok(());
        }
        self.release_manager.restart(name)?;
        Ok(())
    }

    pub fn run_pending_actions(&self) -> Result<(), UpdateExecuteError> {
        let dir = self.state_dir.join(ACTIONS_DIR);
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(UpdateExecuteError::Io(error)),
        };

        let mut action_paths = entries.filter_map(|entry| entry.ok().map(|entry| entry.path())).filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json")).collect::<Vec<_>>();
        action_paths.sort();

        for path in action_paths {
            let action = serde_json::from_slice::<PendingAction>(&fs::read(&path)?)?;
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(UpdateExecuteError::Io(error)),
            }

            match action {
                PendingAction::RestartService { name } => self.release_manager.restart(&name)?,
            }
        }

        Ok(())
    }

    fn enqueue_action(&self, action: PendingAction) -> Result<(), UpdateExecuteError> {
        let dir = self.state_dir.join(ACTIONS_DIR);
        fs::create_dir_all(&dir)?;
        let id = match &action {
            PendingAction::RestartService { name } => format!("restart-{}.json", sanitize_component(name)),
        };
        fs::write(dir.join(id), serde_json::to_vec_pretty(&action)?)?;
        Ok(())
    }

    fn run_manifest_hooks(&self, workload_id: &str, manifest: &UpdateManifest, phase: HookPhase) -> Result<(), UpdateExecuteError> {
        for (index, hook) in manifest.hooks.iter().enumerate().filter(|(_, hook)| hook.phase == phase) {
            let (program, args) = hook.command.split_first().ok_or(UpdateExecuteError::MissingHookCommand { phase: phase.clone() })?;
            let timeout_secs = hook.timeout_secs.unwrap_or(self.default_hook_timeout_secs);
            let (stdout_path, stderr_path) = self.hook_log_paths(workload_id, &phase, index)?;
            let envs = hook_env(workload_id, manifest, &phase);
            let result = run_hook_command(program, args, &envs, Duration::from_secs(timeout_secs), &stdout_path, &stderr_path)?;
            if !result.success {
                let message = if result.timed_out {
                    format!("timed out after {timeout_secs}s; stdout={} stderr={}", stdout_path.display(), stderr_path.display())
                } else if !result.stderr_summary.is_empty() {
                    format!("{}; stdout={} stderr={}", result.stderr_summary, stdout_path.display(), stderr_path.display())
                } else if !result.stdout_summary.is_empty() {
                    format!("{}; stdout={} stderr={}", result.stdout_summary, stdout_path.display(), stderr_path.display())
                } else {
                    format!("exit status {}; stdout={} stderr={}", result.status, stdout_path.display(), stderr_path.display())
                };
                return Err(UpdateExecuteError::HookFailed { phase: phase.clone(), message });
            }
        }
        Ok(())
    }

    fn hook_log_paths(&self, workload_id: &str, phase: &HookPhase, index: usize) -> Result<(PathBuf, PathBuf), UpdateExecuteError> {
        let dir = self.state_dir.join(HOOKS_DIR).join(sanitize_component(workload_id));
        fs::create_dir_all(&dir)?;
        let stem = format!("{}-{}", hook_phase_name(phase), index);
        Ok((dir.join(format!("{stem}.stdout")), dir.join(format!("{stem}.stderr"))))
    }

    fn current_selector(&self, slots: &SlotLayout) -> Result<String, UpdateExecuteError> {
        match slots.scheme.as_str() {
            "squashfs_ab" => Ok(slots.active_name.clone()),
            scheme => Err(UpdateExecuteError::UnsupportedSlotScheme { scheme: scheme.to_string() }),
        }
    }

    fn boot_selector(&self, slots: &SlotLayout, inactive_name: &str) -> Result<String, UpdateExecuteError> {
        match slots.scheme.as_str() {
            "squashfs_ab" => Ok(inactive_name.to_string()),
            scheme => Err(UpdateExecuteError::UnsupportedSlotScheme { scheme: scheme.to_string() }),
        }
    }

    fn install_boot_assets(&self, slots: &SlotLayout, assets: &[StagedBootAsset]) -> Result<(), UpdateExecuteError> {
        if assets.is_empty() {
            return Ok(());
        }
        self.with_boot_paths(slots, |boot_mount_dir, _| {
            if !boot_mount_dir.exists() {
                return Err(UpdateExecuteError::MissingBootMount { path: boot_mount_dir.to_path_buf() });
            }
            let backup_dir = TempDirBuilder::new().prefix("helios-updater-boot-backup-").tempdir_in("/tmp")?;
            let mut backups = Vec::with_capacity(assets.len());
            for asset in assets {
                let dest = boot_asset_dest(boot_mount_dir, &asset.relative_path)?;
                if dest.exists() && same_file_contents(&dest, &asset.staged_path)? {
                    continue;
                }

                let backup = if dest.exists() {
                    let backup = backup_dir.path().join(format!("asset-{}", backups.len()));
                    fs::copy(&dest, &backup)?;
                    Some(backup)
                } else {
                    None
                };
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                backups.push((dest.clone(), backup));
                if let Err(error) = fs::copy(&asset.staged_path, &dest) {
                    rollback_boot_assets(&backups)?;
                    return Err(UpdateExecuteError::Io(error));
                }
            }
            Ok(())
        })
    }

    fn write_file(&self, path: PathBuf, contents: &str) -> Result<(), UpdateExecuteError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, format!("{contents}\n"))?;
        Ok(())
    }

    fn write_confirm_request(&self, slots: &SlotLayout, state: &PreparedState) -> Result<(), UpdateExecuteError> {
        let body = format!("HELIOS_UPDATE_ID={}\nHELIOS_UPDATE_WORKLOAD_ID={}\nHELIOS_UPDATE_EXPECTED_SELECTOR={}", state.update_id, state.workload_id, state.expected_selector);
        self.write_file(self.local_ota_dir.join(CONFIRM_REQUEST_FILE), &body)?;
        self.with_boot_paths(slots, |_, boot_ota_dir| {
            self.write_file(boot_ota_dir.join(CONFIRM_REQUEST_FILE), &body)?;
            Ok(())
        })?;
        Ok(())
    }

    fn write_boot_cmdline_root(&self, slots: &SlotLayout, root_value: &str) -> Result<(), UpdateExecuteError> {
        self.with_boot_paths(slots, |boot_mount_dir, _| {
            let cmdline_path = boot_mount_dir.join("cmdline.txt");
            let existing = fs::read_to_string(&cmdline_path)?;
            let mut fields = Vec::new();
            let mut replaced = false;
            for field in existing.split_whitespace() {
                if field.starts_with("root=") {
                    fields.push(format!("root={root_value}"));
                    replaced = true;
                } else {
                    fields.push(field.to_string());
                }
            }
            if !replaced {
                fields.push(format!("root={root_value}"));
            }
            fs::write(cmdline_path, format!("{}\n", fields.join(" ")))?;
            Ok(())
        })
    }

    fn with_boot_paths<T, F>(&self, slots: &SlotLayout, op: F) -> Result<T, UpdateExecuteError>
    where
        F: FnOnce(&Path, &Path) -> Result<T, UpdateExecuteError>,
    {
        if let Some(boot_device) = self.boot_device_for_slots(slots)? {
            let mount_root = TempDirBuilder::new().prefix("helios-updater-boot-").tempdir_in("/tmp")?;
            mount_filesystem(&boot_device, mount_root.path(), "vfat", true)?;
            let result = op(mount_root.path(), &mount_root.path().join("helios/ota"));
            let sync_result = sync_mountpoint(mount_root.path());
            let unmount_result = unmount_filesystem(mount_root.path());
            match (result, unmount_result) {
                (Ok(value), Ok(())) => {
                    sync_result?;
                    Ok(value)
                }
                (Err(error), Ok(())) => Err(error),
                (Ok(_), Err(error)) => Err(error),
                (Err(error), Err(_)) => Err(error),
            }
        } else {
            op(&self.boot_mount_dir, &self.boot_ota_dir)
        }
    }

    fn boot_device_for_slots(&self, slots: &SlotLayout) -> Result<Option<PathBuf>, UpdateExecuteError> {
        let Some(active_device) = slots.active_device.as_ref() else {
            return Ok(None);
        };
        let metadata = match fs::metadata(active_device) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(UpdateExecuteError::Io(error)),
        };
        if !metadata.file_type().is_block_device() {
            return Ok(None);
        }

        let boot_partition = read_boot_partition(&self.local_ota_dir.join("../storage-layout.env")).or_else(|_| read_boot_partition(Path::new("/etc/helios/storage-layout.env"))).unwrap_or(1);
        let base = partition_base(active_device);
        Ok(Some(partition_device(&base, boot_partition)))
    }

    fn write_bytes(&self, path: PathBuf, contents: &[u8]) -> Result<(), UpdateExecuteError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)?;
        Ok(())
    }

    fn request_reboot(&self) -> Result<(), UpdateExecuteError> {
        let output = Command::new(&self.reboot_program).arg("reboot").output()?;
        if output.status.success() {
            return Ok(());
        }

        let mut messages = vec![format_command_failure(format!("{} reboot", self.reboot_program.display()), &output)];
        let _ = Command::new("sync").status();

        for (program, args) in [("/bin/busybox", &["reboot", "-f"][..]), ("reboot", &["-f"][..])] {
            match Command::new(program).args(args).output() {
                Ok(output) if output.status.success() => return Ok(()),
                Ok(output) => messages.push(format_command_failure(format!("{program} {}", args.join(" ")), &output)),
                Err(error) => messages.push(format!("{program} {}: {error}", args.join(" "))),
            }
        }

        match fs::write("/proc/sysrq-trigger", "b") {
            Ok(()) => Ok(()),
            Err(error) => {
                messages.push(format!("/proc/sysrq-trigger b: {error}"));
                Err(UpdateExecuteError::CommandFailed { command: "request reboot".into(), message: messages.join("; ") })
            }
        }
    }

    fn clear_confirm_metadata(&self) -> Result<(), UpdateExecuteError> {
        for path in
            [self.local_ota_dir.join(CONFIRM_REQUEST_FILE), self.local_ota_dir.join(CONFIRM_RESULT_FILE), self.boot_ota_dir.join(CONFIRM_REQUEST_FILE), self.boot_ota_dir.join(CONFIRM_RESULT_FILE)]
        {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(UpdateExecuteError::Io(error)),
            }
        }
        Ok(())
    }

    fn clear_repartition_metadata(&self) -> Result<(), UpdateExecuteError> {
        for path in [
            self.local_ota_dir.join("repartition-request.env"),
            self.local_ota_dir.join("repartition-result.env"),
            self.local_ota_dir.join("repartition-layout.toml"),
            self.boot_ota_dir.join("repartition-request.env"),
            self.boot_ota_dir.join("repartition-result.env"),
            self.boot_ota_dir.join("repartition-layout.toml"),
        ] {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(UpdateExecuteError::Io(error)),
            }
        }
        Ok(())
    }
}

fn read_boot_partition(path: &Path) -> Result<u32, UpdateExecuteError> {
    let contents = fs::read_to_string(path)?;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() != "HELIOS_LAYOUT_BOOT_PARTITION" {
            continue;
        }
        let value = value.trim().trim_matches('\'').trim_matches('"');
        return value
            .parse::<u32>()
            .map_err(|error| UpdateExecuteError::CommandFailed { command: format!("parse {}", path.display()), message: format!("invalid HELIOS_LAYOUT_BOOT_PARTITION {value}: {error}") });
    }
    Err(UpdateExecuteError::CommandFailed { command: format!("parse {}", path.display()), message: "missing HELIOS_LAYOUT_BOOT_PARTITION".into() })
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

fn mount_filesystem(device: &Path, mountpoint: &Path, fs_type: &str, rw: bool) -> Result<(), UpdateExecuteError> {
    let options = if rw { "rw" } else { "ro" };
    let output = Command::new("mount").arg("-t").arg(fs_type).arg("-o").arg(options).arg(device).arg(mountpoint).output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(UpdateExecuteError::CommandFailed { command: format!("mount {}", device.display()), message: String::from_utf8_lossy(&output.stderr).trim().to_string() })
    }
}

fn unmount_filesystem(mountpoint: &Path) -> Result<(), UpdateExecuteError> {
    let output = Command::new("umount").arg(mountpoint).output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(UpdateExecuteError::CommandFailed { command: format!("umount {}", mountpoint.display()), message: String::from_utf8_lossy(&output.stderr).trim().to_string() })
    }
}

fn sync_mountpoint(mountpoint: &Path) -> Result<(), UpdateExecuteError> {
    let output = Command::new("sync").arg(mountpoint).output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(UpdateExecuteError::CommandFailed { command: format!("sync {}", mountpoint.display()), message: String::from_utf8_lossy(&output.stderr).trim().to_string() })
    }
}

fn swap_live_repartition_slots(layout: &mut toml::Value) -> Result<(), UpdateExecuteError> {
    let partitions = layout
        .get_mut("partitions")
        .and_then(toml::Value::as_array_mut)
        .ok_or(UpdateExecuteError::InvalidRepartitionRequest { message: "storage layout manifest is missing [[partitions]]".into() })?;

    let slot_a_index = partitions
        .iter()
        .position(|partition| partition.get("role").and_then(toml::Value::as_str) == Some("slot_a"))
        .ok_or(UpdateExecuteError::InvalidRepartitionRequest { message: "storage layout manifest is missing slot_a partition".into() })?;
    let slot_b_index = partitions
        .iter()
        .position(|partition| partition.get("role").and_then(toml::Value::as_str) == Some("slot_b"))
        .ok_or(UpdateExecuteError::InvalidRepartitionRequest { message: "storage layout manifest is missing slot_b partition".into() })?;

    let slot_b_template = partitions[slot_b_index].clone();
    let (slot_a, slot_b) = if slot_a_index < slot_b_index {
        let (left, right) = partitions.split_at_mut(slot_b_index);
        (&mut left[slot_a_index], &mut right[0])
    } else {
        let (left, right) = partitions.split_at_mut(slot_a_index);
        (&mut right[0], &mut left[slot_b_index])
    };
    let slot_a = slot_a.as_table_mut().ok_or(UpdateExecuteError::InvalidRepartitionRequest { message: "slot_a partition entry is not a table".into() })?;
    let slot_b = slot_b.as_table_mut().ok_or(UpdateExecuteError::InvalidRepartitionRequest { message: "slot_b partition entry is not a table".into() })?;

    slot_a.insert("mode".into(), toml::Value::String("mkpart".into()));
    for key in ["fs_type", "marker", "size_source", "mkfs", "wipe_signatures", "run_fsck", "run_resizefs"] {
        if let Some(value) = slot_b_template.get(key).cloned() {
            slot_a.insert(key.into(), value);
        }
    }

    slot_b.insert("mode".into(), toml::Value::String("noop".into()));
    slot_b.remove("marker");
    slot_b.insert("mkfs".into(), toml::Value::Boolean(false));
    slot_b.insert("wipe_signatures".into(), toml::Value::Boolean(false));
    slot_b.insert("run_fsck".into(), toml::Value::Boolean(false));
    slot_b.insert("run_resizefs".into(), toml::Value::Boolean(false));

    Ok(())
}

struct HookExecutionResult {
    status: String,
    success: bool,
    timed_out: bool,
    stdout_summary: String,
    stderr_summary: String,
}

fn run_hook_command(program: &str, args: &[String], envs: &[(&str, String)], timeout: Duration, stdout_path: &Path, stderr_path: &Path) -> Result<HookExecutionResult, UpdateExecuteError> {
    let mut command = Command::new(program);
    command.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (key, value) in envs {
        command.env(key, value);
    }

    let mut child = command.spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| UpdateExecuteError::CommandFailed { command: program.into(), message: "hook stdout unavailable".into() })?;
    let stderr = child.stderr.take().ok_or_else(|| UpdateExecuteError::CommandFailed { command: program.into(), message: "hook stderr unavailable".into() })?;

    let stdout_handle = thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut reader = BufReader::new(stdout);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(bytes)
    });
    let stderr_handle = thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut reader = BufReader::new(stderr);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(bytes)
    });

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            child.kill()?;
            break child.wait()?;
        }
        thread::sleep(Duration::from_millis(50));
    };

    let stdout_bytes = stdout_handle.join().map_err(|_| UpdateExecuteError::CommandFailed { command: program.into(), message: "hook stdout reader thread panicked".into() })??;
    let stderr_bytes = stderr_handle.join().map_err(|_| UpdateExecuteError::CommandFailed { command: program.into(), message: "hook stderr reader thread panicked".into() })??;

    fs::write(stdout_path, &stdout_bytes)?;
    fs::write(stderr_path, &stderr_bytes)?;

    Ok(HookExecutionResult {
        status: status.to_string(),
        success: status.success() && !timed_out,
        timed_out,
        stdout_summary: summarize_hook_output(&stdout_bytes),
        stderr_summary: summarize_hook_output(&stderr_bytes),
    })
}

fn summarize_hook_output(bytes: &[u8]) -> String {
    const MAX_SUMMARY_CHARS: usize = 240;

    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut summary = trimmed.chars().take(MAX_SUMMARY_CHARS).collect::<String>();
    if trimmed.chars().count() > MAX_SUMMARY_CHARS {
        summary.push_str("...");
    }
    summary.replace('\n', " ")
}

fn hook_phase_name(phase: &HookPhase) -> &'static str {
    match phase {
        HookPhase::Preinstall => "preinstall",
        HookPhase::Preswitch => "preswitch",
        HookPhase::Postboot => "postboot",
        HookPhase::RollbackCleanup => "rollback-cleanup",
    }
}

fn hook_env(workload_id: &str, manifest: &UpdateManifest, phase: &HookPhase) -> [(&'static str, String); 5] {
    [
        ("HELIOS_UPDATE_WORKLOAD_ID", workload_id.to_string()),
        ("HELIOS_UPDATE_ARTIFACT_ID", manifest.artifact_id.clone()),
        ("HELIOS_UPDATE_VERSION", manifest.version.clone()),
        ("HELIOS_UPDATE_CLASS", manifest.artifact_class.as_str().to_string()),
        ("HELIOS_UPDATE_PHASE", hook_phase_name(phase).to_string()),
    ]
}

fn boot_asset_dest(boot_mount_dir: &Path, relative_path: &Path) -> Result<PathBuf, UpdateExecuteError> {
    if relative_path.is_absolute() || relative_path.components().any(|component| matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))) {
        return Err(UpdateExecuteError::InvalidBootAssetPath { path: relative_path.to_path_buf() });
    }
    Ok(boot_mount_dir.join(relative_path))
}

fn format_command_failure(command: String, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let message = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("exit status {}", output.status)
    };
    format!("{command}: {message}")
}

fn same_file_contents(left: &Path, right: &Path) -> Result<bool, UpdateExecuteError> {
    let left_metadata = fs::metadata(left)?;
    let right_metadata = fs::metadata(right)?;
    if left_metadata.len() != right_metadata.len() {
        return Ok(false);
    }

    let mut left = BufReader::new(fs::File::open(left)?);
    let mut right = BufReader::new(fs::File::open(right)?);
    let mut left_buffer = [0_u8; 64 * 1024];
    let mut right_buffer = [0_u8; 64 * 1024];
    loop {
        let left_read = left.read(&mut left_buffer)?;
        let right_read = right.read(&mut right_buffer)?;
        if left_read != right_read {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
        if left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
    }
}

fn rollback_boot_assets(backups: &[(PathBuf, Option<PathBuf>)]) -> Result<(), UpdateExecuteError> {
    for (dest, backup) in backups.iter().rev() {
        match backup {
            Some(backup) => {
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(backup, dest)?;
            }
            None => remove_file_ok(dest)?,
        }
    }
    Ok(())
}

fn validate_written_rootfs(slots: &SlotLayout, target: &Path) -> Result<(), UpdateExecuteError> {
    if slots.scheme != "squashfs_ab" {
        return Ok(());
    }
    let metadata = fs::metadata(target)?;
    if !metadata.file_type().is_block_device() {
        return Ok(());
    }
    let mount_root = TempDirBuilder::new().prefix("helios-updater-rootfs-verify-").tempdir_in("/tmp")?;
    mount_filesystem(target, mount_root.path(), "squashfs", false)?;
    unmount_filesystem(mount_root.path())
}

fn write_image_to_device(source: &Path, target: &Path) -> Result<(), UpdateExecuteError> {
    let source_name = source.to_string_lossy();
    let is_xz = is_xz_file(source)?;
    let partition_range = rootfs_partition_range(source, is_xz)?;
    if is_xz {
        return write_xz_image_to_device(source, target, partition_range);
    }

    if source_name.ends_with(".img")
        || source_name.ends_with(".bin")
        || source_name.ends_with(".squashfs")
        || source_name.ends_with(".sqfs")
        || source_name.ends_with(".sfs")
        || source.extension().is_none()
    {
        let mut file = fs::File::open(source)?;
        let target_file = fs::File::create(target)?;
        let mut writer = BufWriter::new(target_file);
        if let Some(range) = partition_range {
            file.seek(SeekFrom::Start(range.offset_bytes))?;
            copy_exact_bytes(&mut file.take(range.size_bytes), &mut writer, range.size_bytes)?;
        } else {
            std::io::copy(&mut file, &mut writer)?;
        }
        writer.flush()?;
        writer.get_ref().sync_all()?;
        return Ok(());
    }

    Err(UpdateExecuteError::UnsupportedImageFormat { path: source.to_path_buf() })
}

fn write_xz_image_to_device(source: &Path, target: &Path, range: Option<ImagePartitionRange>) -> Result<(), UpdateExecuteError> {
    let mut child = Command::new("xz").arg("-dc").arg(source).stdout(Stdio::piped()).spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| UpdateExecuteError::CommandFailed { command: "xz -dc".into(), message: "stdout unavailable".into() })?;
    let mut reader = BufReader::new(stdout);
    let target_file = fs::File::create(target)?;
    let mut writer = BufWriter::new(target_file);

    if let Some(range) = range {
        copy_exact_bytes(&mut reader.by_ref(), &mut std::io::sink(), range.offset_bytes)?;
        copy_exact_bytes(&mut reader.take(range.size_bytes), &mut writer, range.size_bytes)?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        if child.try_wait()?.is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
        return Ok(());
    }

    std::io::copy(&mut reader, &mut writer)?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    let status = child.wait()?;
    if !status.success() {
        return Err(UpdateExecuteError::CommandFailed { command: "xz -dc".into(), message: format!("exit status {status}") });
    }
    Ok(())
}

fn copy_exact_bytes(reader: &mut dyn Read, writer: &mut dyn Write, expected: u64) -> Result<(), UpdateExecuteError> {
    let mut remaining = expected;
    let mut buffer = [0_u8; 64 * 1024];
    while remaining > 0 {
        let limit = buffer.len().min(remaining as usize);
        let read = reader.read(&mut buffer[..limit])?;
        if read == 0 {
            return Err(UpdateExecuteError::CommandFailed { command: "copy image range".into(), message: format!("expected {expected} bytes, missing {remaining}") });
        }
        writer.write_all(&buffer[..read])?;
        remaining -= read as u64;
    }
    Ok(())
}

fn rootfs_partition_range(source: &Path, is_xz: bool) -> Result<Option<ImagePartitionRange>, UpdateExecuteError> {
    let mut header = [0_u8; 512];
    if is_xz {
        let mut child = Command::new("xz").arg("-dc").arg(source).stdout(Stdio::piped()).spawn()?;
        let mut stdout = child.stdout.take().ok_or_else(|| UpdateExecuteError::CommandFailed { command: "xz -dc".into(), message: "stdout unavailable".into() })?;
        let mut read_total = 0usize;
        while read_total < header.len() {
            let read = stdout.read(&mut header[read_total..])?;
            if read == 0 {
                break;
            }
            read_total += read;
        }
        if child.try_wait()?.is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
        if read_total < header.len() {
            return Ok(None);
        }
    } else {
        let mut file = fs::File::open(source)?;
        let read = file.read(&mut header)?;
        if read < header.len() {
            return Ok(None);
        }
    }

    Ok(mbr_partition_range(&header, ROOTFS_IMAGE_PARTITION))
}

fn is_xz_file(path: &Path) -> Result<bool, UpdateExecuteError> {
    let mut file = fs::File::open(path)?;
    let mut magic = [0_u8; 6];
    let read = file.read(&mut magic)?;
    Ok(read == magic.len() && &magic == XZ_MAGIC)
}

fn mbr_partition_range(header: &[u8; 512], partition_number: usize) -> Option<ImagePartitionRange> {
    if header[510] != 0x55 || header[511] != 0xaa || !(1..=4).contains(&partition_number) {
        return None;
    }
    let offset = 446 + (partition_number - 1) * 16;
    let partition_type = header[offset + 4];
    let start_lba = u32::from_le_bytes(header[offset + 8..offset + 12].try_into().ok()?) as u64;
    let sector_count = u32::from_le_bytes(header[offset + 12..offset + 16].try_into().ok()?) as u64;
    if partition_type == 0 || sector_count == 0 {
        return None;
    }
    Some(ImagePartitionRange { offset_bytes: start_lba.saturating_mul(SECTOR_BYTES), size_bytes: sector_count.saturating_mul(SECTOR_BYTES) })
}

fn current_boot_id() -> Result<String, UpdateExecuteError> {
    Ok(fs::read_to_string("/proc/sys/kernel/random/boot_id")?.trim().to_string())
}

fn overlay_upperdir_from_mounts(mounts: &str) -> Option<PathBuf> {
    mounts.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let _source = fields.next()?;
        let target = fields.next()?;
        let fs_type = fields.next()?;
        let options = fields.next()?;
        if target != "/" || fs_type != "overlay" {
            return None;
        }
        options.split(',').find_map(|entry| entry.strip_prefix("upperdir=")).map(PathBuf::from)
    })
}

fn scrub_overlay_image_overrides(upperdir: &Path, paths_file: &Path) -> Result<(), UpdateExecuteError> {
    for relative in image_owned_overlay_paths(paths_file)? {
        let candidate = upperdir.join(relative.trim_start_matches('/'));
        match fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
                fs::remove_file(&candidate)?;
                prune_empty_parent_dirs(&candidate, upperdir)?;
            }
            Ok(metadata) if metadata.is_dir() => {
                fs::remove_dir_all(&candidate)?;
                prune_empty_parent_dirs(&candidate, upperdir)?;
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(UpdateExecuteError::Io(error)),
        }
    }
    Ok(())
}

fn image_owned_overlay_paths(path: &Path) -> Result<Vec<String>, UpdateExecuteError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(UpdateExecuteError::Io(error)),
    };

    Ok(contents.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')).map(str::to_string).collect())
}

fn prune_empty_parent_dirs(path: &Path, stop_at: &Path) -> Result<(), UpdateExecuteError> {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir == stop_at {
            break;
        }
        match fs::remove_dir(dir) {
            Ok(()) => current = dir.parent(),
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(UpdateExecuteError::Io(error)),
        }
    }
    Ok(())
}

fn canonicalize_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn remove_file_ok(path: &Path) -> Result<(), UpdateExecuteError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(UpdateExecuteError::Io(error)),
    }
}

fn squashfs_reserve_name(active_name: &str) -> Result<&'static str, UpdateExecuteError> {
    match active_name {
        "ROOT_A" => Ok("ROOT_B"),
        "ROOT_B" => Ok("ROOT_A"),
        other => Err(UpdateExecuteError::UnsupportedSlotScheme { scheme: format!("unknown squashfs slot name {other}") }),
    }
}

fn projected_slot_b_size_bytes(slot_b: &RepartitionLayoutPartition, slots: &SlotLayout) -> Result<u64, UpdateExecuteError> {
    const MIB: u64 = 1024 * 1024;

    let projected = match slot_b.size_source {
        Some(RepartitionSizeSource::MirrorExisting) => {
            slots.required_size_bytes.ok_or(UpdateExecuteError::InvalidRepartitionRequest { message: "slot_b uses mirror_existing sizing but active slot size is unknown".into() })?
        }
        None => slot_b
            .size_mib
            .map(|size| size.saturating_mul(MIB))
            .ok_or(UpdateExecuteError::InvalidRepartitionRequest { message: "slot_b must declare size_mib or size_source for live repartition sizing".into() })?,
    };

    let projected = if let Some(max_mib) = slot_b.max_mib { projected.min(max_mib.saturating_mul(MIB)) } else { projected };

    Ok(projected)
}

fn sanitize_component(value: &str) -> String {
    value.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' }).collect()
}

fn slot_action_label(action: &SlotAction) -> &'static str {
    match action {
        SlotAction::None => "none",
        SlotAction::CreateInactive => "create_inactive",
        SlotAction::ResizeInactive { .. } => "resize_inactive",
    }
}

fn systemd_available() -> bool {
    Path::new("/run/systemd/system").exists()
}

fn parse_shell_env(src: &str) -> std::collections::BTreeMap<String, String> {
    src.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let (key, value) = trimmed.split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

pub fn boot_uptime_secs() -> Result<u64, UpdateExecuteError> {
    let uptime = fs::read_to_string("/proc/uptime")?;
    let seconds = uptime.split_whitespace().next().unwrap_or("0").split('.').next().unwrap_or("0").parse::<u64>().unwrap_or(0);
    Ok(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        manifest::{CompatibilityRule, OsImagePayload, PayloadUpdatePayload, UpdatePayload},
        model::{SlotLayout, UpdateArtifactClass, UpdateSlot},
        staging::{StagedManifest, StagedPayloadTarget},
    };
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn applies_raw_os_image_to_inactive_target_file() {
        let temp = tempdir().expect("tempdir");
        let image = temp.path().join("image.img");
        let target = temp.path().join("target.img");
        fs::write(&image, b"raw-image").expect("image");

        let config =
            UpdaterConfig { updater_state_dir: temp.path().join("state"), service_releases_dir: temp.path().join("releases"), service_bin_dir: temp.path().join("bin"), ..UpdaterConfig::default() };
        let executor = UpdateExecutor {
            state_dir: config.updater_state_dir.clone(),
            boot_mount_dir: temp.path().join("boot"),
            boot_ota_dir: temp.path().join("boot-ota"),
            local_ota_dir: temp.path().join("local-ota"),
            system_layout_manifest_path: temp.path().join("layout.toml"),
            reboot_program: temp.path().join("fake-systemctl"),
            release_manager: ServiceReleaseManager::new(config.service_releases_dir.clone(), config.service_bin_dir.clone()),
            default_hook_timeout_secs: crate::config::DEFAULT_HOOK_TIMEOUT_SECS,
        };
        fs::write(&executor.system_layout_manifest_path, b"schema_version = 1\n").expect("layout");
        fs::write(&executor.reboot_program, "#!/bin/sh\nexit 0\n").expect("fake reboot");
        fs::create_dir_all(&executor.boot_mount_dir).expect("boot dir");
        fs::write(executor.boot_mount_dir.join("cmdline.txt"), "console=tty1 root=/dev/helios-rootfs rootfstype=squashfs ro\n").expect("boot cmdline");
        let mut perms = fs::metadata(&executor.reboot_program).expect("meta").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&executor.reboot_program, perms).expect("chmod");

        let manifest = UpdateManifest {
            manifest_version: "v1".into(),
            artifact_id: "artifact.os".into(),
            version: "2026.2.0".into(),
            artifact_class: UpdateArtifactClass::OsImage,
            compatibility: CompatibilityRule { requires_ab_rootfs: true, ..CompatibilityRule::default() },
            payload: UpdatePayload::OsImage(OsImagePayload { image_url: "file:///tmp/image.img".into(), size_bytes: None, sha256: None, inactive_slot_min_bytes: None, boot_assets: vec![] }),
            hooks: vec![],
        };
        let boot_asset_source = temp.path().join("staged-config.txt");
        fs::write(&boot_asset_source, b"boot-config").expect("boot asset");
        let staged = StagedManifest {
            artifact_id: manifest.artifact_id.clone(),
            artifact: StagedArtifact::OsImage { path: image.clone(), boot_assets: vec![StagedBootAsset { relative_path: PathBuf::from("config.txt"), staged_path: boot_asset_source }] },
        };
        let slots = SlotLayout {
            scheme: "squashfs_ab".into(),
            active: UpdateSlot::A,
            active_name: "ROOT_A".into(),
            active_label: None,
            active_device: Some(temp.path().join("active.img")),
            inactive: Some(UpdateSlot::B),
            inactive_name: Some("ROOT_B".into()),
            inactive_label: None,
            inactive_device: Some(target.clone()),
            inactive_exists: true,
            inactive_size_bytes: Some(0),
            required_size_bytes: Some(0),
        };

        let outcome = executor.apply_staged("update.node-local.1", &manifest, &slots, &staged).expect("apply");
        let message = match outcome {
            ApplyOutcome::BootPrepared { message } => message,
            ApplyOutcome::Completed { .. } => panic!("expected boot preparation"),
        };
        assert!(message.contains("prepared inactive slot ROOT_B"));
        assert_eq!(fs::read(&target).expect("target"), b"raw-image");
        assert_eq!(fs::read(temp.path().join("boot/config.txt")).expect("boot config"), b"boot-config");
        let boot_cmdline = fs::read_to_string(temp.path().join("boot/cmdline.txt")).expect("boot cmdline");
        assert!(boot_cmdline.contains("root=/dev/helios-rootfs"));
        assert_eq!(fs::read_to_string(temp.path().join("local-ota/pending")).expect("pending"), "ROOT_B\n");
        assert!(executor.is_prepared("update.node-local.1"));
    }

    #[test]
    fn writes_rootfs_partition_from_full_disk_image_to_inactive_target_file() {
        let temp = tempdir().expect("tempdir");
        let image = temp.path().join("disk.img");
        let target = temp.path().join("target.img");
        fs::write(&image, test_disk_image()).expect("image");

        write_image_to_device(&image, &target).expect("write image");

        assert_eq!(fs::read(&target).expect("target"), vec![0x5a; 1536]);
    }

    #[test]
    fn writes_rootfs_partition_from_staged_xz_image_without_extension() {
        let temp = tempdir().expect("tempdir");
        let raw = temp.path().join("disk.img");
        let staged = temp.path().join("image");
        let target = temp.path().join("target.img");
        fs::write(&raw, test_disk_image()).expect("image");
        let output = Command::new("xz").arg("-c").arg(&raw).output().expect("xz");
        assert!(output.status.success(), "xz failed: {}", String::from_utf8_lossy(&output.stderr));
        fs::write(&staged, output.stdout).expect("staged");

        write_image_to_device(&staged, &target).expect("write image");

        assert_eq!(fs::read(&target).expect("target"), vec![0x5a; 1536]);
    }

    #[test]
    fn records_terminal_failure_state_for_later_recovery() {
        let temp = tempdir().expect("tempdir");
        let executor = UpdateExecutor {
            state_dir: temp.path().join("state"),
            boot_mount_dir: temp.path().join("boot"),
            boot_ota_dir: temp.path().join("boot-ota"),
            local_ota_dir: temp.path().join("local-ota"),
            system_layout_manifest_path: temp.path().join("layout.toml"),
            reboot_program: temp.path().join("fake-systemctl"),
            release_manager: ServiceReleaseManager::new(temp.path().join("releases"), temp.path().join("bin")),
            default_hook_timeout_secs: crate::config::DEFAULT_HOOK_TIMEOUT_SECS,
        };

        executor.record_failed("update.node-local.1", "apply failed: inactive slot write failed").expect("record failed");
        let failed = executor.failed_state("update.node-local.1").expect("failed state").expect("failed marker");

        assert_eq!(failed.workload_id, "update.node-local.1");
        assert_eq!(failed.message, "apply failed: inactive slot write failed");
    }

    #[test]
    fn boot_asset_paths_cannot_escape_boot_mount() {
        let temp = tempdir().expect("tempdir");

        assert!(boot_asset_dest(temp.path(), Path::new("kernel8.img")).is_ok());
        assert!(matches!(boot_asset_dest(temp.path(), Path::new("../kernel8.img")), Err(UpdateExecuteError::InvalidBootAssetPath { .. })));
        assert!(matches!(boot_asset_dest(temp.path(), Path::new("/tmp/kernel8.img")), Err(UpdateExecuteError::InvalidBootAssetPath { .. })));
    }

    #[test]
    fn boot_asset_install_rolls_back_after_copy_failure() {
        let temp = tempdir().expect("tempdir");
        let boot = temp.path().join("boot");
        fs::create_dir_all(&boot).expect("boot");
        fs::write(boot.join("kernel8.img"), b"old-kernel").expect("old kernel");
        let staged_kernel = temp.path().join("new-kernel");
        fs::write(&staged_kernel, b"new-kernel").expect("new kernel");
        let missing_initramfs = temp.path().join("missing-initramfs");
        let executor = UpdateExecutor {
            state_dir: temp.path().join("state"),
            boot_mount_dir: boot.clone(),
            boot_ota_dir: boot.join("helios/ota"),
            local_ota_dir: temp.path().join("local-ota"),
            system_layout_manifest_path: temp.path().join("layout.toml"),
            reboot_program: temp.path().join("fake-systemctl"),
            release_manager: ServiceReleaseManager::new(temp.path().join("releases"), temp.path().join("bin")),
            default_hook_timeout_secs: crate::config::DEFAULT_HOOK_TIMEOUT_SECS,
        };
        let slots = SlotLayout {
            scheme: "squashfs_ab".into(),
            active: UpdateSlot::A,
            active_name: "ROOT_A".into(),
            active_label: None,
            active_device: None,
            inactive: Some(UpdateSlot::B),
            inactive_name: Some("ROOT_B".into()),
            inactive_label: None,
            inactive_device: None,
            inactive_exists: true,
            inactive_size_bytes: Some(0),
            required_size_bytes: Some(0),
        };

        let error = executor
            .install_boot_assets(
                &slots,
                &[
                    StagedBootAsset { relative_path: PathBuf::from("kernel8.img"), staged_path: staged_kernel },
                    StagedBootAsset { relative_path: PathBuf::from("initramfs8"), staged_path: missing_initramfs },
                ],
            )
            .expect_err("missing second asset should fail");

        assert!(matches!(error, UpdateExecuteError::Io(_)));
        assert_eq!(fs::read(boot.join("kernel8.img")).expect("kernel"), b"old-kernel");
        assert!(!boot.join("initramfs8").exists());
    }

    fn test_disk_image() -> Vec<u8> {
        let mut bytes = vec![0_u8; 4096];
        bytes[510] = 0x55;
        bytes[511] = 0xaa;
        let partition_offset = 446 + 16;
        bytes[partition_offset + 4] = 0x83;
        bytes[partition_offset + 8..partition_offset + 12].copy_from_slice(&2_u32.to_le_bytes());
        bytes[partition_offset + 12..partition_offset + 16].copy_from_slice(&3_u32.to_le_bytes());
        bytes[1024..1024 + 1536].fill(0x5a);
        bytes
    }

    #[test]
    fn observes_completed_prepared_state_after_pending_clears() {
        let temp = tempdir().expect("tempdir");
        let state_dir = temp.path().join("state");
        let executor = UpdateExecutor {
            state_dir: state_dir.clone(),
            boot_mount_dir: temp.path().join("boot"),
            boot_ota_dir: temp.path().join("boot-ota"),
            local_ota_dir: temp.path().join("local-ota"),
            system_layout_manifest_path: temp.path().join("layout.toml"),
            reboot_program: temp.path().join("fake-systemctl"),
            release_manager: ServiceReleaseManager::new(temp.path().join("releases"), temp.path().join("bin")),
            default_hook_timeout_secs: crate::config::DEFAULT_HOOK_TIMEOUT_SECS,
        };
        let slots = SlotLayout {
            scheme: "squashfs_ab".into(),
            active: UpdateSlot::B,
            active_name: "ROOT_B".into(),
            active_label: None,
            active_device: Some(temp.path().join("root-b.img")),
            inactive: Some(UpdateSlot::A),
            inactive_name: Some("ROOT_A".into()),
            inactive_label: None,
            inactive_device: Some(temp.path().join("root-a.img")),
            inactive_exists: true,
            inactive_size_bytes: Some(0),
            required_size_bytes: Some(0),
        };
        let prepared = PreparedState {
            workload_id: "update.node-local.1".into(),
            message: "prepared".into(),
            prepared_boot_id: "old-boot-id".into(),
            update_id: "update.node-local.1".into(),
            expected_selector: "ROOT_B".into(),
            expected_active_device: slots.active_device.clone(),
            previous_selector: "ROOT_A".into(),
            previous_active_device: slots.inactive_device.clone(),
            rollback_requested: false,
            outcome: None,
        };
        let path = state_dir.join(PREPARED_DIR).join("update.node-local.1.json");
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(&path, serde_json::to_vec_pretty(&prepared).expect("json")).expect("write");
        fs::create_dir_all(temp.path().join("local-ota")).expect("mkdir local ota");
        fs::write(temp.path().join("local-ota/pending"), "ROOT_B\n").expect("write pending");

        let observed = executor.observe_prepared("update.node-local.1", &slots).expect("observe").expect("prepared");

        assert!(matches!(observed, PreparedObservation::Completed { .. }));
        assert!(!temp.path().join("local-ota/pending").exists());
        assert!(fs::read_to_string(temp.path().join("local-ota/confirm-result.env")).expect("confirm").contains("HELIOS_UPDATE_CONFIRM_STATUS=confirmed"));
    }

    #[test]
    fn finalize_completed_clears_repartition_state() {
        let temp = tempdir().expect("tempdir");
        let state_dir = temp.path().join("state");
        let local_ota_dir = temp.path().join("local-ota");
        let boot_ota_dir = temp.path().join("boot-ota");
        let executor = UpdateExecutor {
            state_dir: state_dir.clone(),
            boot_mount_dir: temp.path().join("boot"),
            boot_ota_dir: boot_ota_dir.clone(),
            local_ota_dir: local_ota_dir.clone(),
            system_layout_manifest_path: temp.path().join("layout.toml"),
            reboot_program: temp.path().join("fake-systemctl"),
            release_manager: ServiceReleaseManager::new(temp.path().join("releases"), temp.path().join("bin")),
            default_hook_timeout_secs: crate::config::DEFAULT_HOOK_TIMEOUT_SECS,
        };

        let prepared = PreparedState {
            workload_id: "update.node-local.1".into(),
            message: "prepared".into(),
            prepared_boot_id: "boot-id".into(),
            update_id: "update.node-local.1".into(),
            expected_selector: "ROOT_B".into(),
            expected_active_device: Some(temp.path().join("root-b.img")),
            previous_selector: "ROOT_A".into(),
            previous_active_device: Some(temp.path().join("root-a.img")),
            rollback_requested: false,
            outcome: Some(PreparedOutcome::Completed),
        };
        let prepared_path = state_dir.join(PREPARED_DIR).join("update.node-local.1.json");
        fs::create_dir_all(prepared_path.parent().expect("parent")).expect("mkdir");
        fs::write(&prepared_path, serde_json::to_vec_pretty(&prepared).expect("json")).expect("write prepared");

        let repartition = RepartitionState {
            workload_id: "update.node-local.1".into(),
            requested_boot_id: "old-boot-id".into(),
            action: "resize_inactive".into(),
            update_id: "update.node-local.1".into(),
            outcome: None,
        };
        let repartition_path = state_dir.join(REPARTITION_DIR).join("update.node-local.1.json");
        fs::create_dir_all(repartition_path.parent().expect("parent")).expect("mkdir");
        fs::write(&repartition_path, serde_json::to_vec_pretty(&repartition).expect("json")).expect("write repartition");
        fs::create_dir_all(&local_ota_dir).expect("mkdir local ota");
        fs::create_dir_all(&boot_ota_dir).expect("mkdir boot ota");
        for path in [
            local_ota_dir.join("repartition-request.env"),
            local_ota_dir.join("repartition-result.env"),
            local_ota_dir.join("repartition-layout.toml"),
            boot_ota_dir.join("repartition-request.env"),
            boot_ota_dir.join("repartition-result.env"),
            boot_ota_dir.join("repartition-layout.toml"),
        ] {
            fs::write(&path, "placeholder").expect("write repartition metadata");
        }

        let manifest = UpdateManifest {
            artifact_id: "artifact.os".into(),
            manifest_version: "v1".into(),
            version: "2026.2.0".into(),
            artifact_class: crate::model::UpdateArtifactClass::OsImage,
            compatibility: crate::manifest::CompatibilityRule { requires_ab_rootfs: true, ..CompatibilityRule::default() },
            hooks: Vec::new(),
            payload: UpdatePayload::OsImage(crate::manifest::OsImagePayload {
                image_url: "file:///tmp/rootfs.squashfs".into(),
                size_bytes: None,
                sha256: None,
                inactive_slot_min_bytes: None,
                boot_assets: Vec::new(),
            }),
        };

        executor.finalize_completed("update.node-local.1", &manifest).expect("finalize completed");

        assert!(!repartition_path.exists());
        for path in [
            local_ota_dir.join("repartition-request.env"),
            local_ota_dir.join("repartition-result.env"),
            local_ota_dir.join("repartition-layout.toml"),
            boot_ota_dir.join("repartition-request.env"),
            boot_ota_dir.join("repartition-result.env"),
            boot_ota_dir.join("repartition-layout.toml"),
        ] {
            assert!(!path.exists(), "{} should be removed", path.display());
        }
    }

    #[test]
    fn overlay_upperdir_is_parsed_from_root_overlay_mount() {
        let mounts = "\
overlay / overlay rw,relatime,lowerdir=/mnt/lower,upperdir=/var/lib/helios/root-overlay/root-b/upper,workdir=/var/lib/helios/root-overlay/root-b/work 0 0\n\
proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0\n";
        let upperdir = overlay_upperdir_from_mounts(mounts).expect("upperdir");
        assert_eq!(upperdir, PathBuf::from("/var/lib/helios/root-overlay/root-b/upper"));
    }

    #[test]
    fn scrub_overlay_image_overrides_removes_upper_file_and_empty_parents() {
        let temp = tempdir().expect("tempdir");
        let upper = temp.path().join("upper");
        let updater = upper.join("usr/bin/helios-updater");
        let paths_file = temp.path().join("image-owned-overlay-paths");
        fs::create_dir_all(updater.parent().expect("parent")).expect("mkdir");
        fs::write(&updater, b"stale").expect("write");
        fs::write(&paths_file, "/usr/bin/helios-updater\n").expect("paths");

        scrub_overlay_image_overrides(&upper, &paths_file).expect("scrub");

        assert!(!updater.exists());
        assert!(!upper.join("usr/bin").exists());
        assert!(!upper.join("usr").exists());
    }

    #[test]
    fn payload_updates_complete_without_boot_prepare() {
        let _guard = env_lock().lock().expect("env lock");
        let temp = tempdir().expect("tempdir");
        let releases_dir = temp.path().join("releases");
        let bin_dir = temp.path().join("bin");
        let fake_bin_dir = temp.path().join("fake-bin");
        fs::create_dir_all(&fake_bin_dir).expect("fake bin dir");
        let fake_systemctl = fake_bin_dir.join("systemctl");
        fs::write(&fake_systemctl, "#!/bin/sh\nexit 0\n").expect("fake systemctl");
        let mut permissions = fs::metadata(&fake_systemctl).expect("fake systemctl metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_systemctl, permissions).expect("chmod");
        let executor = UpdateExecutor {
            state_dir: temp.path().join("state"),
            boot_mount_dir: temp.path().join("boot"),
            boot_ota_dir: temp.path().join("boot-ota"),
            local_ota_dir: temp.path().join("local-ota"),
            system_layout_manifest_path: temp.path().join("layout.toml"),
            reboot_program: fake_systemctl.clone(),
            release_manager: ServiceReleaseManager::with_systemctl_program(releases_dir.clone(), bin_dir.clone(), fake_systemctl.clone()),
            default_hook_timeout_secs: crate::config::DEFAULT_HOOK_TIMEOUT_SECS,
        };
        fs::write(&executor.system_layout_manifest_path, b"schema_version = 1\n").expect("layout");
        let source_a = temp.path().join("helios-api-a");
        let source_b = temp.path().join("helios-api-b");
        fs::write(&source_a, b"a").expect("source a");
        fs::write(&source_b, b"b").expect("source b");
        executor.release_manager.stage("helios-api", "rev-a", &source_a).expect("stage a");
        executor.release_manager.activate("helios-api", "rev-a").expect("activate a");
        executor.release_manager.stage("helios-api", "rev-b", &source_b).expect("stage b");

        let manifest = UpdateManifest {
            manifest_version: "v1".into(),
            artifact_id: "artifact.payload".into(),
            version: "rev-b".into(),
            artifact_class: UpdateArtifactClass::PayloadUpdate,
            compatibility: CompatibilityRule { requires_ab_rootfs: false, ..CompatibilityRule::default() },
            payload: UpdatePayload::PayloadUpdate(PayloadUpdatePayload { targets: vec![] }),
            hooks: vec![],
        };
        let staged = StagedManifest {
            artifact_id: manifest.artifact_id.clone(),
            artifact: StagedArtifact::PayloadUpdate {
                staged_targets: vec![StagedPayloadTarget { name: "helios-api".into(), revision: "rev-b".into(), staged_path: releases_dir.join("rev-b").join("helios-api") }],
            },
        };
        let slots = SlotLayout {
            scheme: "squashfs_ab".into(),
            active: UpdateSlot::A,
            active_name: "ROOT_A".into(),
            active_label: None,
            active_device: Some(temp.path().join("active.img")),
            inactive: Some(UpdateSlot::B),
            inactive_name: Some("ROOT_B".into()),
            inactive_label: None,
            inactive_device: Some(temp.path().join("inactive.img")),
            inactive_exists: true,
            inactive_size_bytes: Some(0),
            required_size_bytes: Some(0),
        };

        let outcome = executor.apply_staged("update.node-local.payload", &manifest, &slots, &staged).expect("payload apply");

        assert_eq!(outcome, ApplyOutcome::Completed { message: "activated and restarted 1 payload targets".into() });
        assert!(!executor.is_prepared("update.node-local.payload"));
        let status = executor.release_manager.status("helios-api").expect("status");
        assert_eq!(status.active_revision.as_deref(), Some("rev-b"));
    }

    #[test]
    fn updater_payload_update_defers_self_restart_until_post_publish() {
        let _guard = env_lock().lock().expect("env lock");
        let temp = tempdir().expect("tempdir");
        let releases_dir = temp.path().join("releases");
        let bin_dir = temp.path().join("bin");
        let fake_bin_dir = temp.path().join("fake-bin");
        let restart_log = temp.path().join("restart.log");
        fs::create_dir_all(&fake_bin_dir).expect("fake bin dir");
        let fake_systemctl = fake_bin_dir.join("systemctl");
        fs::write(&fake_systemctl, format!("#!/bin/sh\nprintf '%s %s\\n' \"$1\" \"$2\" >> '{}'\nexit 0\n", restart_log.display())).expect("fake systemctl");
        let mut permissions = fs::metadata(&fake_systemctl).expect("fake systemctl metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_systemctl, permissions).expect("chmod");
        let executor = UpdateExecutor {
            state_dir: temp.path().join("state"),
            boot_mount_dir: temp.path().join("boot"),
            boot_ota_dir: temp.path().join("boot-ota"),
            local_ota_dir: temp.path().join("local-ota"),
            system_layout_manifest_path: temp.path().join("layout.toml"),
            reboot_program: fake_systemctl.clone(),
            release_manager: ServiceReleaseManager::with_systemctl_program(releases_dir.clone(), bin_dir.clone(), fake_systemctl.clone()),
            default_hook_timeout_secs: crate::config::DEFAULT_HOOK_TIMEOUT_SECS,
        };
        fs::write(&executor.system_layout_manifest_path, b"schema_version = 1\n").expect("layout");
        let source_a = temp.path().join("helios-updater-a");
        let source_b = temp.path().join("helios-updater-b");
        fs::write(&source_a, b"a").expect("source a");
        fs::write(&source_b, b"b").expect("source b");
        executor.release_manager.stage("helios-updater", "rev-a", &source_a).expect("stage a");
        executor.release_manager.activate("helios-updater", "rev-a").expect("activate a");
        executor.release_manager.stage("helios-updater", "rev-b", &source_b).expect("stage b");

        let manifest = UpdateManifest {
            manifest_version: "v1".into(),
            artifact_id: "artifact.updater".into(),
            version: "rev-b".into(),
            artifact_class: UpdateArtifactClass::PayloadUpdate,
            compatibility: CompatibilityRule { requires_ab_rootfs: false, ..CompatibilityRule::default() },
            payload: UpdatePayload::PayloadUpdate(PayloadUpdatePayload { targets: vec![] }),
            hooks: vec![],
        };
        let staged = StagedManifest {
            artifact_id: manifest.artifact_id.clone(),
            artifact: StagedArtifact::PayloadUpdate {
                staged_targets: vec![StagedPayloadTarget { name: "helios-updater".into(), revision: "rev-b".into(), staged_path: releases_dir.join("rev-b").join("helios-updater") }],
            },
        };
        let slots = SlotLayout {
            scheme: "squashfs_ab".into(),
            active: UpdateSlot::A,
            active_name: "ROOT_A".into(),
            active_label: None,
            active_device: Some(temp.path().join("active.img")),
            inactive: Some(UpdateSlot::B),
            inactive_name: Some("ROOT_B".into()),
            inactive_label: None,
            inactive_device: Some(temp.path().join("inactive.img")),
            inactive_exists: true,
            inactive_size_bytes: Some(0),
            required_size_bytes: Some(0),
        };

        let outcome = executor.apply_staged("update.node-local.updater", &manifest, &slots, &staged).expect("payload apply");

        assert_eq!(outcome, ApplyOutcome::Completed { message: "activated 1 payload targets; deferred 1 self-restart until after publish".into() });
        assert!(!restart_log.exists(), "self restart should not happen inline");

        executor.run_pending_actions().expect("run pending actions");
        let log = fs::read_to_string(&restart_log).expect("restart log");
        assert_eq!(log, "reset-failed helios-updater.service\nrestart helios-updater.service\n");
    }

    #[test]
    fn updater_payload_update_does_not_restart_when_revision_is_already_active() {
        let _guard = env_lock().lock().expect("env lock");
        let temp = tempdir().expect("tempdir");
        let releases_dir = temp.path().join("releases");
        let bin_dir = temp.path().join("bin");
        let fake_bin_dir = temp.path().join("fake-bin");
        let restart_log = temp.path().join("restart.log");
        fs::create_dir_all(&fake_bin_dir).expect("fake bin dir");
        let fake_systemctl = fake_bin_dir.join("systemctl");
        fs::write(&fake_systemctl, format!("#!/bin/sh\nprintf '%s %s\\n' \"$1\" \"$2\" >> '{}'\nexit 0\n", restart_log.display())).expect("fake systemctl");
        let mut permissions = fs::metadata(&fake_systemctl).expect("fake systemctl metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_systemctl, permissions).expect("chmod");
        let executor = UpdateExecutor {
            state_dir: temp.path().join("state"),
            boot_mount_dir: temp.path().join("boot"),
            boot_ota_dir: temp.path().join("boot-ota"),
            local_ota_dir: temp.path().join("local-ota"),
            system_layout_manifest_path: temp.path().join("layout.toml"),
            reboot_program: fake_systemctl.clone(),
            release_manager: ServiceReleaseManager::with_systemctl_program(releases_dir.clone(), bin_dir.clone(), fake_systemctl.clone()),
            default_hook_timeout_secs: crate::config::DEFAULT_HOOK_TIMEOUT_SECS,
        };
        fs::write(&executor.system_layout_manifest_path, b"schema_version = 1\n").expect("layout");
        let source = temp.path().join("helios-updater");
        fs::write(&source, b"a").expect("source");
        executor.release_manager.stage("helios-updater", "rev-a", &source).expect("stage a");
        executor.release_manager.activate("helios-updater", "rev-a").expect("activate a");

        let manifest = UpdateManifest {
            manifest_version: "v1".into(),
            artifact_id: "artifact.updater".into(),
            version: "rev-a".into(),
            artifact_class: UpdateArtifactClass::PayloadUpdate,
            compatibility: CompatibilityRule { requires_ab_rootfs: false, ..CompatibilityRule::default() },
            payload: UpdatePayload::PayloadUpdate(PayloadUpdatePayload { targets: vec![] }),
            hooks: vec![],
        };
        let staged = StagedManifest {
            artifact_id: manifest.artifact_id.clone(),
            artifact: StagedArtifact::PayloadUpdate {
                staged_targets: vec![StagedPayloadTarget { name: "helios-updater".into(), revision: "rev-a".into(), staged_path: releases_dir.join("rev-a").join("helios-updater") }],
            },
        };
        let slots = SlotLayout {
            scheme: "squashfs_ab".into(),
            active: UpdateSlot::A,
            active_name: "ROOT_A".into(),
            active_label: None,
            active_device: Some(temp.path().join("active.img")),
            inactive: Some(UpdateSlot::B),
            inactive_name: Some("ROOT_B".into()),
            inactive_label: None,
            inactive_device: Some(temp.path().join("inactive.img")),
            inactive_exists: true,
            inactive_size_bytes: Some(0),
            required_size_bytes: Some(0),
        };

        let outcome = executor.apply_staged("update.node-local.updater", &manifest, &slots, &staged).expect("payload apply");

        assert_eq!(outcome, ApplyOutcome::Completed { message: "activated 0 payload targets; 1 already active".into() });
        assert!(!restart_log.exists(), "already active updater should not restart");
        assert!(!executor.state_dir.join(ACTIONS_DIR).exists(), "already active updater should not enqueue actions");
    }

    #[test]
    fn payload_hook_receives_context_and_writes_logs() {
        let _guard = env_lock().lock().expect("env lock");
        let temp = tempdir().expect("tempdir");
        let releases_dir = temp.path().join("releases");
        let bin_dir = temp.path().join("bin");
        let fake_bin_dir = temp.path().join("fake-bin");
        fs::create_dir_all(&fake_bin_dir).expect("fake bin dir");
        let fake_systemctl = fake_bin_dir.join("systemctl");
        fs::write(&fake_systemctl, "#!/bin/sh\nexit 0\n").expect("fake systemctl");
        let mut permissions = fs::metadata(&fake_systemctl).expect("fake systemctl metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_systemctl, permissions).expect("chmod");

        let hook = temp.path().join("emit-hook-env.sh");
        fs::write(&hook, "#!/bin/sh\nprintf '%s\\n' \"$HELIOS_UPDATE_WORKLOAD_ID\" \"$HELIOS_UPDATE_PHASE\" \"$HELIOS_UPDATE_CLASS\"\n").expect("hook");
        let mut hook_permissions = fs::metadata(&hook).expect("hook meta").permissions();
        hook_permissions.set_mode(0o755);
        fs::set_permissions(&hook, hook_permissions).expect("hook chmod");

        let executor = UpdateExecutor {
            state_dir: temp.path().join("state"),
            boot_mount_dir: temp.path().join("boot"),
            boot_ota_dir: temp.path().join("boot-ota"),
            local_ota_dir: temp.path().join("local-ota"),
            system_layout_manifest_path: temp.path().join("layout.toml"),
            reboot_program: fake_systemctl.clone(),
            release_manager: ServiceReleaseManager::with_systemctl_program(releases_dir.clone(), bin_dir.clone(), fake_systemctl.clone()),
            default_hook_timeout_secs: crate::config::DEFAULT_HOOK_TIMEOUT_SECS,
        };
        fs::write(&executor.system_layout_manifest_path, b"schema_version = 1\n").expect("layout");
        let source_a = temp.path().join("helios-api-a");
        let source_b = temp.path().join("helios-api-b");
        fs::write(&source_a, b"a").expect("source a");
        fs::write(&source_b, b"b").expect("source b");
        executor.release_manager.stage("helios-api", "rev-a", &source_a).expect("stage a");
        executor.release_manager.activate("helios-api", "rev-a").expect("activate a");
        executor.release_manager.stage("helios-api", "rev-b", &source_b).expect("stage b");

        let manifest = UpdateManifest {
            manifest_version: "v1".into(),
            artifact_id: "artifact.payload".into(),
            version: "rev-b".into(),
            artifact_class: UpdateArtifactClass::PayloadUpdate,
            compatibility: CompatibilityRule { requires_ab_rootfs: false, ..CompatibilityRule::default() },
            payload: UpdatePayload::PayloadUpdate(PayloadUpdatePayload { targets: vec![] }),
            hooks: vec![crate::manifest::ManifestHook { phase: HookPhase::Postboot, command: vec![hook.display().to_string()], timeout_secs: Some(5) }],
        };
        let staged = StagedManifest {
            artifact_id: manifest.artifact_id.clone(),
            artifact: StagedArtifact::PayloadUpdate {
                staged_targets: vec![StagedPayloadTarget { name: "helios-api".into(), revision: "rev-b".into(), staged_path: releases_dir.join("rev-b").join("helios-api") }],
            },
        };
        let slots = SlotLayout {
            scheme: "squashfs_ab".into(),
            active: UpdateSlot::A,
            active_name: "ROOT_A".into(),
            active_label: None,
            active_device: Some(temp.path().join("active.img")),
            inactive: Some(UpdateSlot::B),
            inactive_name: Some("ROOT_B".into()),
            inactive_label: None,
            inactive_device: Some(temp.path().join("inactive.img")),
            inactive_exists: true,
            inactive_size_bytes: Some(0),
            required_size_bytes: Some(0),
        };

        executor.apply_staged("update.node-local.payload", &manifest, &slots, &staged).expect("payload apply");

        let stdout = fs::read_to_string(temp.path().join("state/hooks/update.node-local.payload/postboot-0.stdout")).expect("stdout log");
        assert_eq!(stdout, "update.node-local.payload\npostboot\npayload-update\n");
    }

    #[test]
    fn payload_hook_timeout_rolls_back_activation() {
        let _guard = env_lock().lock().expect("env lock");
        let temp = tempdir().expect("tempdir");
        let releases_dir = temp.path().join("releases");
        let bin_dir = temp.path().join("bin");
        let fake_bin_dir = temp.path().join("fake-bin");
        fs::create_dir_all(&fake_bin_dir).expect("fake bin dir");
        let fake_systemctl = fake_bin_dir.join("systemctl");
        fs::write(&fake_systemctl, "#!/bin/sh\nexit 0\n").expect("fake systemctl");
        let mut permissions = fs::metadata(&fake_systemctl).expect("fake systemctl metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_systemctl, permissions).expect("chmod");
        let executor = UpdateExecutor {
            state_dir: temp.path().join("state"),
            boot_mount_dir: temp.path().join("boot"),
            boot_ota_dir: temp.path().join("boot-ota"),
            local_ota_dir: temp.path().join("local-ota"),
            system_layout_manifest_path: temp.path().join("layout.toml"),
            reboot_program: fake_systemctl.clone(),
            release_manager: ServiceReleaseManager::with_systemctl_program(releases_dir.clone(), bin_dir.clone(), fake_systemctl.clone()),
            default_hook_timeout_secs: 1,
        };
        fs::write(&executor.system_layout_manifest_path, b"schema_version = 1\n").expect("layout");
        let source_a = temp.path().join("helios-api-a");
        let source_b = temp.path().join("helios-api-b");
        fs::write(&source_a, b"a").expect("source a");
        fs::write(&source_b, b"b").expect("source b");
        executor.release_manager.stage("helios-api", "rev-a", &source_a).expect("stage a");
        executor.release_manager.activate("helios-api", "rev-a").expect("activate a");
        executor.release_manager.stage("helios-api", "rev-b", &source_b).expect("stage b");

        let manifest = UpdateManifest {
            manifest_version: "v1".into(),
            artifact_id: "artifact.payload".into(),
            version: "rev-b".into(),
            artifact_class: UpdateArtifactClass::PayloadUpdate,
            compatibility: CompatibilityRule { requires_ab_rootfs: false, ..CompatibilityRule::default() },
            payload: UpdatePayload::PayloadUpdate(PayloadUpdatePayload { targets: vec![] }),
            hooks: vec![crate::manifest::ManifestHook { phase: HookPhase::Postboot, command: vec!["/bin/sh".into(), "-c".into(), "sleep 2".into()], timeout_secs: Some(1) }],
        };
        let staged = StagedManifest {
            artifact_id: manifest.artifact_id.clone(),
            artifact: StagedArtifact::PayloadUpdate {
                staged_targets: vec![StagedPayloadTarget { name: "helios-api".into(), revision: "rev-b".into(), staged_path: releases_dir.join("rev-b").join("helios-api") }],
            },
        };
        let slots = SlotLayout {
            scheme: "squashfs_ab".into(),
            active: UpdateSlot::A,
            active_name: "ROOT_A".into(),
            active_label: None,
            active_device: Some(temp.path().join("active.img")),
            inactive: Some(UpdateSlot::B),
            inactive_name: Some("ROOT_B".into()),
            inactive_label: None,
            inactive_device: Some(temp.path().join("inactive.img")),
            inactive_exists: true,
            inactive_size_bytes: Some(0),
            required_size_bytes: Some(0),
        };

        let error = executor.apply_staged("update.node-local.payload", &manifest, &slots, &staged).expect_err("payload apply should fail");
        assert!(error.to_string().contains("timed out after 1s"));

        let status = executor.release_manager.status("helios-api").expect("status");
        assert_eq!(status.active_revision.as_deref(), Some("rev-a"));
    }

    #[test]
    fn repartition_request_writes_layout_and_request_files() {
        let temp = tempdir().expect("tempdir");
        let fake_systemctl = temp.path().join("fake-systemctl");
        fs::write(&fake_systemctl, "#!/bin/sh\nexit 0\n").expect("fake reboot");
        let mut perms = fs::metadata(&fake_systemctl).expect("meta").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&fake_systemctl, perms).expect("chmod");
        let layout = temp.path().join("storage-layout.toml");
        fs::write(
            &layout,
            "[live_repartition]\nallow_destructive_data_borrow = true\n[[partitions]]\nrole = \"slot_b\"\nsize_source = \"mirror_existing\"\n[[partitions]]\nrole = \"data\"\nfill_to_end = true\n",
        )
        .expect("layout");
        let executor = UpdateExecutor {
            state_dir: temp.path().join("state"),
            boot_mount_dir: temp.path().join("boot"),
            boot_ota_dir: temp.path().join("boot-ota"),
            local_ota_dir: temp.path().join("local-ota"),
            system_layout_manifest_path: layout.clone(),
            reboot_program: fake_systemctl,
            release_manager: ServiceReleaseManager::new(temp.path().join("releases"), temp.path().join("bin")),
            default_hook_timeout_secs: crate::config::DEFAULT_HOOK_TIMEOUT_SECS,
        };

        let message = executor
            .request_repartition(
                "update.node-local.1",
                &SlotAction::ResizeInactive { required_size_bytes: 1024 },
                &SlotLayout {
                    scheme: "squashfs_ab".into(),
                    active: UpdateSlot::A,
                    active_name: "ROOT_A".into(),
                    active_label: None,
                    active_device: Some(temp.path().join("root-a.img")),
                    inactive: Some(UpdateSlot::B),
                    inactive_name: Some("ROOT_B".into()),
                    inactive_label: None,
                    inactive_device: Some(temp.path().join("root-b.img")),
                    inactive_exists: true,
                    inactive_size_bytes: Some(512),
                    required_size_bytes: Some(2048),
                },
            )
            .expect("repartition");

        assert!(message.contains("resize_inactive"));
        assert_eq!(
            fs::read_to_string(temp.path().join("local-ota/repartition-request.env")).expect("request"),
            "HELIOS_REPARTITION_UPDATE_ID=update.node-local.1\nHELIOS_REPARTITION_ACTION=resize_inactive\n"
        );
        let rendered: toml::Value = toml::from_str(&fs::read_to_string(temp.path().join("local-ota/repartition-layout.toml")).expect("layout")).expect("rendered toml");
        let source: toml::Value = toml::from_str(&fs::read_to_string(layout).expect("source layout")).expect("source toml");
        assert_eq!(rendered, source);
    }

    #[test]
    fn repartition_request_swaps_live_layout_when_root_a_is_inactive() {
        let temp = tempdir().expect("tempdir");
        let fake_systemctl = temp.path().join("fake-systemctl");
        fs::write(&fake_systemctl, "#!/bin/sh\nexit 0\n").expect("fake reboot");
        let mut perms = fs::metadata(&fake_systemctl).expect("meta").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&fake_systemctl, perms).expect("chmod");
        let layout = temp.path().join("storage-layout.toml");
        fs::write(
            &layout,
            "schema_version = 1\nlayout_id = \"squashfs_ab\"\nslot_scheme = \"squashfs_ab\"\n[live_repartition]\nallow_destructive_data_borrow = true\n[[partitions]]\nrole = \"slot_a\"\nname = \"ROOT_A\"\nnumber = 2\nmode = \"noop\"\nfs_type = \"none\"\nsize_source = \"mirror_existing\"\nmkfs = false\nwipe_signatures = false\nrun_fsck = false\nrun_resizefs = false\n[[partitions]]\nrole = \"slot_b\"\nname = \"ROOT_B\"\nnumber = 3\nmode = \"mkpart\"\nfs_type = \"ext4\"\nsize_source = \"mirror_existing\"\nmkfs = false\nwipe_signatures = true\nrun_fsck = false\nrun_resizefs = false\ngap_after_mib = 256\nmarker = \"/var/lib/helios/.provision.reserve_v1\"\n[[partitions]]\nrole = \"data\"\nname = \"DATA\"\nnumber = 4\nmode = \"mkpart\"\nfill_to_end = true\n",
        )
        .expect("layout");
        let executor = UpdateExecutor {
            state_dir: temp.path().join("state"),
            boot_mount_dir: temp.path().join("boot"),
            boot_ota_dir: temp.path().join("boot-ota"),
            local_ota_dir: temp.path().join("local-ota"),
            system_layout_manifest_path: layout.clone(),
            reboot_program: fake_systemctl,
            release_manager: ServiceReleaseManager::new(temp.path().join("releases"), temp.path().join("bin")),
            default_hook_timeout_secs: crate::config::DEFAULT_HOOK_TIMEOUT_SECS,
        };

        executor
            .request_repartition(
                "update.node-local.root-a",
                &SlotAction::ResizeInactive { required_size_bytes: 1024 },
                &SlotLayout {
                    scheme: "squashfs_ab".into(),
                    active: UpdateSlot::B,
                    active_name: "ROOT_B".into(),
                    active_label: None,
                    active_device: Some(temp.path().join("root-b.img")),
                    inactive: Some(UpdateSlot::A),
                    inactive_name: Some("ROOT_A".into()),
                    inactive_label: None,
                    inactive_device: Some(temp.path().join("root-a.img")),
                    inactive_exists: true,
                    inactive_size_bytes: Some(512),
                    required_size_bytes: Some(2048),
                },
            )
            .expect("repartition");

        let rendered: toml::Value = toml::from_str(&fs::read_to_string(temp.path().join("local-ota/repartition-layout.toml")).expect("rendered layout")).expect("rendered toml");
        let partitions = rendered.get("partitions").and_then(toml::Value::as_array).expect("partitions");
        let slot_a = partitions.iter().find(|partition| partition.get("role").and_then(toml::Value::as_str) == Some("slot_a")).expect("slot_a");
        let slot_b = partitions.iter().find(|partition| partition.get("role").and_then(toml::Value::as_str) == Some("slot_b")).expect("slot_b");
        assert_eq!(slot_a.get("name").and_then(toml::Value::as_str), Some("ROOT_A"));
        assert_eq!(slot_a.get("number").and_then(toml::Value::as_integer), Some(2));
        assert_eq!(slot_a.get("mode").and_then(toml::Value::as_str), Some("mkpart"));
        assert_eq!(slot_a.get("fs_type").and_then(toml::Value::as_str), Some("ext4"));
        assert_eq!(slot_b.get("name").and_then(toml::Value::as_str), Some("ROOT_B"));
        assert_eq!(slot_b.get("number").and_then(toml::Value::as_integer), Some(3));
        assert_eq!(slot_b.get("mode").and_then(toml::Value::as_str), Some("noop"));
    }

    #[test]
    fn observes_completed_repartition_result() {
        let temp = tempdir().expect("tempdir");
        let fake_systemctl = temp.path().join("fake-systemctl");
        fs::write(&fake_systemctl, "#!/bin/sh\nexit 0\n").expect("fake reboot");
        let mut perms = fs::metadata(&fake_systemctl).expect("meta").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&fake_systemctl, perms).expect("chmod");
        let layout = temp.path().join("storage-layout.toml");
        fs::write(&layout, "[live_repartition]\nallow_destructive_data_borrow = true\n[[partitions]]\nrole = \"slot_b\"\n[[partitions]]\nrole = \"data\"\n").expect("layout");
        let executor = UpdateExecutor {
            state_dir: temp.path().join("state"),
            boot_mount_dir: temp.path().join("boot"),
            boot_ota_dir: temp.path().join("boot-ota"),
            local_ota_dir: temp.path().join("local-ota"),
            system_layout_manifest_path: layout,
            reboot_program: fake_systemctl,
            release_manager: ServiceReleaseManager::new(temp.path().join("releases"), temp.path().join("bin")),
            default_hook_timeout_secs: crate::config::DEFAULT_HOOK_TIMEOUT_SECS,
        };
        executor.record_repartition("update.node-local.1", &SlotAction::ResizeInactive { required_size_bytes: 1024 }, "update.node-local.1").expect("record");
        fs::create_dir_all(temp.path().join("local-ota")).expect("mkdir");
        fs::write(temp.path().join("local-ota/repartition-result.env"), "HELIOS_REPARTITION_UPDATE_ID=update.node-local.1\nHELIOS_REPARTITION_STATUS=applied\n").expect("result");

        let observed = executor
            .observe_repartition(
                "update.node-local.1",
                &SlotLayout {
                    scheme: "squashfs_ab".into(),
                    active: UpdateSlot::A,
                    active_name: "ROOT_A".into(),
                    active_label: None,
                    active_device: Some(temp.path().join("root-a.img")),
                    inactive: Some(UpdateSlot::B),
                    inactive_name: Some("ROOT_B".into()),
                    inactive_label: None,
                    inactive_device: Some(temp.path().join("root-b.img")),
                    inactive_exists: true,
                    inactive_size_bytes: Some(1024),
                    required_size_bytes: Some(1024),
                },
            )
            .expect("observe")
            .expect("state");

        assert_eq!(observed, RepartitionObservation::Completed { message: "repartition request resize_inactive completed with status applied".into() });
    }

    #[test]
    fn repartition_request_fails_when_layout_cannot_satisfy_required_slot_size() {
        let temp = tempdir().expect("tempdir");
        let fake_systemctl = temp.path().join("fake-systemctl");
        fs::write(&fake_systemctl, "#!/bin/sh\nexit 0\n").expect("fake reboot");
        let mut perms = fs::metadata(&fake_systemctl).expect("meta").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&fake_systemctl, perms).expect("chmod");
        let layout = temp.path().join("storage-layout.toml");
        fs::write(
            &layout,
            "[live_repartition]\nallow_destructive_data_borrow = true\n[[partitions]]\nrole = \"slot_b\"\nsize_source = \"mirror_existing\"\n[[partitions]]\nrole = \"data\"\nfill_to_end = true\n",
        )
        .expect("layout");
        let executor = UpdateExecutor {
            state_dir: temp.path().join("state"),
            boot_mount_dir: temp.path().join("boot"),
            boot_ota_dir: temp.path().join("boot-ota"),
            local_ota_dir: temp.path().join("local-ota"),
            system_layout_manifest_path: layout,
            reboot_program: fake_systemctl,
            release_manager: ServiceReleaseManager::new(temp.path().join("releases"), temp.path().join("bin")),
            default_hook_timeout_secs: crate::config::DEFAULT_HOOK_TIMEOUT_SECS,
        };

        let error = executor
            .request_repartition(
                "update.node-local.1",
                &SlotAction::ResizeInactive { required_size_bytes: 4096 },
                &SlotLayout {
                    scheme: "squashfs_ab".into(),
                    active: UpdateSlot::A,
                    active_name: "ROOT_A".into(),
                    active_label: None,
                    active_device: Some(temp.path().join("root-a.img")),
                    inactive: Some(UpdateSlot::B),
                    inactive_name: Some("ROOT_B".into()),
                    inactive_label: None,
                    inactive_device: Some(temp.path().join("root-b.img")),
                    inactive_exists: true,
                    inactive_size_bytes: Some(1024),
                    required_size_bytes: Some(2048),
                },
            )
            .expect_err("request should fail");

        assert!(error.to_string().contains("storage layout can only produce inactive slot size 2048 bytes but update requires 4096 bytes"));
    }
}
