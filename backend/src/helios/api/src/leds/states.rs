//! The LED director's states: what the ring says about HeliOS, chosen by priority from the facts,
//! and the look (or frame) each one shows. The look names are HeliOS's own look file
//! (`gaia/assets/runtime-config/etc/lemnos/looks.d/helios.toml`, the `helios.*` looks).
//!
//! Highest priority first: locate (timed), error or camera lost, calibration capture (with its
//! ripple on each view), the robot enabling (a one-shot ripple), robot code not connected, tags
//! visible (locked or not), searching, starting, then idle. A recording adds the tally on top of
//! whatever shows (its `helios.rec.*` look, or the tally pixel in a frame).

use serde::Serialize;

/// The states the director can show. `name` is the API's name; `preview` takes the same names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Locate,
    Error,
    Calibrating,
    RobotWait,
    RobotWaitTags,
    Tags,
    TagsLocked,
    Searching,
    RobotEnabled,
    Starting,
    Recording,
    Idle,
}

impl State {
    pub const ALL: [State; 12] = [
        State::Locate,
        State::Error,
        State::Calibrating,
        State::RobotWait,
        State::RobotWaitTags,
        State::Tags,
        State::TagsLocked,
        State::Searching,
        State::RobotEnabled,
        State::Starting,
        State::Recording,
        State::Idle,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Locate => "locate",
            Self::Error => "error",
            Self::Calibrating => "calibrating",
            Self::RobotWait => "robot-wait",
            Self::RobotWaitTags => "robot-wait-tags",
            Self::Tags => "tags",
            Self::TagsLocked => "tags-locked",
            Self::Searching => "searching",
            Self::RobotEnabled => "robot-enabled",
            Self::Starting => "starting",
            Self::Recording => "recording",
            Self::Idle => "idle",
        }
    }

    pub fn parse(name: &str) -> Option<State> {
        Self::ALL.into_iter().find(|state| state.name() == name)
    }

    /// The label the UI shows for the state.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Locate => "Locate",
            Self::Error => "Error or camera lost",
            Self::Calibrating => "Calibration capture",
            Self::RobotWait => "Robot code not connected",
            Self::RobotWaitTags => "Robot not connected, tags visible",
            Self::Tags => "Tags visible",
            Self::TagsLocked => "Field pose locked",
            Self::Searching => "Searching",
            Self::RobotEnabled => "Robot enabled",
            Self::Starting => "Starting",
            Self::Recording => "Recording",
            Self::Idle => "Idle",
        }
    }

    /// The state that is drawn as a frame by helios-api (the rainbow comet). Everything else is a
    /// look from the look file; the tags' looks take their spots as inputs, and a frame is the
    /// fallback for a lemnosd without spots (see `ring.rs`).
    pub const fn is_frame(self) -> bool {
        matches!(self, Self::Locate)
    }

    /// Whether the state's look takes spots (the tags).
    pub const fn has_spots(self) -> bool {
        matches!(self, Self::Tags | Self::TagsLocked)
    }

    /// The look a state shows, without a recording tally. `None` for the frame state.
    pub const fn look(self) -> Option<&'static str> {
        match self {
            Self::Locate => None,
            Self::Tags => Some("helios.tags"),
            Self::TagsLocked => Some("helios.tags-locked"),
            Self::Error => Some("helios.error"),
            Self::Calibrating => Some("helios.calibrating"),
            Self::RobotWait => Some("helios.robot-wait"),
            Self::RobotWaitTags => Some("helios.robot-wait-tags"),
            Self::Searching => Some("helios.searching"),
            Self::RobotEnabled => Some("helios.robot-enabled"),
            Self::Starting => Some("helios.starting"),
            Self::Recording => Some("helios.recording"),
            // lemnosd's built-in empty look: nothing from HeliOS on the ring.
            Self::Idle => Some("status.off"),
        }
    }

    /// The look a state shows with the recording tally on. `None` for locate, whose frame carries
    /// the tally itself.
    pub const fn tally_look(self) -> Option<&'static str> {
        match self {
            Self::Locate => None,
            Self::Tags => Some("helios.rec.tags"),
            Self::TagsLocked => Some("helios.rec.tags-locked"),
            Self::Error => Some("helios.rec.error"),
            Self::Calibrating => Some("helios.rec.calibrating"),
            Self::RobotWait => Some("helios.rec.robot-wait"),
            Self::RobotWaitTags => Some("helios.rec.robot-wait-tags"),
            Self::Searching => Some("helios.rec.searching"),
            Self::RobotEnabled => Some("helios.rec.robot-enabled"),
            Self::Starting => Some("helios.rec.starting"),
            Self::Recording | Self::Idle => Some("helios.recording"),
        }
    }
}

