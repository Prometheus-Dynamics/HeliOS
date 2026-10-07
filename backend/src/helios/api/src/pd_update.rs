//! The Raze device package's A/B updater (`/usr/lib/pd-device/update`, Atlas `docs/ota.md`).
//!
//! HeliOS has no updater of its own: the device package's writer copies boot slot A and root
//! slot A of the same `.img.xz` that is flashed into the board's inactive slot, trial-boots it
//! with the Pi bootloader's tryboot and keeps it once `/etc/pd-device/update-health` passes.
//! This module reads the state it publishes in `/run/pd-device/update.json`; helios-api drives
//! the CLI itself (`/v1/update/*`, `/v1/ota/*`).

use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::host::read_trimmed;

/// The device package's update CLI.
pub const PD_UPDATE_TOOL: &str = "/usr/lib/pd-device/update";
/// `{"state", "slot_active", "slot_staged", "version_active", "version_staged", "progress", "error"}`,
/// rewritten by every update command (and at boot by `pd-device-update-confirm.service`).
pub const PD_UPDATE_STATUS: &str = "/run/pd-device/update.json";
/// While an image is copied into the inactive slot: `pd-image-slots`' own progress, 0..1000.
pub const PD_UPDATE_COPY_PROGRESS: &str = "/run/pd-device/update/progress";

/// The updater's state, as `update status` prints it. `progress` is 0..1000.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdUpdateStatus {
    /// idle, staging, staged, trying, confirmed, rolled-back or error.
    #[serde(default)]
    pub state: String,
    /// A or B (the root partition the system booted from), `unknown` off the A/B layout.
    #[serde(default, deserialize_with = "empty_as_none")]
    pub slot_active: Option<String>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub slot_staged: Option<String>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub version_active: Option<String>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub version_staged: Option<String>,
    #[serde(default)]
    pub progress: u32,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub error: Option<String>,
}

fn empty_as_none<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.filter(|v| !v.trim().is_empty()))
}

impl PdUpdateStatus {
    pub fn parse(json: &str) -> Option<Self> {
        serde_json::from_str(json.lines().next()?).ok()
    }

    /// Whether the board runs from an A/B layout the writer can update.
    pub fn on_ab_layout(&self) -> bool {
        matches!(self.slot_active.as_deref(), Some("A" | "B"))
    }

    /// `trying` before the trial reboot: the staged slot is not the one running yet.
    pub fn rebooting(&self) -> bool {
        self.state == "trying" && self.slot_active != self.slot_staged
    }

    /// Progress in percent (the writer reports 0..1000).
    pub fn percent(&self) -> Option<u8> {
        match self.state.as_str() {
            "staging" | "staged" | "trying" | "confirmed" => Some((self.progress.min(1000) / 10) as u8),
            _ => None,
        }
    }

    /// The same state in Atlas's update-stage vocabulary (`/v1/ota/state`).
    pub fn atlas_stage(&self) -> &'static str {
        match self.state.as_str() {
            "idle" => "idle",
            // 0..100 is the image's SHA-256 check, 100..950 the slot copy.
            "staging" if self.progress < 100 => "verifying",
            "staging" => "installing",
            "staged" => "committing",
            "trying" if self.rebooting() => "rebooting",
            "trying" => "finalizing",
            "confirmed" => "complete",
            "rolled-back" => "rolled_back",
            "error" => "failed",
            _ => "unknown",
        }
    }
}

/// Reads the updater's status file; while it copies an image, the copy's own progress (0..1000)
/// maps to 100..950 as `update status` reports it.
pub fn read_status_at(status_path: &Path, copy_progress_path: &Path) -> Option<PdUpdateStatus> {
    let mut status = PdUpdateStatus::parse(&fs::read_to_string(status_path).ok()?)?;
    if status.state == "staging"
        && status.progress >= 100
        && let Some(copied) = read_trimmed(copy_progress_path).and_then(|p| p.parse::<u32>().ok())
    {
        status.progress = status.progress.max(100 + copied.min(1000) * 85 / 100);
    }
    Some(status)
}

/// The `v…` component of an image file name (`helios-full-raze-v2026.4.0.img.xz` -> `v2026.4.0`).
pub fn infer_version_from_path(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let mut stem = name;
    for suffix in [".xz", ".zst", ".gz"] {
        stem = stem.strip_suffix(suffix).unwrap_or(stem);
    }
    let stem = stem.strip_suffix(".img").unwrap_or(stem);
    stem.rsplit('-').find(|part| part.starts_with('v') && part.len() > 1 && part[1..].starts_with(|c: char| c.is_ascii_digit())).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_writer_status() {
        let status = PdUpdateStatus::parse(r#"{"state":"staged","slot_active":"A","slot_staged":"B","version_active":"v1","version_staged":"v2","progress":1000,"error":""}"#).expect("status");
        assert_eq!(status.slot_staged.as_deref(), Some("B"));
        assert_eq!(status.error, None);
        assert_eq!(status.percent(), Some(100));
        assert_eq!(status.atlas_stage(), "committing");
        assert!(status.on_ab_layout());
    }

    #[test]
    fn trial_boot_stages() {
        let mut status = PdUpdateStatus { state: "trying".into(), slot_active: Some("A".into()), slot_staged: Some("B".into()), ..PdUpdateStatus::default() };
        assert_eq!(status.atlas_stage(), "rebooting");
        status.slot_active = Some("B".into());
        assert_eq!(status.atlas_stage(), "finalizing");
        status.state = "rolled-back".into();
        assert_eq!(status.atlas_stage(), "rolled_back");
        let off_layout = PdUpdateStatus { state: "idle".into(), slot_active: Some("unknown".into()), ..PdUpdateStatus::default() };
        assert!(!off_layout.on_ab_layout());
    }

    #[test]
    fn live_copy_progress_while_staging() {
        let dir = tempfile::tempdir().expect("tempdir");
        let status = dir.path().join("update.json");
        let progress = dir.path().join("progress");
        fs::write(&status, r#"{"state":"staging","slot_active":"A","slot_staged":"B","version_active":"v1","version_staged":"","progress":100,"error":""}"#).expect("write");
        fs::write(&progress, "500\n").expect("write");
        let read = read_status_at(&status, &progress).expect("status");
        assert_eq!(read.progress, 525);
        assert_eq!(read.atlas_stage(), "installing");
        assert!(read_status_at(&dir.path().join("missing.json"), &progress).is_none());
    }

    #[test]
    fn versions_from_image_names() {
        assert_eq!(infer_version_from_path(Path::new("/data/helios-full-raze-v2026.4.0.img.xz")).as_deref(), Some("v2026.4.0"));
        assert_eq!(infer_version_from_path(Path::new("helios-raze-v2026.4.0.img")).as_deref(), Some("v2026.4.0"));
        assert_eq!(infer_version_from_path(Path::new("image.img.zst")), None);
    }
}
