//! The director: one tick turns the facts, the settings and the overrides (locate, preview) into
//! what the ring shows. It is pure: the ring thread supplies the clock and does the sending.

use super::ambient::AmbientFilter;
use super::frame::{self, Scene};
use super::settings::LedSettings;
use super::spots::{SpotBank, SpotView, sheen_position, targets};
use super::states::{Calibration, Facts, Overlays, State, TagObs, base_state, fade_ms, look_for};

/// How long the enable ripple runs after the robot is enabled.
pub const RIPPLE_MS: u64 = 2500;
/// How long a captured calibration view's ripple runs.
pub const VIEW_PULSE_MS: u64 = 2000;
pub const LOCATE_SECONDS: u64 = 10;
pub const PREVIEW_SECONDS: u64 = 4;
/// The ring brightness boost while the robot is enabled.
pub const BOOST: f64 = 1.3;

/// What the ring should show now.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub state: State,
    /// The look to show; `None` when `leds` carries the frame.
    pub look: Option<&'static str>,
    /// The calibration arc's progress (0 to 1), in the calibrating looks.
    pub progress: Option<f64>,
    /// The fade for this change in ms (0: the same look as before).
    pub fade_ms: u32,
    /// The 16 pixels: the frame for locate, and the tags' fallback frame (for a lemnosd without
    /// spots) for the tags. `0x00RRGGBB`, LED 0 first.
    pub leds: Option<[u32; 16]>,
    /// The look takes the spots as inputs (the tags).
    pub spot_look: bool,
    /// The ring brightness, 0 to 1 (lemnosd's ring-wide scale).
    pub brightness: f64,
    pub spots: Vec<SpotView>,
    pub sheen: Option<f64>,
    pub tags: usize,
    pub locked: bool,
    pub recording: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Last {
    state: State,
    look: Option<&'static str>,
}

#[derive(Debug, Default)]
pub struct Director {
    spots: SpotBank,
    ambient: AmbientFilter,
    last: Option<Last>,
    last_ms: Option<u64>,
    ripple_until: u64,
    view_until: u64,
    prev_robot_enabled: bool,
    prev_views: u32,
    locate_until: Option<u64>,
    preview: Option<(State, u64)>,
}

impl Director {
    /// Shows the locate state for `seconds` (over everything else).
    pub fn locate(&mut self, now: u64, seconds: u64) {
        self.locate_until = Some(now + seconds * 1000);
    }

    /// Shows `state` for `seconds` (`None` ends the preview).
    pub fn preview(&mut self, state: Option<State>, now: u64, seconds: u64) {
        self.preview = state.map(|state| (state, now + seconds * 1000));
    }

    /// The locate deadline, while it runs.
    pub fn locate_until(&self) -> Option<u64> {
        self.locate_until
    }

    /// The preview state and its deadline, while one runs.
    pub fn preview_state(&self) -> Option<(State, u64)> {
        self.preview
    }

    /// One tick at `now` (ms).
    pub fn step(&mut self, facts: &Facts, settings: &LedSettings, now: u64) -> Step {
        let dt = self.last_ms.map_or(0.0, |last| (now.saturating_sub(last) as f64 / 1000.0).min(0.5));
        self.last_ms = Some(now);
        let views = facts.calibration.map_or(0, |calibration| calibration.views);
        if self.last.is_none() {
            // The first tick is not an edge: a robot already enabled at start shows no ripple.
            self.prev_robot_enabled = facts.robot_enabled;
            self.prev_views = views;
        }
        if facts.robot_enabled && !self.prev_robot_enabled {
            self.ripple_until = now + RIPPLE_MS;
        }
        if views > self.prev_views {
            self.view_until = now + VIEW_PULSE_MS;
        }
        self.prev_robot_enabled = facts.robot_enabled;
        self.prev_views = views;
        if self.locate_until.is_some_and(|until| until <= now) {
            self.locate_until = None;
        }
        if self.preview.is_some_and(|(_, until)| until <= now) {
            self.preview = None;
        }

        let overlays = Overlays { robot_enabled_ripple: now < self.ripple_until };
        let (base, facts) = if self.locate_until.is_some() {
            (State::Locate, facts.clone())
        } else if let Some((state, _)) = self.preview {
            (state, preview_facts(state, facts))
        } else {
            (base_state(facts, overlays), facts.clone())
        };
        let tally = settings.tally && facts.recording;
        let state = if base == State::Idle && tally { State::Recording } else { base };

        let tag_targets = match state {
            State::Tags | State::TagsLocked => targets(&facts.tags, settings.forward_led, settings.direction),
            _ => Vec::new(),
        };
        self.spots.update(&tag_targets, dt);
        let spots = self.spots.views();

        let view_pulse = now < self.view_until && state == State::Calibrating;
        let look = look_for(state, tally, view_pulse);
        // The rainbow is a frame. The tags' look takes its spots as inputs; its frame is the
        // fallback for a lemnosd without spots (the ring thread chooses).
        let leds = if state.is_frame() {
            Some(frame::render(Scene::Locate, now, tally))
        } else if state.has_spots() {
            Some(frame::render(Scene::Tags { spots: &spots, locked: state == State::TagsLocked }, now, tally))
        } else {
            None
        };
        let look = if state.is_frame() { None } else { look };
        let progress = match (state, facts.calibration) {
            (State::Calibrating, Some(Calibration { progress, .. })) => Some(progress.clamp(0.0, 1.0)),
            _ => None,
        };

        let boost = if settings.boost && facts.robot_enabled { BOOST } else { 1.0 };
        let ambient = self.ambient.step(facts.lux, &settings.ambient, dt);
        let brightness = (settings.brightness * boost * ambient).clamp(0.0, 1.0);

        let current = Last { state, look };
        let fade = match self.last {
            Some(last) if last != current => fade_ms(last.state, state),
            Some(_) => 0,
            None => 0,
        };
        self.last = Some(current);

        Step {
            state,
            look,
            progress,
            fade_ms: fade,
            leds,
            spot_look: state.has_spots(),
            brightness,
            spots,
            sheen: (state == State::TagsLocked).then(|| sheen_position(now)),
            tags: facts.tags.len(),
            locked: facts.locked,
            recording: facts.recording,
        }
    }
}

