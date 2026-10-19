//! Tag spots on the ring. A tag's bearing maps to a fractional logical LED (camera forward is
//! `forward_led`, and positive bearings run the way `direction` says). Spots glide to their new
//! positions along the shorter arc, and fade in and out, so a tag that moves never jumps.

use std::collections::BTreeMap;

use super::settings::{Direction, RING_LEDS};
use super::states::TagObs;

/// The ring's length in LEDs, as a float for position maths.
pub const RING: f64 = RING_LEDS as f64;

/// Time constants of the easing, in seconds (about 1 to 2 time constants to settle).
const POSITION_TAU: f64 = 0.25;
const LEVEL_TAU: f64 = 0.35;
/// A tag's spot is dropped once it has faded out this far.
const GONE_BELOW: f64 = 0.01;
/// The sheen orbits the ring once per this many milliseconds (a slow white sheen).
pub const SHEEN_PERIOD_MS: f64 = 6000.0;

/// Wraps a ring position into `[0, 16)`.
pub fn wrap(position: f64) -> f64 {
    position.rem_euclid(RING)
}

/// The signed step from `from` to `to` along the shorter arc of the ring, in LEDs.
pub fn shortest_step(from: f64, to: f64) -> f64 {
    (to - from + RING / 2.0).rem_euclid(RING) - RING / 2.0
}

/// The fractional logical LED a tag at `bearing_deg` lands on.
pub fn ring_position(bearing_deg: f64, forward_led: u8, direction: Direction) -> f64 {
    wrap(f64::from(forward_led) + direction.sign() * bearing_deg / 360.0 * RING)
}

/// A tag's spot level from its apparent size: a tag filling a tenth of the frame width is full
/// brightness, a small distant one is dim but still visible.
pub fn size_level(size: f64) -> f64 {
    ((size - 0.02) / 0.10).clamp(0.3, 1.0)
}

/// The sheen's position at `t_ms`.
pub fn sheen_position(t_ms: u64) -> f64 {
    wrap(t_ms as f64 / SHEEN_PERIOD_MS * RING)
}

/// One spot: where it is, and where it is going.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Spot {
    position: f64,
    target: f64,
    level: f64,
    level_target: f64,
}

/// A spot as drawn: a fractional LED position and a level (0 to 1).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct SpotView {
    #[serde(rename = "at")]
    pub position: f64,
    pub level: f64,
}

/// The spots, keyed by tag id (ordered, so the frames are deterministic).
#[derive(Debug, Default)]
pub struct SpotBank {
    spots: BTreeMap<u32, Spot>,
}

/// The exponential approach factor for `dt` seconds at time constant `tau`.
fn approach(dt: f64, tau: f64) -> f64 {
    1.0 - (-dt / tau).exp()
}

impl SpotBank {
    /// Moves the spots toward `tags` (id, ring position, level) over `dt` seconds. A spot whose tag
    /// is not in `tags` fades out and is dropped.
    pub fn update(&mut self, tags: &[(u32, f64, f64)], dt: f64) {
        for spot in self.spots.values_mut() {
            spot.level_target = 0.0;
        }
        for &(id, position, level) in tags {
            let spot = self.spots.entry(id).or_insert(Spot { position, target: position, level: 0.0, level_target: 0.0 });
            spot.target = position;
            spot.level_target = level;
        }
        for spot in self.spots.values_mut() {
            spot.position = wrap(spot.position + shortest_step(spot.position, spot.target) * approach(dt, POSITION_TAU));
            spot.level += (spot.level_target - spot.level) * approach(dt, LEVEL_TAU);
        }
        self.spots.retain(|_, spot| spot.level_target > 0.0 || spot.level >= GONE_BELOW);
    }

    /// The spots as drawn now.
    pub fn views(&self) -> Vec<SpotView> {
        self.spots.values().map(|spot| SpotView { position: spot.position, level: spot.level.clamp(0.0, 1.0) }).collect()
    }
}

/// The tag spots' targets for `tags`, with the tags' ring positions and levels.
pub fn targets(tags: &[TagObs], forward_led: u8, direction: Direction) -> Vec<(u32, f64, f64)> {
    tags.iter().map(|tag| (tag.id, ring_position(tag.bearing_deg, forward_led, direction), size_level(tag.size))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bearing_lands_on_the_forward_led_and_runs_with_the_direction() {
        assert_eq!(ring_position(0.0, 0, Direction::Cw), 0.0);
        assert_eq!(ring_position(0.0, 5, Direction::Cw), 5.0, "camera forward is the configured LED");
        assert!((ring_position(90.0, 0, Direction::Cw) - 4.0).abs() < 1e-9, "a quarter turn is four LEDs");
        assert!((ring_position(90.0, 0, Direction::Ccw) - 12.0).abs() < 1e-9, "counter-clockwise runs the other way");
        assert!((ring_position(-22.5, 0, Direction::Cw) - 15.0).abs() < 1e-9, "left of forward wraps round the top");
        assert!((ring_position(360.0, 3, Direction::Cw) - 3.0).abs() < 1e-9, "a full turn is the same LED");
    }

    #[test]
    fn the_shortest_step_crosses_the_seam() {
        assert!((shortest_step(15.0, 1.0) - 2.0).abs() < 1e-9, "from 15 to 1 goes forward through 0");
        assert!((shortest_step(1.0, 15.0) + 2.0).abs() < 1e-9, "and back the other way");
        assert!((shortest_step(4.0, 8.0) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn a_spot_glides_across_the_seam_and_settles() {
        let mut bank = SpotBank::default();
        bank.update(&[(7, 15.5, 1.0)], 1.0);
        let mut previous = bank.views()[0].position;
        for _ in 0..60 {
            bank.update(&[(7, 0.5, 1.0)], 0.05);
            let now = bank.views()[0].position;
            // The spot moves forward through the seam (15.5 to 0.5 is one LED), never the long way.
            assert!(shortest_step(previous, now).abs() < 1.0, "a jump from {previous} to {now}");
            previous = now;
        }
        assert!(shortest_step(previous, 0.5).abs() < 0.01, "settles on the target: {previous}");
    }

    #[test]
    fn a_spot_fades_in_and_out_and_is_dropped() {
        let mut bank = SpotBank::default();
        bank.update(&[(1, 3.0, 1.0)], 0.1);
        assert!(bank.views()[0].level < 1.0, "a new spot fades in");
        for _ in 0..40 {
            bank.update(&[(1, 3.0, 1.0)], 0.1);
        }
        assert!(bank.views()[0].level > 0.98);
        for _ in 0..80 {
            bank.update(&[], 0.1);
        }
        assert!(bank.views().is_empty(), "a tag that is gone fades out and is dropped");
    }

    #[test]
    fn a_closer_tag_is_brighter() {
        assert!(size_level(0.02) <= size_level(0.06));
        assert!((size_level(0.12) - 1.0).abs() < 1e-9);
        assert!((size_level(0.0) - 0.3).abs() < 1e-9, "a tiny tag is still shown");
    }

    #[test]
    fn the_sheen_orbits_once_per_period() {
        assert!(sheen_position(0).abs() < 1e-9);
        assert!((sheen_position(3000) - 8.0).abs() < 1e-9);
        assert!((sheen_position(6000)).abs() < 1e-9, "back to the start");
    }
}
