//! HeliOS's fan rule, over lemnosd: **read-only by default**.
//!
//! The kernel's thermal governor drives the fan, and lemnosd hands it back to the governor
//! whenever it stops (Lemnos `docs/system-service.md`, "Fan hand-back"). HeliOS only publishes
//! the fan's readings. Its one write is an explicit, time-limited override:
//!
//! - `fan.override` `{pwm: 0-255 | duty: 0.0-1.0, duration_ms: 1000-600000}` (default 60 s):
//!   `DeviceClient::set(fan, "duty", ..)`.
//! - It ends with `DeviceClient::release(fan)` (back to the governor) on `fan.release`, when
//!   `duration_ms` runs out and when helios-peripherals stops.
//! - lemnosd ties the write to HeliOS's connection: when the connection closes (a crash, a
//!   `kill -9`, a lemnosd restart), lemnosd undoes it and the fan goes back to the governor. So a
//!   lost connection simply ends the override, with nothing left to release and nothing kept on
//!   disk; a reconnect never resumes it.
//! - Another client writing the fan (a self-test, `lemnos-ctl`) ends the override without a
//!   release: the fan is theirs now.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub const FAN_OVERRIDE_ACTION: &str = "fan.override";
pub const FAN_RELEASE_ACTION: &str = "fan.release";
/// The `hwmon-fan` driver's control: a duty ratio, 0.0-1.0.
pub const FAN_DUTY_CONTROL: &str = "duty";
pub const DEFAULT_OVERRIDE_MS: u64 = 60_000;
pub const MIN_OVERRIDE_MS: u64 = 1_000;
pub const MAX_OVERRIDE_MS: u64 = 600_000;

/// A validated `fan.override`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FanOverrideRequest {
    /// 0.0-1.0.
    pub duty: f64,
    pub duration: Duration,
}

impl FanOverrideRequest {
    /// `pwm` (0-255) or `duty` (0.0-1.0), exactly one, and `duration_ms` (default 60 s, at most
    /// 10 min).
    pub fn from_args(pwm: Option<u64>, duty: Option<f64>, duration_ms: Option<u64>) -> Result<Self, String> {
        let duty = match (pwm, duty) {
            (Some(pwm), None) if pwm <= 255 => pwm as f64 / 255.0,
            (Some(_), None) => return Err("arg.pwm must be between 0 and 255".into()),
            (None, Some(duty)) if (0.0..=1.0).contains(&duty) => duty,
            (None, Some(_)) => return Err("arg.duty must be between 0.0 and 1.0".into()),
            (Some(_), Some(_)) => return Err("give arg.pwm (0-255) or arg.duty (0.0-1.0), not both".into()),
            (None, None) => return Err("arg.pwm (0-255) or arg.duty (0.0-1.0) is required".into()),
        };
        let duration_ms = duration_ms.unwrap_or(DEFAULT_OVERRIDE_MS);
        if !(MIN_OVERRIDE_MS..=MAX_OVERRIDE_MS).contains(&duration_ms) {
            return Err(format!("arg.duration_ms must be between {MIN_OVERRIDE_MS} and {MAX_OVERRIDE_MS}"));
        }
        Ok(Self { duty, duration: Duration::from_millis(duration_ms) })
    }
}

/// One running override.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveOverride {
    /// The duty lemnosd applied.
    pub duty: f64,
    pub until: Instant,
    /// `until` on the wall clock, for the published state.
    pub until_ms: u64,
}

/// The running overrides per fan device.
#[derive(Debug, Default)]
pub struct FanOverrides {
    active: BTreeMap<String, ActiveOverride>,
}

impl FanOverrides {
    /// An override was applied: it runs for `duration` from `now`.
    pub fn start(&mut self, device: &str, duty: f64, now: Instant, now_ms: u64, duration: Duration) -> ActiveOverride {
        let active = ActiveOverride { duty, until: now + duration, until_ms: now_ms.saturating_add(duration.as_millis() as u64) };
        self.active.insert(device.to_string(), active);
        active
    }

