//! Timed fan overrides on Lemnos's fan devices (`lemnos.device`, `lemnos.class=fan`, Lemnos
//! `docs/orion.md`): `fan.override` sets the fan's `duty` control through Lemnos's Orion bridge for a
//! limited time, then `release` hands the fan back to the kernel's thermal governor; `fan.release`
//! ends it at once. The bridge has no timed write, so the timer is kept here, and kept on disk, so a
//! restart of helios-api still ends an override on time (or at once, when its time has passed).

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use orion::control_plane::{ActionRequest, ActionResult, ActionState, TypedConfigValue};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{
    error::{ApiError, ApiResult},
    host::now_ms,
    orion::ActionCall,
};

pub const OVERRIDE: &str = "fan.override";
pub const RELEASE: &str = "fan.release";
/// The Lemnos class of a fan device.
pub const FAN_CLASS: &str = "fan";
/// The fan control an override writes, 0.0-1.0.
pub const DUTY_CONTROL: &str = "duty";
pub const DEFAULT_OVERRIDE_MS: u64 = 60_000;
pub const MIN_OVERRIDE_MS: u64 = 1_000;
pub const MAX_OVERRIDE_MS: u64 = 600_000;
/// How long a bridge action may take.
const ACTION_WAIT: Duration = Duration::from_secs(10);
const ACTION_DEADLINE_MS: u64 = 10_000;

/// A validated `fan.override`: `pwm` (0-255) or `duty` (0.0-1.0), exactly one, and
/// `duration_ms` (default 60 s, 1 s to 10 min).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverrideRequest {
    pub duty: f64,
    pub duration_ms: u64,
}

impl OverrideRequest {
    pub fn from_args(arg: &serde_json::Map<String, serde_json::Value>) -> Result<Self, String> {
        if let Some(unknown) = arg.keys().find(|key| !["pwm", "duty", "duration_ms"].contains(&key.as_str())) {
            return Err(format!("unknown argument arg.{unknown}; fan.override takes pwm or duty, and duration_ms"));
        }
        let pwm = match arg.get("pwm") {
            None => None,
            Some(value) => Some(value.as_u64().filter(|pwm| *pwm <= 255).ok_or("arg.pwm must be between 0 and 255")?),
        };
        let duty = match arg.get("duty") {
            None => None,
            Some(value) => Some(value.as_f64().filter(|duty| (0.0..=1.0).contains(duty)).ok_or("arg.duty must be between 0.0 and 1.0")?),
        };
        let duty = match (pwm, duty) {
            (Some(pwm), None) => pwm as f64 / 255.0,
            (None, Some(duty)) => duty,
            (Some(_), Some(_)) => return Err("give arg.pwm (0-255) or arg.duty (0.0-1.0), not both".into()),
            (None, None) => return Err("arg.pwm (0-255) or arg.duty (0.0-1.0) is required".into()),
        };
        let duration_ms = match arg.get("duration_ms") {
            None => DEFAULT_OVERRIDE_MS,
            Some(value) => {
                value.as_u64().filter(|ms| (MIN_OVERRIDE_MS..=MAX_OVERRIDE_MS).contains(ms)).ok_or_else(|| format!("arg.duration_ms must be between {MIN_OVERRIDE_MS} and {MAX_OVERRIDE_MS}"))?
            }
        };
        Ok(Self { duty, duration_ms })
    }
}

/// A running override of one fan resource.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Active {
    /// The duty the fan took.
    pub duty: f64,
    /// When it ends, on the wall clock.
    pub until_ms: u64,
}

#[derive(Debug, Default)]
struct Running {
    active: BTreeMap<String, (Active, u64)>,
    generation: u64,
}

/// Told when an override starts (`Some`) or ends (`None`) on a fan resource.
pub type OnChange = Box<dyn Fn(&str, Option<Active>) + Send + Sync>;

/// The running overrides, by fan resource id, kept in `path`.
pub struct FanOverrides {
    path: PathBuf,
    running: Mutex<Running>,
    on_change: std::sync::OnceLock<OnChange>,
}

