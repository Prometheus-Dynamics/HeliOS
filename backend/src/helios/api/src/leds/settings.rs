//! The LED director's settings (`GET`/`PUT /v1/device/leds`), kept in the API's state directory
//! (`leds.json`, see `store.rs`).

use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};

/// The Raze's status ring: logical LEDs, LED 0 at the top (lemnosd applies the board's offset
/// and direction).
pub const RING_LEDS: usize = 16;

/// Which way the logical indices run round the ring, as seen from the front.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Cw,
    Ccw,
}

impl Direction {
    /// +1 for clockwise, -1 for counter-clockwise: the sign of a tag's bearing on the ring.
    pub const fn sign(self) -> f64 {
        match self {
            Self::Cw => 1.0,
            Self::Ccw => -1.0,
        }
    }
}

/// Ambient brightness: the ring follows the camera's lux between `min` and `max` (both 0 to 1 of
/// the ring brightness).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ambient {
    pub enabled: bool,
    pub min: f64,
    pub max: f64,
}

impl Default for Ambient {
    fn default() -> Self {
        Self { enabled: false, min: 0.3, max: 1.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LedSettings {
    /// HeliOS drives the ring. Off: the director releases it and the ring shows nothing from HeliOS.
    pub enabled: bool,
    /// Ring brightness, 0 to 1 (lemnosd's ring-wide look scale; 0.5 is the board's default).
    pub brightness: f64,
    pub ambient: Ambient,
    /// The logical LED that faces the camera's centre (0 to 15; 0 is the ring's top).
    pub forward_led: u8,
    pub direction: Direction,
    /// The camera's horizontal field of view in degrees, used for tag bearings until the camera
    /// is calibrated.
    pub fov_deg: f64,
    /// A soft red tally on LED 0 while a recording runs.
    pub tally: bool,
    /// Ring brightness +30% while the robot is enabled.
    pub boost: bool,
}

impl Default for LedSettings {
    fn default() -> Self {
        Self { enabled: true, brightness: 0.5, ambient: Ambient::default(), forward_led: 0, direction: Direction::Cw, fov_deg: 90.0, tally: true, boost: true }
    }
}

impl LedSettings {
    pub fn validate(&self) -> ApiResult<()> {
        let unit = |value: f64| value.is_finite() && (0.0..=1.0).contains(&value);
        if !unit(self.brightness) {
            return Err(ApiError::unprocessable("brightness is 0 to 1"));
        }
        if !unit(self.ambient.min) || !unit(self.ambient.max) || self.ambient.min > self.ambient.max {
            return Err(ApiError::unprocessable("ambient min and max are 0 to 1, with min at most max"));
        }
        if usize::from(self.forward_led) >= RING_LEDS {
            return Err(ApiError::unprocessable("forward_led is an LED index, 0 to 15"));
        }
        if !self.fov_deg.is_finite() || !(20.0..=180.0).contains(&self.fov_deg) {
            return Err(ApiError::unprocessable("fov_deg is the camera's horizontal field of view in degrees, 20 to 180"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_parse_partially() {
        assert!(LedSettings::default().validate().is_ok());
        let partial: LedSettings = serde_json::from_str(r#"{"brightness": 0.8, "direction": "ccw"}"#).expect("partial settings");
        assert_eq!(partial.brightness, 0.8);
        assert_eq!(partial.direction, Direction::Ccw);
        assert!(partial.enabled && partial.tally, "missing fields keep their defaults");
    }

    #[test]
    fn unknown_keys_and_bad_ranges_are_refused() {
        assert!(serde_json::from_str::<LedSettings>(r#"{"colour": "red"}"#).is_err());
        let bad = |edit: fn(&mut LedSettings)| {
            let mut settings = LedSettings::default();
            edit(&mut settings);
            settings.validate().is_err()
        };
        assert!(bad(|s| s.brightness = 1.5));
        assert!(bad(|s| s.brightness = f64::NAN));
        assert!(bad(|s| s.ambient = Ambient { enabled: true, min: 0.8, max: 0.2 }));
        assert!(bad(|s| s.forward_led = 16));
        assert!(bad(|s| s.fov_deg = 10.0));
        assert!(LedSettings { forward_led: 15, ..LedSettings::default() }.validate().is_ok());
    }

    #[test]
    fn direction_is_the_sign_of_a_clockwise_step() {
        assert_eq!(Direction::Cw.sign(), 1.0);
        assert_eq!(Direction::Ccw.sign(), -1.0);
    }
}
