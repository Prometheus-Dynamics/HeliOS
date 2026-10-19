//! Ambient brightness: the ring's scale follows the camera's lux on a log scale, from `min` at
//! darkness to `max` at 1000 lux and above, smoothed over a few seconds so the ring does not
//! flicker with the lighting.

use super::settings::Ambient;

/// The lux at which the scale reaches `max` (the log mapping's top).
const FULL_LUX: f64 = 1000.0;
/// The smoothing time constant, in seconds.
const TAU_S: f64 = 3.0;

#[derive(Debug, Default)]
pub struct AmbientFilter {
    value: f64,
    primed: bool,
}

/// The lux-driven target scale, from `min` to `max`.
pub fn target(lux: Option<f64>, settings: &Ambient) -> f64 {
    match lux {
        // Without a reading, the ring keeps its brightest scale rather than dimming on a guess.
        None => settings.max,
        Some(lux) => {
            let fraction = ((1.0 + lux.max(0.0)).log10() / (1.0 + FULL_LUX).log10()).clamp(0.0, 1.0);
            settings.min + (settings.max - settings.min) * fraction
        }
    }
}

impl AmbientFilter {
    /// The scale for this tick. Off: 1 (the settings' brightness applies as given).
    pub fn step(&mut self, lux: Option<f64>, settings: &Ambient, dt: f64) -> f64 {
        if !settings.enabled {
            self.primed = false;
            self.value = 1.0;
            return 1.0;
        }
        let target = target(lux, settings);
        if self.primed {
            self.value += (target - self.value) * (1.0 - (-dt / TAU_S).exp());
        } else {
            self.value = target;
            self.primed = true;
        }
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> Ambient {
        Ambient { enabled: true, min: 0.2, max: 1.0 }
    }

    #[test]
    fn dark_is_min_and_bright_is_max() {
        assert!((target(Some(0.0), &on()) - 0.2).abs() < 1e-9);
        assert!((target(Some(1000.0), &on()) - 1.0).abs() < 1e-9);
        assert!((target(Some(1_000_000.0), &on()) - 1.0).abs() < 1e-9, "above the top is max");
        let mid = target(Some(30.0), &on());
        assert!(mid > 0.2 && mid < 1.0, "a dim room sits between: {mid}");
    }

    #[test]
    fn without_a_reading_the_ring_keeps_its_full_scale() {
        assert_eq!(target(None, &on()), 1.0);
    }

    #[test]
    fn the_scale_eases_toward_the_target_over_seconds() {
        let mut filter = AmbientFilter::default();
        let first = filter.step(Some(0.0), &on(), 0.1);
        assert!((first - 0.2).abs() < 1e-9, "the first reading is taken as it is");
        let mut value = 0.2;
        for _ in 0..10 {
            value = filter.step(Some(1000.0), &on(), 0.1);
        }
        assert!(value > 0.2 && value < 0.6, "one second is about a third of the way: {value}");
        for _ in 0..200 {
            value = filter.step(Some(1000.0), &on(), 0.1);
        }
        assert!((value - 1.0).abs() < 1e-3, "it settles on the target: {value}");
    }

    #[test]
    fn off_is_a_unit_scale_and_restarts_cleanly() {
        let mut filter = AmbientFilter::default();
        filter.step(Some(0.0), &on(), 0.1);
        let off = Ambient { enabled: false, ..on() };
        assert_eq!(filter.step(Some(0.0), &off, 0.1), 1.0);
        assert!((filter.step(Some(0.0), &on(), 0.1) - 0.2).abs() < 1e-9, "turning it back on starts from the reading");
    }
}