impl std::fmt::Debug for FanOverrides {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FanOverrides").field("path", &self.path).field("running", &self.running).finish_non_exhaustive()
    }
}

/// An override's state as `GET /v1/peripherals` shows it on the fan, as a `peripheral` event
/// (`{id, values, expired}`), so a client sees an override start and end without reading again.
pub fn peripheral_event(resource: &str, active: Option<Active>) -> serde_json::Value {
    match active {
        Some(active) => serde_json::json!({ "id": resource, "values": { "fan.override.active": true, "fan.override.duty": active.duty, "fan.override.until_ms": active.until_ms }, "expired": [] }),
        None => serde_json::json!({ "id": resource, "values": { "fan.override.active": false }, "expired": ["fan.override.duty", "fan.override.until_ms"] }),
    }
}

impl FanOverrides {
    /// The overrides `path` holds from before a restart (none when it is missing or unreadable).
    pub fn load(path: PathBuf) -> Self {
        let active: BTreeMap<String, Active> = std::fs::read(&path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        let mut running = Running::default();
        for (resource, override_) in active {
            running.generation += 1;
            running.active.insert(resource, (override_, running.generation));
        }
        Self { path, running: Mutex::new(running), on_change: std::sync::OnceLock::new() }
    }

    /// Where overrides that start or end are told (once; later calls are ignored).
    pub fn on_change(&self, on_change: OnChange) {
        let _ = self.on_change.set(on_change);
    }

    fn changed(&self, resource: &str, active: Option<Active>) {
        if let Some(on_change) = self.on_change.get() {
            on_change(resource, active);
        }
    }

    /// The override running on `resource`.
    pub fn active(&self, resource: &str) -> Option<Active> {
        self.lock().active.get(resource).map(|(active, _)| *active).filter(|active| active.until_ms > now_ms())
    }

    /// Ends the overrides whose time passed while helios-api was down, and times the others again.
    pub fn resume<C: ActionCall + Clone + Send + 'static>(self: &Arc<Self>, caller: C) {
        let entries: Vec<(String, Active, u64)> = self.lock().active.iter().map(|(resource, (active, generation))| (resource.clone(), *active, *generation)).collect();
        for (resource, active, generation) in entries {
            self.spawn_expiry(caller.clone(), resource, active.until_ms, generation);
        }
    }

    /// `fan.override`: sets the duty now, and releases the fan after `request.duration_ms`.
    pub async fn start<C: ActionCall + Clone + Send + 'static>(self: &Arc<Self>, caller: C, resource: &str, request: OverrideRequest) -> ApiResult<ActionResult> {
        let args = BTreeMap::from([("control".to_string(), TypedConfigValue::String(DUTY_CONTROL.into())), ("value".to_string(), TypedConfigValue::F64(request.duty))]);
        let result = caller.call_action(bridge_action(resource, "set", args), ACTION_WAIT).await?;
        if result.state == ActionState::Succeeded {
            let duty = match result.output.get("applied") {
                Some(TypedConfigValue::F64(applied)) => *applied,
                _ => request.duty,
            };
            let until_ms = now_ms().saturating_add(request.duration_ms);
            let generation = {
                let mut running = self.lock();
                running.generation += 1;
                let generation = running.generation;
                running.active.insert(resource.to_string(), (Active { duty, until_ms }, generation));
                generation
            };
            self.save();
            self.changed(resource, Some(Active { duty, until_ms }));
            self.spawn_expiry(caller, resource.to_string(), until_ms, generation);
        }
        Ok(result)
    }

    /// `fan.release`: the fan goes back to the governor now.
    pub async fn release<C: ActionCall>(&self, caller: &C, resource: &str) -> ApiResult<ActionResult> {
        let result = caller.call_action(bridge_action(resource, "release", BTreeMap::new()), ACTION_WAIT).await?;
        if result.state == ActionState::Succeeded && self.lock().active.remove(resource).is_some() {
            self.save();
            self.changed(resource, None);
        }
        Ok(result)
    }

    fn spawn_expiry<C: ActionCall + Send + 'static>(self: &Arc<Self>, caller: C, resource: String, until_ms: u64, generation: u64) {
        let overrides = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(until_ms.saturating_sub(now_ms()))).await;
            // A later override (or a release) replaced this one: it is not this timer's to end.
            if overrides.lock().active.get(&resource).is_none_or(|(_, current)| *current != generation) {
                return;
            }
            match overrides.release(&caller, &resource).await {
                Ok(result) if result.state == ActionState::Succeeded => {}
                Ok(result) => warn!(resource = %resource, state = ?result.state, "the fan override ended but the fan was not released"),
                Err(error) => warn!(resource = %resource, error = %error.message, "the fan override ended but the fan was not released"),
            }
        });
    }

    fn save(&self) {
        let active: BTreeMap<String, Active> = self.lock().active.iter().map(|(resource, (active, _))| (resource.clone(), *active)).collect();
        let written = serde_json::to_vec(&active).map_err(std::io::Error::other).and_then(|bytes| {
            if let Some(dir) = self.path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let temp = self.path.with_extension("json.tmp");
            std::fs::write(&temp, bytes)?;
            std::fs::rename(&temp, &self.path)
        });
        if let Err(error) = written {
            warn!(path = %self.path.display(), error = %error, "the fan overrides were not saved");
        }
    }

    fn lock(&self) -> MutexGuard<'_, Running> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// An action on a Lemnos bridge resource.
