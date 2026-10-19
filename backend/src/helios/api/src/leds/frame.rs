//! The ring drawn as a frame (16 LEDs, `0x00RRGGBB`), for the states that move every tick: the
//! rainbow locate, and the tags (a cyan breath, the spots, and the white sheen once the field
//! pose is locked). A recording's tally is a soft red LED 0 on top of either.
//!
//! This is HeliOS's fallback for the spots, which lemnosd does not draw yet (Lemnos's `spots`
//! block and `update_inputs` are a parallel change). The output is linear; lemnosd applies the
//! ring's gamma.

use super::settings::RING_LEDS;
use super::spots::{SHEEN_PERIOD_MS, SpotView, shortest_step, wrap};

pub type Rgb = [f64; 3];

const CYAN: Rgb = [40.0 / 255.0, 200.0 / 255.0, 1.0];
const RED: Rgb = [1.0, 40.0 / 255.0, 40.0 / 255.0];
const WHITE: Rgb = [1.0, 1.0, 1.0];

/// The rainbow comet: one turn every this many milliseconds, with a tail of this many LEDs.
const LOCATE_PERIOD_MS: f64 = 4000.0;
const LOCATE_TAIL: f64 = 6.0;
const LOCATE_BASE: f64 = 0.18;
/// The hue wheel turns once per this many milliseconds, so the colours drift round the ring.
const HUE_PERIOD_MS: f64 = 12000.0;
/// The glow's breath: a cosine over this many milliseconds.
const BREATH_PERIOD_MS: f64 = 4000.0;

/// The frame to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Scene<'a> {
    /// The rainbow comet going round.
    Locate,
    /// The tags: the cyan breath glow, the spots, and the white sheen when locked.
    Tags { spots: &'a [SpotView], locked: bool },
}

/// The 16 pixels of `scene` at `t_ms`, with the tally on LED 0 when `tally` is set.
pub fn render(scene: Scene<'_>, t_ms: u64, tally: bool) -> [u32; RING_LEDS] {
    let t = t_ms as f64;
    let mut out = [[0.0_f64; 3]; RING_LEDS];
    match scene {
        Scene::Locate => locate(&mut out, t),
        Scene::Tags { spots, locked } => tags(&mut out, t, spots, locked),
    }
    if tally {
        max_into(&mut out[0], RED.map(|c| c * 0.6));
    }
    out.map(to_pixel)
}

fn locate(out: &mut [Rgb; RING_LEDS], t: f64) {
    let head = wrap(t / LOCATE_PERIOD_MS * RING_LEDS as f64);
    let hue_offset = t / HUE_PERIOD_MS;
    for (i, pixel) in out.iter_mut().enumerate() {
        let distance = wrap(head - i as f64);
        let tail = if distance < LOCATE_TAIL { (1.0 - distance / LOCATE_TAIL).powf(2.2) } else { 0.0 };
        let level = (LOCATE_BASE + (1.0 - LOCATE_BASE) * tail) * 0.8;
        let hue = (i as f64 / RING_LEDS as f64 + hue_offset).rem_euclid(1.0);
        max_into(pixel, scale(hsv(hue, 0.85, 1.0), level));
    }
}

fn tags(out: &mut [Rgb; RING_LEDS], t: f64, spots: &[SpotView], locked: bool) {
    let breath = 0.5 - 0.5 * (std::f64::consts::TAU * t / BREATH_PERIOD_MS).cos();
    let glow = 0.20 + 0.08 * breath;
    for pixel in out.iter_mut() {
        max_into(pixel, scale(CYAN, glow));
    }
    // A locked pose warms the spots toward white.
    let spot_colour = if locked { mix(CYAN, WHITE, 0.6) } else { CYAN };
    for spot in spots {
        for (i, pixel) in out.iter_mut().enumerate() {
            let distance = circular_distance(i as f64, spot.position);
            let reach = (1.0 - distance / 1.5).max(0.0);
            if reach > 0.0 {
                max_into(pixel, scale(spot_colour, spot.level * reach * reach));
            }
        }
    }
    if locked {
        let sheen = wrap(t / SHEEN_PERIOD_MS * RING_LEDS as f64);
        for (i, pixel) in out.iter_mut().enumerate() {
            let reach = (1.0 - circular_distance(i as f64, sheen) / 2.0).max(0.0);
            max_into(pixel, scale(WHITE, 0.35 * reach * reach));
        }
    }
}

