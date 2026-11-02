//! The Raze device package's slots writer (`/usr/lib/board/slots`, Atlas `docs/slots.md`).
//!
//! HeliOS has no updater of its own: the writer installs a slot bundle (a tar of `slot.json`, the
//! slot's root image and its boot files) into an inactive slot, trial-boots it once with the
//! firmware's tryboot, and keeps it when `/etc/board/update-health` passes. This module reads the
//! state it publishes in `/run/board/update.json`; helios-api drives the CLI itself
//! (`/v1/update/*`, `/v1/ota/*`).

use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

/// The writer's state, as `slots status` prints it: `{"state", "layout", "slot_active",
/// "slot_staged", "slot_default", "slot_previous", "slot_trial", "version_active", "version_staged",
/// "progress", "error", "free_bytes", "reserve_bytes", "slots"}`, rewritten by every slots command.
/// `progress` is 0..1000 (the install's own progress while it is staging).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardUpdateStatus {
    /// idle, staging, staged, rebooting (a trial or rollback reboot is under way), trying (the
    /// trial boot is being judged), confirmed, rolled-back or error.
    #[serde(default)]
    pub state: String,
    /// `slots` on the dynamic-slots layout (the only Raze layout); absent otherwise.
    #[serde(default, deserialize_with = "empty_as_none")]
    pub layout: Option<String>,
    /// The slot this system booted from.
    #[serde(default, deserialize_with = "empty_as_none")]
    pub slot_active: Option<String>,
    /// The slot holding a staged update (until it is confirmed, or staged but not yet applied).
    #[serde(default, deserialize_with = "empty_as_none")]
    pub slot_staged: Option<String>,
    /// The slot a plain boot starts.
    #[serde(default, deserialize_with = "empty_as_none")]
    pub slot_default: Option<String>,
    /// The slot before the default, the one `rollback` returns to.
    #[serde(default, deserialize_with = "empty_as_none")]
    pub slot_previous: Option<String>,
    /// The slot on trial (booted once, not yet confirmed).
    #[serde(default, deserialize_with = "empty_as_none")]
    pub slot_trial: Option<String>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub version_active: Option<String>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub version_staged: Option<String>,
    #[serde(default)]
    pub progress: u32,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub free_bytes: Option<u64>,
    #[serde(default)]
    pub reserve_bytes: Option<u64>,
    /// Every installed slot as the writer lists it (id, family, version, variant, size,
    /// verity, flags, pinned, good, ...).
    #[serde(default)]
    pub slots: Vec<serde_json::Value>,
}

fn empty_as_none<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.filter(|v| !v.trim().is_empty()))
}

impl BoardUpdateStatus {
    pub fn parse(json: &str) -> Option<Self> {
        serde_json::from_str(json.lines().next()?).ok()
    }

    /// Whether the board runs the dynamic-slots layout the writer can update.
    pub fn on_slots_layout(&self) -> bool {
        self.layout.as_deref() == Some("slots")
    }

    /// Whether a trial boot is pending or under way (its verdict is not in yet).
    pub fn on_trial(&self) -> bool {
        self.slot_trial.is_some()
    }

    /// HeliOS's phase: the writer's state, with a reboot into a trial shown as `trying` (the trial
    /// stays open until its boot is judged).
    pub fn phase(&self) -> String {
        if self.state == "rebooting" && self.on_trial() { "trying".into() } else { self.state.clone() }
    }

    /// Progress in percent (the writer reports 0..1000).
    pub fn percent(&self) -> Option<u8> {
        match self.state.as_str() {
            "staging" | "staged" | "rebooting" | "trying" | "confirmed" => Some((self.progress.min(1000) / 10) as u8),
            _ => None,
        }
    }

    /// The same state in Atlas's update-stage vocabulary (`/v1/ota/state`).
    pub fn atlas_stage(&self) -> &'static str {
        match self.state.as_str() {
            "idle" => "idle",
            "staging" => "installing",
            "staged" => "committing",
            "rebooting" => "rebooting",
            "trying" => "finalizing",
            "confirmed" => "complete",
            "rolled-back" => "rolled_back",
            "error" => "failed",
            _ => "unknown",
        }
    }
}