/// The look a state shows, with the calibration ripple (on a captured view) and the tally.
pub fn look_for(state: State, tally: bool, view_pulse: bool) -> Option<&'static str> {
    if state == State::Calibrating && view_pulse {
        return Some(if tally { "helios.rec.calibrating-view" } else { "helios.calibrating-view" });
    }
    if tally { state.tally_look() } else { state.look() }
}

/// How long a look change fades on the ring, in milliseconds (lemnosd fades every look change
/// from what is on the ring). A start-up look cross-fades into the steady state slowly; an error
/// shows at once.
pub const fn fade_ms(from: State, to: State) -> u32 {
    match (from, to) {
        (_, State::Error) => 200,
        (State::Starting, _) => 1000,
        (_, State::Locate) => 300,
        (_, State::Idle) => 600,
        _ => 400,
    }
}

/// Calibration coverage: the views captured per camera that count as a full arc.
pub const CALIBRATION_VIEWS: u32 = 12;

/// A calibration capture session (`calibration_capture.rs`), from the camera's views.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Calibration {
    /// Views captured, as a fraction of `CALIBRATION_VIEWS` (0 to 1).
    pub progress: f64,
    pub views: u32,
}

/// A tag seen in the latest detections: its id, bearing and apparent size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TagObs {
    pub id: u32,
    /// Degrees from the camera's forward axis, positive to the right of the image centre.
    pub bearing_deg: f64,
    /// The tag's side length as a fraction of the frame width.
    pub size: f64,
}

/// What the director knows, sampled every tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    /// HeliOS is still coming up (no pipeline has run yet, shortly after start).
    pub starting: bool,
    /// The engine failed, or the camera is lost.
    pub error: bool,
    pub calibration: Option<Calibration>,
    pub recording: bool,
    /// The team number and the NetworkTables server are set, and robot code is not connected.
    pub robot_missing: bool,
    /// The robot is enabled (not yet published by helios-engine; see `collector.rs`).
    pub robot_enabled: bool,
    pub tags: Vec<TagObs>,
    /// A multi-tag field pose is valid.
    pub locked: bool,
    /// A pipeline is running and no tags are visible.
    pub searching: bool,
    /// Ambient light in lux (not yet published by helios-engine; see `collector.rs`).
    pub lux: Option<f64>,
}

/// The transient overlays that run for a while after an edge: the enable ripple and a calibration
/// view's ripple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Overlays {
    pub robot_enabled_ripple: bool,
}