/// The distance between two ring positions, the short way round.
fn circular_distance(a: f64, b: f64) -> f64 {
    shortest_step(a, b).abs()
}

fn max_into(pixel: &mut Rgb, other: Rgb) {
    for (channel, value) in pixel.iter_mut().zip(other) {
        *channel = channel.max(value);
    }
}

fn scale(colour: Rgb, level: f64) -> Rgb {
    colour.map(|c| c * level.clamp(0.0, 1.0))
}

fn mix(a: Rgb, b: Rgb, amount: f64) -> Rgb {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * amount)
}

fn hsv(hue: f64, saturation: f64, value: f64) -> Rgb {
    let h = hue.rem_euclid(1.0) * 6.0;
    let sector = h.floor();
    let fraction = h - sector;
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * fraction);
    let t = value * (1.0 - saturation * (1.0 - fraction));
    match sector as u8 {
        0 => [value, t, p],
        1 => [q, value, p],
        2 => [p, value, t],
        3 => [p, q, value],
        4 => [t, p, value],
        _ => [value, p, q],
    }
}

fn to_pixel(rgb: Rgb) -> u32 {
    let channel = |c: f64| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
    (channel(rgb[0]) << 16) | (channel(rgb[1]) << 8) | channel(rgb[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brightness(pixel: u32) -> u32 {
        ((pixel >> 16) & 0xff).max((pixel >> 8) & 0xff).max(pixel & 0xff)
    }

    #[test]
    fn the_rainbow_comet_has_its_head_moving_round_the_ring() {
        let at = |t: u64| render(Scene::Locate, t, false);
        let brightest = |frame: [u32; RING_LEDS]| (0..RING_LEDS).max_by_key(|&i| brightness(frame[i])).expect("led");
        assert_eq!(brightest(at(0)), 0, "the head starts at LED 0");
        assert_eq!(brightest(at(1000)), 4, "a quarter turn in a quarter of the period");
        assert!(brightness(at(0)[15]) > 0 && brightness(at(0)[8]) > 0, "the glow is never bare: the tail trails behind the head");
        let frame = at(2000);
        assert_ne!(frame[0], frame[8], "the hues differ round the ring");
    }

    #[test]
    fn the_tags_glow_is_cyan_and_each_spot_is_brightest_at_its_position() {
        let spots = [SpotView { position: 5.0, level: 1.0 }];
        let frame = render(Scene::Tags { spots: &spots, locked: false }, 0, false);
        let brightest = (0..RING_LEDS).max_by_key(|&i| brightness(frame[i])).expect("led");
        assert_eq!(brightest, 5, "the spot is at its bearing");
        let red = (frame[5] >> 16) & 0xff;
        let blue = frame[5] & 0xff;
        assert!(blue > red, "cyan, not red: {:06x}", frame[5]);
        let glow = render(Scene::Tags { spots: &[], locked: false }, 0, false);
        assert!(brightness(glow[12]) > 0, "the glow shows without spots");
        assert!(brightness(frame[5]) > brightness(glow[5]));
    }

    #[test]
    fn a_locked_pose_warms_the_spots_and_adds_a_sheen() {
        let spots = [SpotView { position: 2.0, level: 1.0 }];
        let open = render(Scene::Tags { spots: &spots, locked: false }, 0, false);
        let locked = render(Scene::Tags { spots: &spots, locked: true }, 0, false);
        assert!((locked[2] >> 8) & 0xff > (open[2] >> 8) & 0xff, "warmer: more green in the spot");
        let sheen_at = |t: u64| render(Scene::Tags { spots: &[], locked: true }, t, false);
        assert_ne!(sheen_at(0), sheen_at(1500), "the sheen moves");
    }

    #[test]
    fn the_tally_lights_led_zero_in_soft_red() {
        let plain = render(Scene::Locate, 0, false);
        let tallied = render(Scene::Locate, 0, true);
        assert!((tallied[0] >> 16) & 0xff >= (plain[0] >> 16) & 0xff);
        assert!(((tallied[0] >> 16) & 0xff) > 100, "red");
        assert!(((tallied[0] >> 16) & 0xff) < 255, "soft, not full red");
        assert!(tallied[1..].iter().zip(&plain[1..]).all(|(a, b)| a == b), "only LED 0 changes");
    }

    #[test]
    fn every_pixel_is_a_valid_colour() {
        for t in (0..8000).step_by(313) {
            for pixel in render(Scene::Locate, t, true) {
                assert!(pixel <= 0x00ff_ffff);
            }
        }
    }
}
