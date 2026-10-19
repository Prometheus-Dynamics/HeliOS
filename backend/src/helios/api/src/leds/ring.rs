//! The ring thread: owns lemnosd's LED and device clients (blocking, like the peripherals bridge),
//! runs the director each tick, writes its step to the ring, publishes the live state for
//! `GET /v1/device/leds`, and emits the `leds` events (about 10 a second).
//!
//! The client is a normal application client at [`DIRECTOR_PRIORITY`], without `keep_intents`:
//! when helios-api stops, lemnosd drops its looks and the peripherals' status shows again.
//!
//! The tags are a look with a spots block: the spots are inputs (`look_spots`, then
//! `update_inputs`, and lemnosd glides them). A lemnosd without spots refuses those requests, and
//! the ring then shows the director's frame (the same glow and spots, drawn here) and retries the
//! look after [`NATIVE_RETRY_MS`].

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use lemnos_ipc::{ClientError, ClientOptions, DeviceClient, LedClient, LedRequest, LedShow, Spot, SpotList};
use serde_json::json;
use tokio::sync::watch;

use super::director::{Director, Step};
use super::settings::LedSettings;
use super::spots::{RING, SpotView};
use super::states::Facts;
use super::{Command, Live};
use crate::events::EventHub;
use crate::host::now_ms;

/// The priority of HeliOS's director: above the peripherals' status fallback (led.rs, 20) and the
/// default (50).
pub const DIRECTOR_PRIORITY: u8 = 160;
const CLIENT_NAME: &str = "helios-leds";
/// The Raze's status ring, as the board definition names it.
pub const RING_DEVICE: &str = "status-ring";
const RECONNECT_MS: u64 = 2000;
const FRAME_MS: u64 = 33;
const TICK_MS: u64 = 100;
const EVENT_MS: u64 = 100;
/// A look with a moving arc is sent again at most this often.
const PROGRESS_MS: u64 = 250;
/// Brightness is written when it moves by at least this much.
const BRIGHTNESS_STEP: f64 = 0.004;
/// A lemnosd that refused the spots is left alone for this long before the look is tried again.
const NATIVE_RETRY_MS: u64 = 30_000;
/// lemnosd's spots block shows at most this many spots.
const MAX_NATIVE_SPOTS: usize = 8;

// Above lemnosd's default client priority (50) and the peripherals' status fallback (20,
// peripherals led.rs `LED_PRIORITY`).
const _: () = assert!(DIRECTOR_PRIORITY > 50 && DIRECTOR_PRIORITY > 20);

pub struct RingThread {
    pub socket: PathBuf,
    pub live: Arc<Mutex<Live>>,
    pub hub: Arc<EventHub>,
    pub facts: watch::Receiver<Facts>,
    pub settings: watch::Receiver<LedSettings>,
    pub commands: Receiver<Command>,
}