fn bridge_action(resource: &str, name: &str, args: BTreeMap<String, TypedConfigValue>) -> ActionRequest {
    let mut request = crate::routes::resources::action_request(resource, name, args).with_deadline_ms(ACTION_DEADLINE_MS);
    request.name = name.to_string();
    request
}

/// Whether `kind` is a fan action this module runs.
pub fn is_fan_action(kind: &str) -> bool {
    kind == OVERRIDE || kind == RELEASE
}

/// A fan action on `resource` from the HTTP API (`kind`, `arg`).
pub async fn run<C: ActionCall + Clone + Send + 'static>(
    overrides: &Arc<FanOverrides>,
    caller: C,
    resource: &str,
    kind: &str,
    arg: &serde_json::Map<String, serde_json::Value>,
) -> ApiResult<ActionResult> {
    let mut result = match kind {
        OVERRIDE => overrides.start(caller, resource, OverrideRequest::from_args(arg).map_err(ApiError::bad_request)?).await?,
        RELEASE if arg.is_empty() => overrides.release(&caller, resource).await?,
        RELEASE => return Err(ApiError::bad_request("fan.release takes no arguments")),
        other => return Err(ApiError::bad_request(format!("{other:?} is not a fan action"))),
    };
    result.name = kind.to_string();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default)]
    struct Bridge {
        sent: Arc<Mutex<Vec<ActionRequest>>>,
    }

    impl Bridge {
        fn names(&self) -> Vec<String> {
            self.sent.lock().expect("sent").iter().map(|request| request.name.clone()).collect()
        }
    }

    impl ActionCall for Bridge {
        async fn call_action(&self, request: ActionRequest, _timeout: Duration) -> ApiResult<ActionResult> {
            self.sent.lock().expect("sent").push(request.clone());
            let mut result = ActionResult::new(request.action_id, request.target, request.name, orion::core::NodeId::new("node-local"), ActionState::Succeeded);
            if let Some(TypedConfigValue::F64(value)) = request.args.get("value") {
                result = result.with_output("applied", TypedConfigValue::F64(*value));
            }
            Ok(result)
        }
    }

    fn args(json: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        json.as_object().cloned().expect("object")
    }

    #[test]
    fn an_override_takes_pwm_or_duty_and_a_bounded_time() {
        assert_eq!(OverrideRequest::from_args(&args(serde_json::json!({"pwm": 255}))), Ok(OverrideRequest { duty: 1.0, duration_ms: DEFAULT_OVERRIDE_MS }));
        assert_eq!(OverrideRequest::from_args(&args(serde_json::json!({"duty": 0.5, "duration_ms": 1000}))), Ok(OverrideRequest { duty: 0.5, duration_ms: 1000 }));
        for bad in [
            serde_json::json!({}),
            serde_json::json!({"pwm": 256}),
            serde_json::json!({"duty": 1.5}),
            serde_json::json!({"pwm": 1, "duty": 0.1}),
            serde_json::json!({"duty": 0.5, "duration_ms": 999}),
            serde_json::json!({"duty": 0.5, "speed": 1}),
        ] {
            assert!(OverrideRequest::from_args(&args(bad.clone())).is_err(), "{bad}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn an_override_sets_the_duty_then_releases_the_fan_on_time() {
        let dir = tempfile::tempdir().expect("dir");
        let overrides = Arc::new(FanOverrides::load(dir.path().join("fan-overrides.json")));
        let bridge = Bridge::default();
        let told = Arc::new(Mutex::new(Vec::new()));
        let sink = told.clone();
        overrides.on_change(Box::new(move |resource, active| sink.lock().expect("told").push(peripheral_event(resource, active))));
        let result = run(&overrides, bridge.clone(), "lemnos.raze.fan", OVERRIDE, &args(serde_json::json!({"duty": 0.6, "duration_ms": 1000}))).await.expect("override");
        assert_eq!(result.name, OVERRIDE);
        assert_eq!(bridge.names(), ["set"]);
        assert_eq!(overrides.active("lemnos.raze.fan").map(|active| active.duty), Some(0.6));
        // On disk, for a restart.
        assert!(FanOverrides::load(dir.path().join("fan-overrides.json")).lock().active.contains_key("lemnos.raze.fan"));
        tokio::time::sleep(Duration::from_millis(1500)).await;
        tokio::task::yield_now().await;
        assert_eq!(bridge.names(), ["set", "release"]);
        assert!(FanOverrides::load(dir.path().join("fan-overrides.json")).lock().active.is_empty());
        // Clients are told the override started and ended.
        let told = told.lock().expect("told");
        assert_eq!(told.len(), 2);
        assert_eq!(told[0]["values"]["fan.override.active"], true);
        assert_eq!(told[0]["values"]["fan.override.duty"], 0.6);
        assert_eq!(told[1]["values"]["fan.override.active"], false);
        assert_eq!(told[1]["expired"], serde_json::json!(["fan.override.duty", "fan.override.until_ms"]));
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_override_replaces_the_old_timer() {
        let dir = tempfile::tempdir().expect("dir");
        let overrides = Arc::new(FanOverrides::load(dir.path().join("fan-overrides.json")));
        let bridge = Bridge::default();
        run(&overrides, bridge.clone(), "fan", OVERRIDE, &args(serde_json::json!({"duty": 0.6, "duration_ms": 1000}))).await.expect("first");
        run(&overrides, bridge.clone(), "fan", OVERRIDE, &args(serde_json::json!({"duty": 0.8, "duration_ms": 5000}))).await.expect("second");
        tokio::time::sleep(Duration::from_millis(1500)).await;
        tokio::task::yield_now().await;
        assert_eq!(bridge.names(), ["set", "set"], "the first timer does not end the second override");
        run(&overrides, bridge.clone(), "fan", RELEASE, &serde_json::Map::new()).await.expect("release");
        assert_eq!(bridge.names(), ["set", "set", "release"]);
        assert_eq!(overrides.active("fan"), None);
    }

    #[tokio::test(start_paused = true)]
    async fn an_override_whose_time_passed_during_a_restart_is_released_at_once() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("fan-overrides.json");
        std::fs::write(&path, serde_json::to_vec(&BTreeMap::from([("fan".to_string(), Active { duty: 1.0, until_ms: now_ms() - 1 })])).expect("json")).expect("write");
        let overrides = Arc::new(FanOverrides::load(path));
        let bridge = Bridge::default();
        overrides.resume(bridge.clone());
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        assert_eq!(bridge.names(), ["release"]);
    }
}
