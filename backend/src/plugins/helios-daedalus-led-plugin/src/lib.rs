mod worker;

use std::time::{Duration, Instant};

use daedalus::macros::node;
use daedalus::runtime::NodeError;
use daedalus::{declare_plugin, export_plugin};
use lib_led_animations::{LED_ANIMATIONS_PATH, LedAnimationEntry, load_led_animations_sync};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LedAnimationName(pub String);

#[derive(Default)]
struct LedNodeState {
    worker: Option<worker::LedWorkerHandle>,
    cache: AnimationCache,
}

#[derive(Clone, Debug)]
struct AnimationCache {
    entries: Vec<LedAnimationEntry>,
    last_loaded: Instant,
}

impl Default for AnimationCache {
    fn default() -> Self {
        Self { entries: Vec::new(), last_loaded: Instant::now() - Duration::from_secs(3600) }
    }
}

impl AnimationCache {
    fn refresh_if_stale(&mut self) {
        if self.entries.is_empty() || self.last_loaded.elapsed() > Duration::from_secs(1) {
            let doc = load_led_animations_sync(LED_ANIMATIONS_PATH);
            self.entries = doc.animations;
            self.last_loaded = Instant::now();
        }
    }

    fn find(&mut self, name: &str) -> Option<LedAnimationEntry> {
        self.refresh_if_stale();
        self.entries.iter().find(|entry| entry.name.eq_ignore_ascii_case(name)).cloned()
    }
}

impl LedNodeState {
    fn ensure_worker(&mut self) -> Result<&worker::LedWorkerHandle, NodeError> {
        if self.worker.is_none() {
            self.worker = Some(worker::spawn()?);
        }
        Ok(self.worker.as_ref().expect("worker initialized"))
    }
}

#[node(id = "play_saved", inputs("animation"), outputs("ok"), state(LedNodeState))]
fn led_play_saved(animation: LedAnimationName, state: &mut LedNodeState) -> Result<bool, NodeError> {
    let entry = state.cache.find(&animation.0).ok_or_else(|| NodeError::InvalidInput(format!("unknown LED animation '{}'", animation.0)))?;
    let worker = state.ensure_worker()?;
    Ok(worker.play(entry, worker::PlayMode::Immediate))
}

#[node(id = "queue_saved", inputs("animation"), outputs("ok"), state(LedNodeState))]
fn led_queue_saved(animation: LedAnimationName, state: &mut LedNodeState) -> Result<bool, NodeError> {
    let entry = state.cache.find(&animation.0).ok_or_else(|| NodeError::InvalidInput(format!("unknown LED animation '{}'", animation.0)))?;
    let worker = state.ensure_worker()?;
    Ok(worker.play(entry, worker::PlayMode::Queue))
}

#[node(id = "stop", outputs("ok"), state(LedNodeState))]
fn led_stop(state: &mut LedNodeState) -> Result<bool, NodeError> {
    let worker = state.ensure_worker()?;
    Ok(worker.stop())
}

declare_plugin!(
    LedPlugin,
    "led",
    [led_play_saved, led_queue_saved, led_stop],
    install = |registry| {
        let doc = load_led_animations_sync(LED_ANIMATIONS_PATH);
        let variants = doc.animations.into_iter().map(|entry| entry.name).collect::<Vec<_>>();
        registry.register_enum::<LedAnimationName>(variants);
    }
);

export_plugin!(LedPlugin);