/// Runs until the command channel closes (the API is stopping).
pub fn run(thread: RingThread) {
    let mut director = Director::default();
    let mut sink = Sink::new(thread.socket.clone());
    let mut enabled_before = true;
    let mut last_event = 0;
    loop {
        let now = now_ms();
        let settings = thread.settings.borrow().clone();
        let facts = thread.facts.borrow().clone();
        let delay = if settings.enabled {
            let step = director.step(&facts, &settings, now);
            sink.apply(&step, now);
            enabled_before = true;
            if now.saturating_sub(last_event) >= EVENT_MS {
                last_event = now;
                thread.hub.publish("leds", event_data(&step));
            }
            publish(&thread.live, live_of(&step, &facts, &sink));
            if step.leds.is_some() { FRAME_MS } else { TICK_MS }
        } else {
            if enabled_before {
                sink.release();
                enabled_before = false;
            }
            publish(&thread.live, Live { reachable: sink.reachable(), error: sink.error(), ..Live::default() });
            TICK_MS
        };
        match thread.commands.recv_timeout(Duration::from_millis(delay)) {
            Ok(command) => apply_command(&mut director, command, now_ms()),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    sink.release();
}

fn apply_command(director: &mut Director, command: Command, now: u64) {
    match command {
        Command::Locate { seconds } => director.locate(now, seconds),
        Command::Preview { state, seconds } => director.preview(state, now, seconds),
    }
}

fn publish(live: &Mutex<Live>, next: Live) {
    *live.lock().unwrap_or_else(PoisonError::into_inner) = next;
}

fn live_of(step: &Step, facts: &Facts, sink: &Sink) -> Live {
    Live {
        state: step.state,
        look: step.look.map(str::to_string),
        brightness: step.brightness,
        tags: step.tags,
        locked: step.locked,
        recording: step.recording,
        lux: facts.lux,
        reachable: sink.reachable(),
        error: sink.error(),
    }
}

/// The `leds` event: the state, the look, the spots, and the frame when the ring shows one.
fn event_data(step: &Step) -> serde_json::Value {
    json!({
        "state": step.state.name(),
        "look": step.look,
        "brightness": step.brightness,
        "tags": step.tags,
        "locked": step.locked,
        "recording": step.recording,
        "leds": step.leds.map(|pixels| pixels.map(|pixel| format!("{pixel:06x}"))),
        "spots": step.spots,
        "sheen": step.sheen,
    })
}

/// The director's spots as lemnosd's: thousandths of a turn from LED 0, and levels in
/// thousandths. The brightest eight, when there are more.
pub fn native_spots(spots: &[SpotView]) -> Vec<Spot> {
    let mut sorted: Vec<&SpotView> = spots.iter().collect();
    sorted.sort_by(|a, b| b.level.total_cmp(&a.level));
    sorted
        .into_iter()
        .take(MAX_NATIVE_SPOTS)
        .map(|spot| {
            let at = ((spot.position / RING) * 1000.0).round() as u16 % 1000;
            let level = (spot.level.clamp(0.0, 1.0) * 1000.0).round() as u16;
            Spot::new(at, level)
        })
        .collect()
}

/// The writer: the LED client for the ring's look, spots or frame, and the device client for the
/// ring-wide brightness. Both reconnect after a failure, every couple of seconds.
struct Sink {
    socket: PathBuf,
    leds: Option<LedClient>,
    devices: Option<DeviceClient>,
    retry_at: u64,
    /// Until this time, the tags show their frame (a lemnosd refused the spots).
    native_retry_at: u64,
    applied: Option<Applied>,
    brightness: Option<f64>,
    last_frame_at: u64,
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct Applied {
    look: &'static str,
    progress: Option<u16>,
    sent_at: u64,
}

impl Sink {
    fn new(socket: PathBuf) -> Self {
        Self { socket, leds: None, devices: None, retry_at: 0, native_retry_at: 0, applied: None, brightness: None, last_frame_at: 0, error: None }
    }

    fn reachable(&self) -> bool {
        self.leds.is_some()
    }

    fn error(&self) -> Option<String> {
        self.error.clone()
    }

    fn connect(&mut self, now: u64) {
        if now < self.retry_at {
            return;
        }
        if self.leds.is_none() {
            match ClientOptions::new(&self.socket, CLIENT_NAME).priority(DIRECTOR_PRIORITY).reconnecting().leds() {
                Ok(leds) => {
                    self.leds = Some(leds);
                    self.applied = None;
                    self.error = None;
                }
                Err(error) => {
                    self.error = Some(error.to_string());
                    self.retry_at = now + RECONNECT_MS;
                }
            }
        }
        if self.devices.is_none() {
            match DeviceClient::connect(&self.socket, CLIENT_NAME) {
                Ok(devices) => self.devices = Some(devices),
                Err(error) => {
                    self.error.get_or_insert_with(|| error.to_string());
                    self.retry_at = now + RECONNECT_MS;
                }
            }
        }
    }

    fn apply(&mut self, step: &Step, now: u64) {
        self.connect(now);
        if let Some(mut leds) = self.leds.take() {
            let sent = if step.spot_look && now >= self.native_retry_at { self.apply_spots(&mut leds, step, now) } else { self.apply_plain(&mut leds, step, now) };
            match sent {
                Ok(()) => self.leds = Some(leds),
                Err(error) => {
                    self.error = Some(error.to_string());
                    self.applied = None;
                    self.retry_at = now + RECONNECT_MS;
                }
            }
        }
        self.write_brightness(step.brightness, now);
    }

    /// The tags' look with its spots as inputs. A refusal means an older lemnosd: the frame
    /// shows instead until the retry time.
    fn apply_spots(&mut self, leds: &mut LedClient, step: &Step, now: u64) -> Result<(), ClientError> {
        let Some(look) = step.look else { return Ok(()) };
        let spots = native_spots(&step.spots);
        if self.applied.as_ref().is_some_and(|applied| applied.look == look) {
            return match leds.update_inputs(RING_DEVICE, None, Some(&spots)) {
                Err(ClientError::Rejected(_)) => self.refuse_spots(leds, step, now),
                other => other,
            };
        }
        let mut request = LedRequest::new(LedShow::LookSpots { name: look.to_string(), progress: None, spots: SpotList::new(&spots) });
        request.fade_ms = Some(step.fade_ms);
        match leds.send_checked(request) {
            Ok(()) => {
                self.applied = Some(Applied { look, progress: None, sent_at: now });
                Ok(())
            }
            Err(ClientError::Rejected(_)) => self.refuse_spots(leds, step, now),
            Err(other) => Err(other),
        }
    }

    fn refuse_spots(&mut self, leds: &mut LedClient, step: &Step, now: u64) -> Result<(), ClientError> {
        self.native_retry_at = now + NATIVE_RETRY_MS;
        self.applied = None;
        self.apply_plain(leds, step, now)
    }

    /// The look without spots, or the frame.
    fn apply_plain(&mut self, leds: &mut LedClient, step: &Step, now: u64) -> Result<(), ClientError> {
        match (step.leds, step.look) {
            (Some(pixels), _) => {
                if now.saturating_sub(self.last_frame_at) < FRAME_MS {
                    return Ok(());
                }
                self.last_frame_at = now;
                self.applied = None;
                leds.frame(&pixels)
            }
            (None, Some(look)) => {
                let progress = step.progress.map(|value| (value * 1000.0).round() as u16);
                let changed = match &self.applied {
                    None => true,
                    Some(applied) => applied.look != look || (applied.progress != progress && now.saturating_sub(applied.sent_at) >= PROGRESS_MS),
                };
                if !changed {
                    return Ok(());
                }
                let look_changed = self.applied.as_ref().is_none_or(|applied| applied.look != look);
                let mut request = LedRequest::new(LedShow::Look { name: look.to_string(), progress });
                request.fade_ms = Some(if look_changed { step.fade_ms } else { 0 });
                self.applied = Some(Applied { look, progress, sent_at: now });
                leds.send(request)
            }
            (None, None) => Ok(()),
        }
    }

    fn write_brightness(&mut self, brightness: f64, now: u64) {
        let moved = self.brightness.is_none_or(|applied| (applied - brightness).abs() >= BRIGHTNESS_STEP);
        if !moved {
            return;
        }
        let Some(devices) = self.devices.as_mut() else { return };
        match devices.light_brightness(RING_DEVICE, brightness, false) {
            Ok(_) => self.brightness = Some(brightness),
            Err(error) => {
                self.error = Some(error.to_string());
                self.devices = None;
                self.brightness = None;
                self.retry_at = self.retry_at.max(now + RECONNECT_MS);
            }
        }
    }

    /// Ends this client's looks, so lemnosd shows the layers below (the peripherals' status).
    fn release(&mut self) {
        if let Some(leds) = self.leds.as_mut() {
            let _ = leds.clear();
        }
        self.applied = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::leds::states::State;

    #[test]
    fn spots_become_lemnos_spots_in_thousandths_of_a_turn() {
        let spots = [SpotView { position: 4.0, level: 0.5 }, SpotView { position: 15.9, level: 1.0 }];
        let native = native_spots(&spots);
        assert_eq!(native.len(), 2);
        assert_eq!((native[0].at, native[0].level), (994, 1000), "the brightest first; 15.9 of 16 is 994 thousandths");
        assert_eq!((native[1].at, native[1].level), (250, 500), "4 of 16 is a quarter turn");
    }

    #[test]
    fn at_most_eight_spots_are_sent_and_the_brightest_win() {
        let spots: Vec<SpotView> = (0..12).map(|i| SpotView { position: f64::from(i), level: f64::from(i) / 12.0 }).collect();
        let native = native_spots(&spots);
        assert_eq!(native.len(), MAX_NATIVE_SPOTS);
        assert_eq!(native[0].level, 917, "the level 11/12 spot is first");
        // Levels are 0, 83, 167, ... 917: the eight brightest start at 333 (4/12).
        assert!(native.iter().all(|spot| spot.level >= 333), "the four dimmest are dropped");
    }

    #[test]
    fn the_event_carries_the_frame_as_hex_when_there_is_one() {
        let step = Step {
            state: State::Locate,
            look: None,
            progress: None,
            fade_ms: 0,
            leds: Some([0x0028_c8ff; 16]),
            spot_look: false,
            brightness: 0.5,
            spots: Vec::new(),
            sheen: None,
            tags: 0,
            locked: false,
            recording: false,
        };
        let data = event_data(&step);
        assert_eq!(data["state"], "locate");
        assert_eq!(data["leds"][0], "28c8ff");
        assert_eq!(data["leds"].as_array().map(Vec::len), Some(16));
        let looked = Step { state: State::Searching, look: Some("helios.searching"), leds: None, ..step };
        assert!(event_data(&looked)["leds"].is_null());
    }
}