    /// The override is over (released, refused, or the fan is with the governor again).
    pub fn end(&mut self, device: &str) {
        self.active.remove(device);
    }

    /// Another client wrote the fan: the override is over, and the fan is theirs (no release).
    pub fn taken_over(&mut self, device: &str) -> bool {
        self.active.remove(device).is_some()
    }

    /// The connection to lemnosd is gone: lemnosd undid HeliOS's writes, so every override is
    /// over (a reconnect does not resume it).
    pub fn disconnected(&mut self) {
        self.active.clear();
    }

    /// The overrides whose time ran out at `now`.
    pub fn expired(&self, now: Instant) -> Vec<String> {
        self.active.iter().filter(|(_, active)| active.until <= now).map(|(device, _)| device.clone()).collect()
    }

    pub fn is_active(&self, device: &str) -> bool {
        self.active.contains_key(device)
    }

    pub fn active(&self) -> impl Iterator<Item = (&str, &ActiveOverride)> {
        self.active.iter().map(|(device, active)| (device.as_str(), active))
    }

    /// The fans to release when helios-peripherals stops.
    pub fn devices(&self) -> Vec<String> {
        self.active.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_requests_are_bounded() {
        assert_eq!(FanOverrideRequest::from_args(Some(255), None, None), Ok(FanOverrideRequest { duty: 1.0, duration: Duration::from_millis(DEFAULT_OVERRIDE_MS) }));
        assert_eq!(FanOverrideRequest::from_args(None, Some(0.5), Some(5_000)), Ok(FanOverrideRequest { duty: 0.5, duration: Duration::from_secs(5) }));
        assert_eq!(FanOverrideRequest::from_args(Some(0), None, Some(MAX_OVERRIDE_MS)).map(|r| r.duty), Ok(0.0));
        assert!(FanOverrideRequest::from_args(Some(256), None, None).is_err());
        assert!(FanOverrideRequest::from_args(None, Some(1.5), None).is_err());
        assert!(FanOverrideRequest::from_args(None, Some(-0.1), None).is_err());
        assert!(FanOverrideRequest::from_args(Some(10), Some(0.1), None).is_err());
        assert!(FanOverrideRequest::from_args(None, None, None).is_err());
        assert!(FanOverrideRequest::from_args(Some(10), None, Some(MAX_OVERRIDE_MS + 1)).is_err(), "at most 10 minutes");
        assert!(FanOverrideRequest::from_args(Some(10), None, Some(10)).is_err());
    }

    #[test]
    fn overrides_expire_after_their_duration() {
        let mut fans = FanOverrides::default();
        let now = Instant::now();
        let active = fans.start("fan", 0.8, now, 1_000, Duration::from_secs(5));
        assert_eq!(active.until_ms, 6_000);
        assert!(fans.expired(now + Duration::from_millis(4_999)).is_empty());
        assert_eq!(fans.expired(now + Duration::from_secs(5)), vec!["fan".to_string()]);
        fans.end("fan");
        assert!(!fans.is_active("fan"));
        assert!(fans.devices().is_empty());
    }

    #[test]
    fn a_disconnect_ends_overrides_without_a_release() {
        let mut fans = FanOverrides::default();
        let now = Instant::now();
        fans.start("fan", 0.8, now, 0, Duration::from_secs(60));
        fans.disconnected();
        assert!(!fans.is_active("fan"), "a reconnect must not resume the override");
        assert!(fans.devices().is_empty(), "lemnosd undid the write with the connection");
    }

    #[test]
    fn another_writer_takes_over_without_a_release() {
        let mut fans = FanOverrides::default();
        fans.start("fan", 0.5, Instant::now(), 0, Duration::from_secs(10));
        assert!(fans.taken_over("fan"));
        assert!(fans.devices().is_empty(), "the other writer owns the fan now");
        assert!(!fans.taken_over("fan"));
    }
}