/// The base state from the facts, before recording and overrides.
pub fn base_state(facts: &Facts, overlays: Overlays) -> State {
    if facts.error {
        return State::Error;
    }
    if facts.calibration.is_some() {
        return State::Calibrating;
    }
    if overlays.robot_enabled_ripple {
        return State::RobotEnabled;
    }
    if facts.robot_missing {
        return if facts.tags.is_empty() { State::RobotWait } else { State::RobotWaitTags };
    }
    if !facts.tags.is_empty() {
        return if facts.locked { State::TagsLocked } else { State::Tags };
    }
    if facts.searching {
        return State::Searching;
    }
    if facts.starting {
        return State::Starting;
    }
    State::Idle
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StateInfo {
    pub name: &'static str,
    pub label: &'static str,
    pub look: Option<&'static str>,
}

/// The states for the UI's picker, in priority order.
pub fn state_list() -> Vec<StateInfo> {
    State::ALL.into_iter().map(|state| StateInfo { name: state.name(), label: state.label(), look: state.look() }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(id: u32) -> TagObs {
        TagObs { id, bearing_deg: 0.0, size: 0.08 }
    }

    #[test]
    fn names_round_trip_and_are_unique() {
        for state in State::ALL {
            assert_eq!(State::parse(state.name()), Some(state));
        }
        assert_eq!(State::parse("nope"), None);
        let mut names: Vec<_> = State::ALL.iter().map(|state| state.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), State::ALL.len());
    }

    #[test]
    fn priorities_run_from_error_down_to_idle() {
        let base = Facts::default();
        assert_eq!(base_state(&base, Overlays::default()), State::Idle);
        let starting = Facts { starting: true, ..Facts::default() };
        assert_eq!(base_state(&starting, Overlays::default()), State::Starting);
        let searching = Facts { searching: true, starting: true, ..Facts::default() };
        assert_eq!(base_state(&searching, Overlays::default()), State::Searching, "a running pipeline beats starting");
        let tags = Facts { tags: vec![tag(1)], searching: true, ..Facts::default() };
        assert_eq!(base_state(&tags, Overlays::default()), State::Tags, "tags beat searching");
        let locked = Facts { locked: true, ..tags.clone() };
        assert_eq!(base_state(&locked, Overlays::default()), State::TagsLocked);
        let robot = Facts { robot_missing: true, ..tags.clone() };
        assert_eq!(base_state(&robot, Overlays::default()), State::RobotWaitTags, "no robot beats tags");
        let no_tags = Facts { robot_missing: true, ..Facts::default() };
        assert_eq!(base_state(&no_tags, Overlays::default()), State::RobotWait);
        let calibrating = Facts { calibration: Some(Calibration { progress: 0.2, views: 2 }), ..robot.clone() };
        assert_eq!(base_state(&calibrating, Overlays::default()), State::Calibrating, "calibration beats the robot");
        let broken = Facts { error: true, ..calibrating.clone() };
        assert_eq!(base_state(&broken, Overlays::default()), State::Error, "an error beats calibration");
    }

    #[test]
    fn the_enable_ripple_sits_above_the_steady_states_but_below_error_and_calibration() {
        let ripple = Overlays { robot_enabled_ripple: true };
        assert_eq!(base_state(&Facts::default(), ripple), State::RobotEnabled);
        let tags = Facts { tags: vec![tag(1)], ..Facts::default() };
        assert_eq!(base_state(&tags, ripple), State::RobotEnabled);
        let calibrating = Facts { calibration: Some(Calibration { progress: 0.5, views: 6 }), ..Facts::default() };
        assert_eq!(base_state(&calibrating, ripple), State::Calibrating);
        let error = Facts { error: true, ..Facts::default() };
        assert_eq!(base_state(&error, ripple), State::Error);
    }

    #[test]
    fn looks_follow_the_state_and_the_tally_and_the_view_pulse() {
        assert_eq!(look_for(State::Searching, false, false), Some("helios.searching"));
        assert_eq!(look_for(State::Searching, true, false), Some("helios.rec.searching"));
        assert_eq!(look_for(State::Calibrating, false, true), Some("helios.calibrating-view"));
        assert_eq!(look_for(State::Calibrating, true, true), Some("helios.rec.calibrating-view"));
        assert_eq!(look_for(State::Calibrating, false, false), Some("helios.calibrating"));
        assert_eq!(look_for(State::Idle, false, false), Some("status.off"), "idle is lemnosd's empty look");
        assert_eq!(look_for(State::Recording, false, false), Some("helios.recording"));
        assert_eq!(look_for(State::Locate, true, false), None, "a frame state has no look");
        assert!(State::Locate.is_frame() && !State::Tags.is_frame() && !State::Error.is_frame());
        assert!(State::Tags.has_spots() && State::TagsLocked.has_spots() && !State::Searching.has_spots());
        assert_eq!(look_for(State::TagsLocked, true, false), Some("helios.rec.tags-locked"));
    }

    #[test]
    fn the_starting_look_cross_fades_slowly_and_an_error_shows_at_once() {
        assert_eq!(fade_ms(State::Starting, State::Searching), 1000);
        assert_eq!(fade_ms(State::Searching, State::Error), 200);
        assert_eq!(fade_ms(State::Searching, State::Tags), 400);
        assert_eq!(fade_ms(State::Searching, State::Idle), 600);
    }

    #[test]
    fn every_look_state_names_a_look_in_the_look_file_convention() {
        for state in State::ALL {
            for name in [state.look(), state.tally_look()].into_iter().flatten() {
                // lemnosd's built-in empty look is the one name outside HeliOS's own.
                if name == "status.off" {
                    continue;
                }
                assert!(name.starts_with("helios."), "{name}");
                assert!(name.len() <= 40, "a Lemnos look name is at most 40 characters: {name}");
            }
        }
        assert_eq!(state_list().len(), State::ALL.len());
    }
}