/// The facts a previewed state shows: its own, with demo tags and a half-way calibration where
/// they are needed. The recording and the ambient light stay as they are.
pub fn preview_facts(state: State, base: &Facts) -> Facts {
    let mut facts = Facts { recording: base.recording, lux: base.lux, ..Facts::default() };
    match state {
        State::Error => facts.error = true,
        State::Calibrating => facts.calibration = Some(Calibration { progress: 0.6, views: 7 }),
        State::RobotWait => facts.robot_missing = true,
        State::RobotWaitTags => {
            facts.robot_missing = true;
            facts.tags = demo_tags();
        }
        State::Tags => facts.tags = demo_tags(),
        State::TagsLocked => {
            facts.tags = demo_tags();
            facts.locked = true;
        }
        State::Searching => facts.searching = true,
        State::RobotEnabled => facts.robot_enabled = true,
        State::Starting => facts.starting = true,
        State::Recording | State::Idle | State::Locate => {}
    }
    facts
}

fn demo_tags() -> Vec<TagObs> {
    vec![TagObs { id: 1, bearing_deg: -30.0, size: 0.09 }, TagObs { id: 2, bearing_deg: 18.0, size: 0.06 }]
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 10_000;

    fn settings() -> LedSettings {
        LedSettings::default()
    }

    fn tag(id: u32, bearing_deg: f64) -> TagObs {
        TagObs { id, bearing_deg, size: 0.08 }
    }

    /// Runs `facts` for `millis` ms in 100 ms steps and returns the last step.
    fn run(director: &mut Director, facts: &Facts, settings: &LedSettings, from: u64, millis: u64) -> Step {
        let mut step = None;
        let mut now = from;
        while now < from + millis {
            step = Some(director.step(facts, settings, now));
            now += 100;
        }
        step.expect("a step")
    }

    #[test]
    fn a_robot_already_enabled_at_start_shows_no_ripple() {
        let mut director = Director::default();
        let enabled = Facts { robot_enabled: true, ..Facts::default() };
        let step = director.step(&enabled, &settings(), T0);
        assert_eq!(step.state, State::Idle, "no ripple on the first tick");
        assert!(step.brightness > 0.6, "but the boost applies while enabled");
    }

    #[test]
    fn the_enable_edge_ripples_then_settles() {
        let mut director = Director::default();
        let off = Facts { searching: true, ..Facts::default() };
        director.step(&off, &settings(), T0);
        let on = Facts { robot_enabled: true, ..off.clone() };
        assert_eq!(director.step(&on, &settings(), T0 + 100).state, State::RobotEnabled);
        let later = run(&mut director, &on, &settings(), T0 + 200, RIPPLE_MS + 200);
        assert_eq!(later.state, State::Searching, "the ripple is one-shot");
    }

    #[test]
    fn locate_is_on_top_and_ends_after_its_time() {
        let mut director = Director::default();
        let errored = Facts { error: true, ..Facts::default() };
        director.locate(T0, 2);
        let during = director.step(&errored, &settings(), T0 + 500);
        assert_eq!(during.state, State::Locate);
        assert!(during.leds.is_some(), "locate is a frame");
        let after = director.step(&errored, &settings(), T0 + 2500);
        assert_eq!(after.state, State::Error);
    }

    #[test]
    fn a_captured_view_pulses_the_calibration_ripple_once() {
        let mut director = Director::default();
        let one = Facts { calibration: Some(Calibration { progress: 0.1, views: 1 }), ..Facts::default() };
        director.step(&one, &settings(), T0);
        let two = Facts { calibration: Some(Calibration { progress: 0.2, views: 2 }), ..Facts::default() };
        let pulse = director.step(&two, &settings(), T0 + 100);
        assert_eq!(pulse.look, Some("helios.calibrating-view"));
        assert_eq!(pulse.progress, Some(0.2), "the arc follows the coverage");
        let settled = run(&mut director, &two, &settings(), T0 + 200, VIEW_PULSE_MS + 200);
        assert_eq!(settled.look, Some("helios.calibrating"));
    }

    #[test]
    fn a_recording_adds_the_tally_to_each_state() {
        let mut director = Director::default();
        let recording = Facts { recording: true, searching: true, ..Facts::default() };
        let step = director.step(&recording, &settings(), T0);
        assert_eq!(step.look, Some("helios.rec.searching"));
        let idle = director.step(&Facts { recording: true, ..Facts::default() }, &settings(), T0 + 100);
        assert_eq!(idle.state, State::Recording);
        assert_eq!(idle.look, Some("helios.recording"));
        let tags = director.step(&Facts { recording: true, tags: vec![tag(1, 0.0)], ..Facts::default() }, &settings(), T0 + 200);
        assert_eq!(tags.state, State::Tags);
        let plain = director.step(&Facts { tags: vec![tag(1, 0.0)], ..Facts::default() }, &settings(), T0 + 300);
        let red = |step: &Step| step.leds.expect("a frame")[0] >> 16 & 0xff;
        assert!(red(&tags) > red(&plain) + 100, "the tally is soft red on LED 0: {} against {}", red(&tags), red(&plain));
    }

    #[test]
    fn the_tally_option_turns_the_tally_off() {
        let mut director = Director::default();
        let no_tally = LedSettings { tally: false, ..settings() };
        let step = director.step(&Facts { recording: true, searching: true, ..Facts::default() }, &no_tally, T0);
        assert_eq!(step.look, Some("helios.searching"));
    }

    #[test]
    fn robot_enabled_boosts_the_ring_by_thirty_percent_and_the_option_can_turn_it_off() {
        let mut director = Director::default();
        let plain = director.step(&Facts::default(), &settings(), T0).brightness;
        let on = Facts { robot_enabled: true, ..Facts::default() };
        let boosted = director.step(&on, &settings(), T0 + 100).brightness;
        assert!((boosted / plain - BOOST).abs() < 1e-9, "{plain} {boosted}");
        let no_boost = LedSettings { boost: false, ..settings() };
        let steady = director.step(&on, &no_boost, T0 + 200).brightness;
        assert!((steady - plain).abs() < 1e-9);
    }

    #[test]
    fn the_ring_fades_when_its_state_changes_and_holds_otherwise() {
        let mut director = Director::default();
        let starting = Facts { starting: true, ..Facts::default() };
        director.step(&starting, &settings(), T0);
        let steady = Facts { searching: true, ..Facts::default() };
        let change = director.step(&steady, &settings(), T0 + 100);
        assert_eq!(change.fade_ms, 1000, "the start-up look cross-fades slowly into the steady state");
        assert_eq!(director.step(&steady, &settings(), T0 + 200).fade_ms, 0);
    }

    #[test]
    fn tags_move_their_spots_round_the_ring() {
        let mut director = Director::default();
        let tags = Facts { tags: vec![tag(1, 90.0)], ..Facts::default() };
        let step = run(&mut director, &tags, &settings(), T0, 3000);
        assert_eq!(step.state, State::Tags);
        assert_eq!(step.spots.len(), 1);
        assert!((step.spots[0].position - 4.0).abs() < 0.05, "a quarter turn clockwise: {}", step.spots[0].position);
        assert_eq!(step.look, Some("helios.tags"), "the tags are a look that takes the spots");
        assert!(step.spot_look && step.sheen.is_none() && step.leds.is_some(), "with the frame as its fallback");
        let locked = Facts { locked: true, ..tags };
        let warm = run(&mut director, &locked, &settings(), T0 + 3000, 1000);
        assert_eq!(warm.state, State::TagsLocked);
        assert!(warm.sheen.is_some());
    }

    #[test]
    fn a_preview_shows_its_state_and_ends() {
        let mut director = Director::default();
        director.preview(Some(State::RobotWaitTags), T0, 2);
        let step = director.step(&Facts::default(), &settings(), T0 + 100);
        assert_eq!(step.state, State::RobotWaitTags);
        assert_eq!(step.look, Some("helios.robot-wait-tags"));
        assert_eq!(director.step(&Facts::default(), &settings(), T0 + 2500).state, State::Idle);
        director.preview(Some(State::Calibrating), T0, 9);
        director.preview(None, T0, 9);
        assert_eq!(director.step(&Facts::default(), &settings(), T0 + 100).state, State::Idle, "stopping ends it");
    }

    #[test]
    fn the_ambient_light_scales_the_brightness_when_it_is_on() {
        let mut director = Director::default();
        let ambient = LedSettings { ambient: super::super::settings::Ambient { enabled: true, min: 0.2, max: 1.0 }, ..settings() };
        let dark = Facts { lux: Some(0.0), ..Facts::default() };
        let first = director.step(&dark, &ambient, T0).brightness;
        assert!((first - 0.5 * 0.2).abs() < 1e-9, "the first reading is taken as it is: {first}");
        let plain = director.step(&Facts::default(), &settings(), T0 + 100).brightness;
        assert!((plain - 0.5).abs() < 1e-9, "off: the brightness as set");
    }
}
