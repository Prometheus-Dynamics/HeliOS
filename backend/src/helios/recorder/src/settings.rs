//! Recorder settings: where recordings go, how much space to keep free, file caps, the frame rate
//! limit, compression and the host channels' sample periods. helios-api keeps them in its state
//! directory and passes the effective values with each start.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const GIB: u64 = 1 << 30;
pub const MIB: u64 = 1 << 20;

/// What a recording stores of each frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordFrames {
    /// The sensor's 16-bit Bayer frames as they are, before the ISP (`Frames::sensor_raw`).
    #[default]
    Raw,
    /// The ISP's 8-bit luma at the camera's mode size.
    Luma,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RecorderSettings {
    /// Where recordings are written. `None`: a mounted USB drive under `usb_roots` that is
    /// writable, else `fallback_dir`.
    pub storage_dir: Option<PathBuf>,
    /// Directories whose mounted children are searched for a USB drive (depth 2).
    pub usb_roots: Vec<PathBuf>,
    /// Used when no USB drive is writable.
    pub fallback_dir: PathBuf,
    /// Free space kept on the volume: a recording is refused, or stopped, below it.
    pub reserve_bytes: u64,
    /// A part is closed and the next one started above this size.
    pub max_file_bytes: u64,
    /// ... or after this long.
    pub max_file_seconds: u64,
    /// Frames per second at most; `None` records the camera's rate, every frame. The camera's
    /// 120 fps is subsampled to this (30 by default: the rate the CM5 eMMC sustains with raw + lz4).
    pub max_fps: Option<f64>,
    /// What each frame is: the raw sensor frames (default) or the ISP's luma.
    pub frames: RecordFrames,
    /// lz4 chunk compression (the default), else uncompressed chunks.
    pub compression: bool,
    /// Encoded bytes waiting for the disk before frames and host messages are dropped and counted.
    pub queue_bytes: usize,
    /// Sample period of `/helios/imu`, `/helios/mag` and `/helios/power` (lemnosd subscriptions).
    pub imu_period_ms: u32,
    pub mag_period_ms: u32,
    pub power_period_ms: u32,
}

impl Default for RecorderSettings {
    fn default() -> Self {
        Self {
            storage_dir: None,
            usb_roots: vec!["/media".into(), "/run/media".into()],
            fallback_dir: "/data/helios/recordings".into(),
            reserve_bytes: 2 * GIB,
            max_file_bytes: 2 * GIB,
            max_file_seconds: 600,
            max_fps: Some(30.0),
            frames: RecordFrames::Raw,
            compression: true,
            queue_bytes: 64 * (MIB as usize),
            imu_period_ms: 20,
            mag_period_ms: 100,
            power_period_ms: 100,
        }
    }
}

impl RecorderSettings {
    /// Checks the values a recording can run with (ranges, absolute paths).
    pub fn validate(&self) -> Result<(), String> {
        if let Some(dir) = &self.storage_dir
            && !dir.is_absolute()
        {
            return Err("storage_dir must be an absolute path".into());
        }
        if self.usb_roots.iter().any(|root| !root.is_absolute()) || !self.fallback_dir.is_absolute() {
            return Err("usb_roots and fallback_dir must be absolute paths".into());
        }
        if self.max_file_bytes < MIB {
            return Err("max_file_bytes must be at least 1 MiB".into());
        }
        if !(1..=86_400).contains(&self.max_file_seconds) {
            return Err("max_file_seconds must be 1 to 86400".into());
        }
        if self.max_fps.is_some_and(|fps| !(fps.is_finite() && fps > 0.0 && fps <= 1000.0)) {
            return Err("max_fps must be above 0 and at most 1000 (or null for the camera's rate)".into());
        }
        if self.queue_bytes < MIB as usize {
            return Err("queue_bytes must be at least 1 MiB".into());
        }
        for (name, period) in [("imu_period_ms", self.imu_period_ms), ("mag_period_ms", self.mag_period_ms), ("power_period_ms", self.power_period_ms)] {
            if !(1..=60_000).contains(&period) {
                return Err(format!("{name} must be 1 to 60000"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_raw_at_30_fps_compressed() {
        let settings = RecorderSettings::default();
        assert!(settings.validate().is_ok());
        assert_eq!(settings.max_fps, Some(30.0));
        assert_eq!(settings.frames, RecordFrames::Raw);
        assert!(settings.compression);
        assert!(settings.queue_bytes >= 64 * MIB as usize);
        assert_eq!(settings.reserve_bytes, 2 * GIB);
    }

    #[test]
    fn out_of_range_values_are_refused() {
        assert!(RecorderSettings { max_fps: Some(0.0), ..RecorderSettings::default() }.validate().is_err());
        assert!(RecorderSettings { max_file_bytes: 1024, ..RecorderSettings::default() }.validate().is_err());
        assert!(RecorderSettings { storage_dir: Some("relative".into()), ..RecorderSettings::default() }.validate().is_err());
        assert!(RecorderSettings { imu_period_ms: 0, ..RecorderSettings::default() }.validate().is_err());
    }

    #[test]
    fn unknown_fields_are_refused() {
        assert!(serde_json::from_str::<RecorderSettings>(r#"{"bogus": 1}"#).is_err());
        let partial: RecorderSettings = serde_json::from_str(r#"{"compression": false}"#).expect("partial");
        assert!(!partial.compression);
        assert_eq!(partial.reserve_bytes, 2 * GIB);
    }
}
