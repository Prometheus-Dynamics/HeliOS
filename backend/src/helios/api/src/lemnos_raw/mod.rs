//! Raw GPIO, PWM, I2C and SPI access for the HTTP API (`/v1/peripherals/io`), straight to lemnosd
//! (Lemnos `docs/system-service.md`, "Raw bus and line access"). lemnosd owns the hardware and
//! decides every claim by its board definition (`raw_clients`); helios-api is one more client of
//! it, so a raw action takes no Orion hop and no other HeliOS process.
//!
//! One thread owns the connection: a reconnecting lemnos `DeviceClient`, opened on the first raw
//! action (a board nobody pokes at keeps no connection), with no subscriptions. It keeps HeliOS's
//! claims as leases (`actions`): renewed by use, released when they run out, claimed again after a
//! lemnosd restart, and released when helios-api stops (lemnosd ends them with the connection
//! anyway). Changes to the claims are `gpio` events.

pub mod actions;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use lemnos_ipc::{ClientError, ClientEvent, ClientOptions, DeviceClient, Event, Refusal, Update};
use tokio::sync::oneshot;
use tracing::{debug, info, warn};

pub use actions::{RAW_ACTIONS, RawAction, raw_access_allowed};
use actions::{ClaimSpec, Claims, EdgeView, Held, MAX_CLAIMS};

use crate::{events::EventHub, routes::resources::ClaimDto};

/// The client name lemnosd knows HeliOS by: the board's `raw_clients` must admit it.
pub const CLIENT_NAME: &str = "helios";
/// How long the thread waits for lemnosd's events before it looks at commands and timers.
const TICK: Duration = Duration::from_millis(50);
/// How long the first connection waits for lemnosd before it keeps trying in the background.
const STARTUP_WAIT: Duration = Duration::from_secs(2);
const NOT_CONNECTED: &str = "lemnosd is not connected";

/// Where lemnosd is, and its board definition (for the raw-access policy).
#[derive(Debug, Clone)]
pub struct RawOptions {
    pub socket: PathBuf,
    pub board_file: Option<PathBuf>,
    pub client: String,
}

/// Raw access now: whether the board lets HeliOS in, whether lemnosd is connected, and the claims.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawStatus {
    pub allowed: bool,
    pub connected: bool,
    pub claims: Vec<ClaimDto>,
}

type Reply = oneshot::Sender<Result<Option<serde_json::Value>, String>>;
/// The worker thread and the queue of actions to it.
type WorkerHandle = (Sender<(RawAction, Reply)>, JoinHandle<()>);

/// helios-api's raw-access connection to lemnosd.
pub struct RawAccess {
    options: RawOptions,
    events: Option<Arc<EventHub>>,
    status: Arc<Mutex<RawStatus>>,
    allowed: std::sync::OnceLock<bool>,
    worker: Mutex<Option<WorkerHandle>>,
}

impl RawAccess {
    /// `events`: where `gpio` events go (none in tests).
    pub fn new(options: RawOptions, events: Option<Arc<EventHub>>) -> Self {
        Self { options, events, status: Arc::new(Mutex::new(RawStatus::default())), allowed: std::sync::OnceLock::new(), worker: Mutex::new(None) }
    }

    /// Whether the board definition admits HeliOS, read once (it is part of the image; lemnosd
    /// decides each claim by it anyway). A board file that cannot be read admits nobody.
    pub fn allowed(&self) -> bool {
        *self.allowed.get_or_init(|| {
            let Some(board_file) = &self.options.board_file else { return false };
            raw_access_allowed(board_file, &self.options.client).unwrap_or_else(|error| {
                debug!(error = %error, "the lemnosd board definition could not be read; no raw access");
                false
            })
        })
    }

    pub fn status(&self) -> RawStatus {
        RawStatus { allowed: self.allowed(), ..lock(&self.status).clone() }
    }

    /// A raw action; answers what it returns (a claim id, a level, the bytes read, a lease's end).
    pub async fn run(&self, action: RawAction) -> Result<Option<serde_json::Value>, String> {
        if !self.allowed() {
            return Err("the board definition gives HeliOS no raw access (lemnosd's raw_clients)".into());
        }
        let (reply, answer) = oneshot::channel();
        {
            let mut worker = lock(&self.worker);
            if worker.is_none() {
                let (commands, receiver) = mpsc::channel();
                let thread = Worker::new(self.options.clone(), self.status.clone(), self.events.clone());
                let handle = std::thread::Builder::new().name("lemnos-raw".into()).spawn(move || thread.run(receiver)).map_err(|error| format!("the lemnosd connection did not start: {error}"))?;
                *worker = Some((commands, handle));
            }
            let sent = worker.as_ref().is_some_and(|(commands, _)| commands.send((action, reply)).is_ok());
            if !sent {
                return Err("the lemnosd connection has stopped".into());
            }
        }
        answer.await.unwrap_or_else(|_| Err("the lemnosd connection has stopped".into()))
    }