/// Reads the writer's status file (its first line).
pub fn read_status(status_path: &Path) -> Option<BoardUpdateStatus> {
    BoardUpdateStatus::parse(&fs::read_to_string(status_path).ok()?)
}

/// The version from an image or bundle name (`helios-full-raze-v2026.4.0.slot.tar` ->
/// `v2026.4.0`, `helios-full-raze-v2026.4.0.img.xz` likewise).
pub fn infer_version_from_path(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let mut stem = name;
    for suffix in [".xz", ".zst", ".gz", ".tar"] {
        stem = stem.strip_suffix(suffix).unwrap_or(stem);
    }
    for suffix in [".img", ".slot"] {
        stem = stem.strip_suffix(suffix).unwrap_or(stem);
    }
    stem.rsplit('-').find(|part| part.starts_with('v') && part.len() > 1 && part[1..].starts_with(|c: char| c.is_ascii_digit())).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_writer_status() {
        let status = BoardUpdateStatus::parse(
            r#"{"state":"staged","layout":"slots","slot_active":"helios-a","slot_staged":"helios-b","slot_default":"helios-a","slot_previous":"","slot_trial":"","version_active":"v1","version_staged":"v2","progress":1000,"error":"","free_bytes":2147483648,"reserve_bytes":2147483648,"slots":[{"id":"helios-a"}]}"#,
        )
        .expect("status");
        assert_eq!(status.slot_staged.as_deref(), Some("helios-b"));
        assert_eq!(status.slot_previous, None, "an empty id is no slot");
        assert_eq!(status.error, None);
        assert_eq!(status.percent(), Some(100));
        assert_eq!(status.atlas_stage(), "committing");
        assert!(status.on_slots_layout());
        assert_eq!(status.slots.len(), 1);
    }

    #[test]
    fn a_trial_reboot_is_the_trying_phase() {
        let mut status = BoardUpdateStatus {
            state: "rebooting".into(),
            slot_active: Some("helios-a".into()),
            slot_staged: Some("helios-b".into()),
            slot_trial: Some("helios-b".into()),
            ..BoardUpdateStatus::default()
        };
        assert_eq!(status.phase(), "trying");
        assert_eq!(status.atlas_stage(), "rebooting");
        status.state = "trying".into();
        assert_eq!(status.phase(), "trying");
        assert_eq!(status.atlas_stage(), "finalizing");
        status.state = "rolled-back".into();
        status.slot_trial = None;
        assert_eq!(status.atlas_stage(), "rolled_back");
        // A rollback reboot has no trial: it is not shown as a trial.
        let rollback = BoardUpdateStatus { state: "rebooting".into(), ..BoardUpdateStatus::default() };
        assert_eq!(rollback.phase(), "rebooting");
        let off_layout = BoardUpdateStatus { state: "idle".into(), ..BoardUpdateStatus::default() };
        assert!(!off_layout.on_slots_layout());
    }

    #[test]
    fn reads_the_status_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("update.json");
        fs::write(&path, r#"{"state":"staging","layout":"slots","progress":600,"error":""}"#).expect("write");
        let status = read_status(&path).expect("status");
        assert_eq!(status.percent(), Some(60));
        assert_eq!(status.atlas_stage(), "installing");
        assert!(read_status(&dir.path().join("missing.json")).is_none());
    }

    #[test]
    fn versions_from_image_and_bundle_names() {
        assert_eq!(infer_version_from_path(Path::new("/data/helios-full-raze-v2026.4.0.img.xz")).as_deref(), Some("v2026.4.0"));
        assert_eq!(infer_version_from_path(Path::new("helios-raze-v2026.4.0.img")).as_deref(), Some("v2026.4.0"));
        assert_eq!(infer_version_from_path(Path::new("helios-raze-v2026.4.0.slot.tar")).as_deref(), Some("v2026.4.0"));
        assert_eq!(infer_version_from_path(Path::new("image.img.zst")), None);
    }
}
