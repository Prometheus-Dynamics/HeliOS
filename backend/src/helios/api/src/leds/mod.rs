//! The LED director: HeliOS's state on the board's status ring (`GET`/`PUT /v1/device/leds`).
//!
//! It runs in helios-api, which has the facts (the pipelines' detections and field pose, the
//! engine's state, the NetworkTables connection, the calibration and the recorder). Three parts:
//!
//! - [`collector`] (async, every tick) turns those into [`states::Facts`];
//! - [`director`] (pure) chooses the state from the facts, the settings and the overrides (locate,
//!   preview), and the look or frame it shows ([`states`], [`spots`], [`frame`], [`ambient`]);
//! - [`ring`] (a thread that owns lemnosd's clients) writes the step to the ring and publishes the
//!   live state and the `leds` events.
//!
//! The looks are data: `gaia/assets/runtime-config/etc/lemnos/looks.d/helios.toml`, read by
//! lemnosd. Only the tag spots, the rainbow locate and the tally on a frame are drawn here.

pub mod ambient;
pub mod bearing;
pub mod collector;
pub mod detections;
pub mod director;
pub mod frame;
pub mod ring;
pub mod settings;
pub mod spots;
pub mod states;

#[cfg(test)]
mod looks_test;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::{Mutex, PoisonError, mpsc};

use tokio::sync::watch;

use self::settings::LedSettings;
use self::states::{Facts, State};
use crate::SharedState;
use crate::host::now_ms;

/// A request for the ring thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    Locate { seconds: u64 },
    Preview { state: Option<State>, seconds: u64 },
}

/// The ring as it is now, for `GET /v1/device/leds`.
#[derive(Debug, Clone, PartialEq)]
pub struct Live {
    pub state: State,
    /// The look shown (`None` while a frame is shown).
    pub look: Option<String>,
    pub brightness: f64,
    pub tags: usize,
    pub locked: bool,
    pub recording: bool,
    pub lux: Option<f64>,
    /// The ring thread is connected to lemnosd.
    pub reachable: bool,
    pub error: Option<String>,
}

impl Default for Live {
    fn default() -> Self {
        Self { state: State::Idle, look: None, brightness: 0.0, tags: 0, locked: false, recording: false, lux: None, reachable: false, error: None }
    }
}

/// The preview and the locate, as the API last set them (they expire by their deadlines).
#[derive(Debug, Default, Clone, Copy)]
struct Overrides {
    preview: Option<(State, u64)>,
    locate_until: Option<u64>,
}

/// The director's handle in the API state: settings, commands and the live view.
pub struct LedHandle {
    settings: watch::Sender<LedSettings>,
    commands: mpsc::Sender<Command>,
    overrides: Mutex<Overrides>,
    live: Arc<Mutex<Live>>,
    worker: Mutex<Option<Worker>>,
}

/// The channel ends [`start`] hands to the collector and the ring thread.
struct Worker {
    facts: watch::Sender<Facts>,
    facts_rx: watch::Receiver<Facts>,
    commands: mpsc::Receiver<Command>,
}

impl Default for LedHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl LedHandle {
    pub fn new() -> Self {
        let (settings, _) = watch::channel(LedSettings::default());
        let (facts, facts_rx) = watch::channel(Facts::default());
        let (commands, command_rx) = mpsc::channel();
        Self {
            settings,
            commands,
            overrides: Mutex::new(Overrides::default()),
            live: Arc::new(Mutex::new(Live::default())),
            worker: Mutex::new(Some(Worker { facts, facts_rx, commands: command_rx })),
        }
    }

    pub fn settings(&self) -> LedSettings {
        self.settings.borrow().clone()
    }

    pub fn set_settings(&self, settings: LedSettings) {
        self.settings.send_replace(settings);
    }

    /// Shows the locate state for `seconds`.
    pub fn locate(&self, seconds: u64) {
        self.overrides.lock().unwrap_or_else(PoisonError::into_inner).locate_until = Some(now_ms() + seconds * 1000);
        let _ = self.commands.send(Command::Locate { seconds });
    }

    /// Shows `state` for `seconds` (`None` ends the preview).
    pub fn preview(&self, state: Option<State>, seconds: u64) {
        let now = now_ms();
        self.overrides.lock().unwrap_or_else(PoisonError::into_inner).preview = state.map(|state| (state, now + seconds * 1000));
        let _ = self.commands.send(Command::Preview { state, seconds });
    }

    /// The preview and the locate deadline, while they run.
    pub fn overrides(&self) -> (Option<(State, u64)>, Option<u64>) {
        let now = now_ms();
        let overrides = *self.overrides.lock().unwrap_or_else(PoisonError::into_inner);
        (overrides.preview.filter(|(_, until)| *until > now), overrides.locate_until.filter(|until| *until > now))
    }

    pub fn live(&self) -> Live {
        self.live.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// Starts the collector and the ring thread, once, for the server's lifetime. `socket` is
/// lemnosd's socket. Without a start, the settings and the routes still work, and nothing is
/// written to the ring.
pub fn start(state: &SharedState, socket: PathBuf) {
    let Some(worker) = state.leds.worker.lock().unwrap_or_else(PoisonError::into_inner).take() else { return };
    collector::spawn(state.clone(), worker.facts, state.leds.settings.clone());
    let thread = ring::RingThread { socket, live: state.leds.live.clone(), hub: state.events.clone(), facts: worker.facts_rx, settings: state.leds.settings.subscribe(), commands: worker.commands };
    std::thread::Builder::new().name("helios-leds".into()).spawn(move || ring::run(thread)).expect("the LED ring thread");
}
