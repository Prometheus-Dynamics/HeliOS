mod runtime;
mod scoring;
mod system;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use once_cell::sync::Lazy;
use serde::Serialize;
use tokio::sync::{mpsc, oneshot};
use utoipa::ToSchema;
use uuid::Uuid;

use self::system::{env_flag, env_u64, env_usize};

pub use runtime::{restore_stream, snapshot, spawn_resource_guard_task};

const MAX_RECENT_ACTIONS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResourceGuardStage {
    DecoderDisabled = 1,
    CodecsDisabled = 2,
}

#[derive(Debug, Clone)]
struct DegradedStream {
    alias: Option<String>,
    original_decoder_id: Option<String>,
    original_encoder_id: Option<String>,
    stage: ResourceGuardStage,
    changed_at_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResourceGuardActionKind {
    DisableDecoder,
    DisableAllCodecs,
    StopStream,
    RestoreCodecs,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ResourceGuardAction {
    pub at_ms: u64,
    pub kind: ResourceGuardActionKind,
    pub stream_id: Uuid,
    #[serde(default)]
    pub alias: Option<String>,
    pub score: f64,
    pub reason: String,
    #[serde(default)]
    pub mem_available_kb: Option<u64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ResourceGuardDegradedStream {
    pub stream_id: Uuid,
    #[serde(default)]
    pub alias: Option<String>,
    pub stage: ResourceGuardStage,
    pub changed_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ResourceGuardStatus {
    pub enabled: bool,
    pub poll_ms: u64,
    pub cooldown_ms: u64,
    pub mem_low_kb: u64,
    pub mem_recover_kb: u64,
    #[serde(default)]
    pub last_mem_available_kb: Option<u64>,
    pub pressure_active: bool,
    #[serde(default)]
    pub degraded_streams: Vec<ResourceGuardDegradedStream>,
    #[serde(default)]
    pub last_action: Option<ResourceGuardAction>,
    #[serde(default)]
    pub recent_actions: Vec<ResourceGuardAction>,
}

#[derive(Debug, Clone, Copy)]
struct GuardConfig {
    enabled: bool,
    poll_ms: u64,
    mem_low_kb: u64,
    mem_recover_kb: u64,
    cooldown_ms: u64,
    metrics_top_n: usize,
    metrics_timeout_ms: u64,
    allow_stop_fallback: bool,
    stop_timeout_ms: u64,
}

impl GuardConfig {
    fn from_env() -> Self {
        let mem_low_kb = env_u64("HELIOS_RESOURCE_GUARD_MEM_LOW_KB", 700_000).max(64 * 1024);
        let mem_recover_kb = env_u64("HELIOS_RESOURCE_GUARD_MEM_RECOVER_KB", mem_low_kb.saturating_add(300_000)).max(mem_low_kb.saturating_add(64 * 1024));
        Self {
            enabled: env_flag("HELIOS_RESOURCE_GUARD_ENABLED", true),
            poll_ms: env_u64("HELIOS_RESOURCE_GUARD_POLL_MS", 1500).max(250),
            mem_low_kb,
            mem_recover_kb,
            cooldown_ms: env_u64("HELIOS_RESOURCE_GUARD_COOLDOWN_MS", 5000).max(500),
            metrics_top_n: env_usize("HELIOS_RESOURCE_GUARD_METRICS_TOP_N", 6).clamp(1, 24),
            metrics_timeout_ms: env_u64("HELIOS_RESOURCE_GUARD_METRICS_TIMEOUT_MS", 300).max(50),
            allow_stop_fallback: env_flag("HELIOS_RESOURCE_GUARD_ALLOW_STOP_FALLBACK", false),
            stop_timeout_ms: env_u64("HELIOS_RESOURCE_GUARD_STOP_TIMEOUT_MS", 4000).max(500),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ReliefAction {
    DisableDecoder,
    DisableAllCodecs,
    StopStream,
}

#[derive(Debug, Clone)]
struct ReliefCandidate {
    stream_id: Uuid,
    score: f64,
    action: ReliefAction,
}

enum GuardCommand {
    Restore { stream_id: Uuid, respond_to: oneshot::Sender<Result<ResourceGuardAction, String>> },
}

struct GuardRuntimeState {
    enabled: bool,
    poll_ms: u64,
    cooldown_ms: u64,
    mem_low_kb: u64,
    mem_recover_kb: u64,
    last_mem_available_kb: Option<u64>,
    pressure_active: bool,
    degraded_streams: HashMap<Uuid, ResourceGuardDegradedStream>,
    last_action: Option<ResourceGuardAction>,
    recent_actions: VecDeque<ResourceGuardAction>,
}

impl GuardRuntimeState {
    fn disabled() -> Self {
        Self {
            enabled: false,
            poll_ms: 0,
            cooldown_ms: 0,
            mem_low_kb: 0,
            mem_recover_kb: 0,
            last_mem_available_kb: None,
            pressure_active: false,
            degraded_streams: HashMap::new(),
            last_action: None,
            recent_actions: VecDeque::new(),
        }
    }

    fn from_config(cfg: GuardConfig) -> Self {
        Self {
            enabled: cfg.enabled,
            poll_ms: cfg.poll_ms,
            cooldown_ms: cfg.cooldown_ms,
            mem_low_kb: cfg.mem_low_kb,
            mem_recover_kb: cfg.mem_recover_kb,
            last_mem_available_kb: None,
            pressure_active: false,
            degraded_streams: HashMap::new(),
            last_action: None,
            recent_actions: VecDeque::new(),
        }
    }

    fn update_mem(&mut self, mem_available_kb: u64) {
        self.last_mem_available_kb = Some(mem_available_kb);
        self.pressure_active = self.enabled && mem_available_kb <= self.mem_low_kb;
    }

    fn sync_degraded(&mut self, degraded: &HashMap<Uuid, DegradedStream>) {
        self.degraded_streams = degraded
            .iter()
            .map(|(stream_id, state)| (*stream_id, ResourceGuardDegradedStream { stream_id: *stream_id, alias: state.alias.clone(), stage: state.stage, changed_at_ms: state.changed_at_ms }))
            .collect();
    }

    fn push_action(&mut self, action: ResourceGuardAction) {
        self.last_action = Some(action.clone());
        self.recent_actions.push_front(action);
        while self.recent_actions.len() > MAX_RECENT_ACTIONS {
            self.recent_actions.pop_back();
        }
    }

    fn snapshot(&self) -> ResourceGuardStatus {
        let mut degraded_streams: Vec<_> = self.degraded_streams.values().cloned().collect();
        degraded_streams.sort_by(|a, b| b.changed_at_ms.cmp(&a.changed_at_ms));
        ResourceGuardStatus {
            enabled: self.enabled,
            poll_ms: self.poll_ms,
            cooldown_ms: self.cooldown_ms,
            mem_low_kb: self.mem_low_kb,
            mem_recover_kb: self.mem_recover_kb,
            last_mem_available_kb: self.last_mem_available_kb,
            pressure_active: self.pressure_active,
            degraded_streams,
            last_action: self.last_action.clone(),
            recent_actions: self.recent_actions.iter().cloned().collect(),
        }
    }
}

static STATE: Lazy<Mutex<GuardRuntimeState>> = Lazy::new(|| Mutex::new(GuardRuntimeState::disabled()));
static COMMAND_TX: Lazy<Mutex<Option<mpsc::UnboundedSender<GuardCommand>>>> = Lazy::new(|| Mutex::new(None));