    /// Releases the claims and closes the connection.
    pub fn stop(&self) {
        let worker = lock(&self.worker).take();
        if let Some((commands, handle)) = worker {
            drop(commands);
            let _ = handle.join();
        }
    }
}

impl Drop for RawAccess {
    fn drop(&mut self) {
        self.stop();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ms() -> u64 {
    crate::host::now_ms()
}

fn describe(error: ClientError) -> String {
    match error {
        ClientError::Refused(Refusal::Owned) => "lemnosd refused: a board device owns it (raw access is never given to a board device's line, channel or address)".into(),
        ClientError::Refused(Refusal::Claimed) => "lemnosd refused: another client holds it".into(),
        ClientError::Refused(refusal) => format!("lemnosd refused: {refusal}"),
        other => other.to_string(),
    }
}

fn not_held(id: &str) -> String {
    format!("{id} is not held right now (lemnosd is reconnecting); try again")
}

struct Worker {
    options: RawOptions,
    status: Arc<Mutex<RawStatus>>,
    events: Option<Arc<EventHub>>,
    devices: Option<DeviceClient>,
    claims: Claims,
    published: std::collections::BTreeMap<String, serde_json::Value>,
    changed: bool,
}

impl Worker {
    fn new(options: RawOptions, status: Arc<Mutex<RawStatus>>, events: Option<Arc<EventHub>>) -> Self {
        Self { options, status, events, devices: None, claims: Claims::default(), published: Default::default(), changed: false }
    }

    fn run(mut self, commands: Receiver<(RawAction, Reply)>) {
        loop {
            if self.devices.is_none() {
                self.open();
            }
            for event in self.next_events() {
                self.handle(event);
            }
            let mut next = if self.devices.is_some() {
                commands.try_recv().map_err(|error| error == TryRecvError::Disconnected)
            } else {
                commands.recv_timeout(TICK).map_err(|error| error == RecvTimeoutError::Disconnected)
            };
            loop {
                match next {
                    Ok((action, reply)) => {
                        let kind = action.kind();
                        let result = self.raw(action);
                        if let Err(error) = &result {
                            debug!(action = kind, error = %error, "raw action failed");
                        }
                        // The status shows what the action did before its caller hears back, so
                        // a status read after the reply never shows a released claim.
                        self.flush();
                        let _ = reply.send(result);
                    }
                    Err(true) => return self.shutdown(),
                    Err(false) => break,
                }
                next = commands.try_recv().map_err(|error| error == TryRecvError::Disconnected);
            }
            self.expire_claims();
            self.flush();
        }
    }

    fn open(&mut self) {
        let options = ClientOptions::new(&self.options.socket, self.options.client.clone()).reconnecting();
        let devices = options.clone().wait(STARTUP_WAIT).devices().or_else(|error| {
            warn!(socket = %self.options.socket.display(), error = %error, "lemnosd is not up; connecting in the background");
            options.devices()
        });
        match devices {
            // Its `Connected` (or `Disconnected`) event is queued.
            Ok(devices) => self.devices = Some(devices),
            Err(error) => {
                warn!(error = %error, "no lemnosd client; trying again");
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }

    fn next_events(&mut self) -> Vec<ClientEvent<Update>> {
        let mut events = Vec::new();
        let Some(devices) = self.devices.as_mut() else { return events };
        let mut wait = TICK;
        while events.len() < 256 {
            match devices.next_event_timeout(wait) {
                Ok(Some(event)) => {
                    events.push(event);
                    wait = Duration::ZERO;
                }
                Ok(None) => break,
                Err(error) => {
                    warn!(error = %error, "lemnosd connection closed");
                    self.devices = None;
                    events.push(ClientEvent::Disconnected { error });
                    break;
                }
            }
        }
        events
    }

    fn handle(&mut self, event: ClientEvent<Update>) {
        match event {
            ClientEvent::Connected { reconnects } => {
                info!(reconnects, "raw access connected to lemnosd");
                lock(&self.status).connected = true;
                self.reclaim();
                self.changed = true;
            }
            ClientEvent::Disconnected { error } => {
                warn!(error = %error, "raw access lost lemnosd; claims are taken again when it is back");
                self.claims.disconnected();
                lock(&self.status).connected = false;
                self.changed = true;
            }
            ClientEvent::Data(Update::Event(Event::Edge { handle, rising, timestamp_ns, seq })) => {
                if self.claims.edge(handle, EdgeView { rising, timestamp_ns, seq }) {
                    self.changed = true;
                }
            }
            ClientEvent::Data(Update::Event(Event::Dropped { count })) => warn!(count, "lemnosd dropped events helios-api did not read in time"),
            ClientEvent::Data(_) => {}
        }
    }

    /// Claims do not survive a lemnosd restart: claim every live lease again with its last
    /// settings. A claim lemnosd refuses now ends.
    fn reclaim(&mut self) {
        let Some(devices) = self.devices.as_mut() else { return };
        let mut lost = Vec::new();
        for claim in self.claims.iter_mut().filter(|claim| claim.held == Held::Pending) {
            let held = match &claim.spec {
                ClaimSpec::Line(line) => devices.claim_line_with(line.target.clone(), line.config, line.on_release).map(Held::Line),
                ClaimSpec::Pwm { target, config } => devices.claim_pwm(target.clone()).and_then(|pwm| match config {
                    Some(config) => pwm.configure(devices, *config).map(|()| Held::Pwm(pwm)),
                    None => Ok(Held::Pwm(pwm)),
                }),
            };
            match held {
                Ok(held) => {
                    info!(claim = %claim.id, target = %claim.target(), "raw claim taken again after reconnecting to lemnosd");
                    claim.held = held;
                }
                Err(error) => {
                    warn!(claim = %claim.id, target = %claim.target(), error = %describe(error), "raw claim lost with the lemnosd connection");
                    lost.push(claim.id.clone());
                }
            }
        }
        for id in lost {
            self.claims.remove(&id);
        }
    }

    fn raw(&mut self, action: RawAction) -> Result<Option<serde_json::Value>, String> {
        let (now, now_ms) = (Instant::now(), now_ms());
        let devices = self.devices.as_mut().filter(|devices| devices.is_connected()).ok_or_else(|| NOT_CONNECTED.to_string())?;
        let claims = &mut self.claims;
        let bytes = |bytes: Vec<u8>| serde_json::Value::Array(bytes.into_iter().map(serde_json::Value::from).collect());
        let value = match action {
            RawAction::GpioClaim { claim, ttl } => {
                if claims.len() >= MAX_CLAIMS {
                    return Err(format!("at most {MAX_CLAIMS} raw claims at a time"));
                }
                let line = devices.claim_line_with(claim.target.clone(), claim.config, claim.on_release).map_err(describe)?;
                let id = claims.insert(ClaimSpec::Line(claim), Held::Line(line), ttl, now, now_ms);
                info!(claim = %id, "GPIO line claimed through lemnosd");
                self.changed = true;
                Some(serde_json::Value::String(id))
            }
            RawAction::GpioConfigure { claim, config, ttl } => {
                let claim = claims.renew(&claim, Some("gpio"), ttl, now, now_ms)?;
                let (Held::Line(line), ClaimSpec::Line(spec)) = (claim.held, &mut claim.spec) else { return Err(not_held(&claim.id)) };
                let config = config.apply(spec.config)?;
                line.configure(devices, config).map_err(describe)?;
                spec.config = config;
                self.changed = true;
                None
            }
            RawAction::GpioGet { claim, ttl } => {
                let claim = claims.renew(&claim, Some("gpio"), ttl, now, now_ms)?;
                let Held::Line(line) = claim.held else { return Err(not_held(&claim.id)) };
                Some(serde_json::Value::Bool(line.get(devices).map_err(describe)?))
            }
            RawAction::GpioSet { claim, value, ttl } => {
                let claim = claims.renew(&claim, Some("gpio"), ttl, now, now_ms)?;
                let (Held::Line(line), ClaimSpec::Line(spec)) = (claim.held, &mut claim.spec) else { return Err(not_held(&claim.id)) };
                line.set(devices, value).map_err(describe)?;
                // Kept, so a claim taken again after a lemnosd restart comes back at this level.
                spec.config.initial = value;
                self.changed = true;
                None
            }
            RawAction::GpioRelease { claim } | RawAction::PwmRelease { claim } => {
                let kind = if claim.starts_with("gpio-") { "gpio" } else { "pwm" };
                claims.renew(&claim, Some(kind), None, now, now_ms)?;
                let released = claims.remove(&claim).expect("just renewed");
                self.changed = true;
                match released.held {
                    Held::Line(line) => line.release(devices).map_err(describe)?,
                    Held::Pwm(pwm) => pwm.release(devices).map_err(describe)?,
                    Held::Pending => {}
                }
                info!(claim = %released.id, "raw claim released");
                None
            }
            RawAction::PwmClaim { target, config, ttl } => {
                if claims.len() >= MAX_CLAIMS {
                    return Err(format!("at most {MAX_CLAIMS} raw claims at a time"));
                }
                let pwm = devices.claim_pwm(target.clone()).map_err(describe)?;
                let applied = match config.map(|config| config.apply(Default::default())).transpose() {
                    Ok(applied) => applied,
                    Err(error) => {
                        let _ = pwm.release(devices);
                        return Err(error);
                    }
                };
                if let Some(applied) = applied
                    && let Err(error) = pwm.configure(devices, applied)
                {
                    let _ = pwm.release(devices);
                    return Err(describe(error));
                }
                let id = claims.insert(ClaimSpec::Pwm { target, config: applied }, Held::Pwm(pwm), ttl, now, now_ms);
                info!(claim = %id, "PWM channel claimed through lemnosd");
                self.changed = true;
                Some(serde_json::Value::String(id))
            }
            RawAction::PwmConfigure { claim, config, ttl } => {
                let claim = claims.renew(&claim, Some("pwm"), ttl, now, now_ms)?;
                let (Held::Pwm(pwm), ClaimSpec::Pwm { config: current, .. }) = (claim.held, &mut claim.spec) else { return Err(not_held(&claim.id)) };
                let config = config.apply(current.unwrap_or_default())?;
                pwm.configure(devices, config).map_err(describe)?;
                *current = Some(config);
                self.changed = true;
                None
            }
            RawAction::I2cTransfer { bus, address, ops } => Some(bytes(devices.i2c(bus, address).transfer(devices, ops).map_err(describe)?)),
            RawAction::SpiTransfer { bus, chip_select, transfers } => Some(bytes(devices.spi(bus, chip_select).transfer(devices, transfers).map_err(describe)?)),
            RawAction::Renew { claim, ttl } => {
                let claim = claims.renew(&claim, None, ttl, now, now_ms)?;
                self.changed = true;
                Some(serde_json::Value::from(claim.expires_at_ms))
            }
        };
        Ok(value)
    }

    /// Releases the claims whose lease ran out (their client stopped renewing them).
    fn expire_claims(&mut self) {
        for id in self.claims.expired(Instant::now()) {
            let Some(claim) = self.claims.remove(&id) else { continue };
            self.changed = true;
            let result = match (self.devices.as_mut().filter(|devices| devices.is_connected()), claim.held) {
                (Some(devices), Held::Line(line)) => line.release(devices),
                (Some(devices), Held::Pwm(pwm)) => pwm.release(devices),
                _ => Ok(()),
            };
            match result {
                Ok(()) | Err(ClientError::Closed) => info!(claim = %id, target = %claim.target(), "raw claim ran out; released"),
                Err(error) => warn!(claim = %id, error = %describe(error), "releasing a raw claim that ran out failed"),
            }
        }
    }

    /// Publishes the claims when they changed: the status, and a `gpio` event per claim made,
    /// changed or ended.
    fn flush(&mut self) {
        if !self.changed {
            return;
        }
        self.changed = false;
        let claims: Vec<ClaimDto> = self.claims.iter().map(actions::Claim::dto).collect();
        let now: std::collections::BTreeMap<String, serde_json::Value> = claims.iter().filter_map(|claim| Some((claim.id.clone(), serde_json::to_value(claim).ok()?))).collect();
        lock(&self.status).claims = claims;
        if let Some(events) = &self.events {
            let mut out = Vec::new();
            crate::events::diff_maps("gpio", &self.published, &now, &mut out);
            for (kind, data) in out {
                events.publish(kind, data);
            }
        }
        self.published = now;
    }

    fn shutdown(mut self) {
        // lemnosd ends the claims with the connection anyway; releasing them first is quicker.
        if let Some(devices) = self.devices.as_mut().filter(|devices| devices.is_connected()) {
            for claim in self.claims.iter() {
                let _ = match claim.held {
                    Held::Line(line) => line.release(devices),
                    Held::Pwm(pwm) => pwm.release(devices),
                    Held::Pending => Ok(()),
                };
            }
        }
    }
}
