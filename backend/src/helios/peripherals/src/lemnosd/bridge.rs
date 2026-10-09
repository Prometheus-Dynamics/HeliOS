//! The connection to lemnosd: one thread owns a reconnecting `DeviceClient` and `LedClient`
//! (both blocking, like Styx's clients), keeps [`LemnosdState`] current for the resource probe,
//! runs fan overrides, control writes and raw GPIO/PWM/I2C/SPI actions, and holds HeliOS's
//! status on the status light.
//!
//! - Start: `ClientOptions::wait` gives lemnosd a moment to come up (the unit starts after it);
//!   if it is still not there, the client connects in the background (`reconnecting`), so
//!   helios-peripherals never waits on lemnosd.
//! - Connected (first time or after a lemnosd restart): list the devices, subscribe to every
//!   device with channels, re-send the LED status, and claim the live raw claims again.
//! - Disconnected: the devices go missing and readings are dropped. lemnosd has undone HeliOS's
//!   writes and ended its claims with the connection (Lemnos `docs/system-service.md`), so fan
//!   overrides are over and raw claims wait to be claimed again; nothing is kept on disk.
//! - The runtime is told about changes through `notify`: at once for connection, device-list,
//!   status, override and claim changes, and at most once per reading interval for readings and
//!   GPIO edges.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use lemnos_ipc::{ClientError, ClientEvent, ClientOptions, DeviceClient, Event, LedClient, LedStatus, Reading, Refusal, Update};
use tokio::sync::oneshot;
use tracing::{debug, info, warn};

use crate::lemnosd::fan::{FAN_DUTY_CONTROL, FanOverrideRequest, FanOverrides};
use crate::lemnosd::raw::{ClaimSpec, Claims, EdgeView, Held, MAX_CLAIMS, RawAction};
use crate::lemnosd::resources::{DeviceReading, LemnosdProbe, LemnosdState, OverrideView};
use crate::model::ObservedValue;

/// How long the thread waits for lemnosd's events before it looks at commands and timers.
const TICK: Duration = Duration::from_millis(50);
/// How long to try again when even a background-connecting client could not be made.
const OPEN_RETRY: Duration = Duration::from_secs(1);
/// Events handled per pass before commands get a turn.
const MAX_EVENTS_PER_PASS: usize = 256;

#[derive(Debug, Clone)]
pub struct LemnosdOptions {
    pub socket: PathBuf,
    /// The client name; the fan's board `writers` must list it.
    pub client: String,
    /// The subscription period, and the most often readings and edges are republished.
    pub reading_interval: Duration,
    /// How long the first connection waits for lemnosd (`ClientOptions::wait`) before the client
    /// keeps trying in the background.
    pub startup_wait: Duration,
}

/// Called from the bridge thread when [`LemnosdState`] changed.
pub type Notify = Box<dyn Fn() + Send + 'static>;

type Reply<T> = oneshot::Sender<Result<T, String>>;

enum Command {
    Status(LedStatus),
    FanOverride {
        device: String,
        request: FanOverrideRequest,
        reply: Reply<f64>,
    },
    FanRelease {
        device: String,
        reply: Reply<()>,
    },
    Set {
        device: String,
        control: String,
        value: f64,
        reply: Reply<f64>,
    },
    Raw {
        action: RawAction,
        reply: Reply<Option<ObservedValue>>,
    },
    /// Tests: the thread ends at once, dropping its connections without any cleanup, as a
    /// `kill -9` of helios-peripherals would.
    #[cfg(test)]
    Abandon,
}

pub struct LemnosdBridge {
    options: LemnosdOptions,
    state: Arc<Mutex<LemnosdState>>,
    commands: Mutex<Option<Sender<Command>>>,
    receiver: Mutex<Option<Receiver<Command>>>,
    started: AtomicBool,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl LemnosdBridge {
    pub fn new(options: LemnosdOptions) -> Self {
        let (commands, receiver) = mpsc::channel();
        let state = LemnosdState { socket: options.socket.clone(), ..LemnosdState::default() };
        Self { options, state: Arc::new(Mutex::new(state)), commands: Mutex::new(Some(commands)), receiver: Mutex::new(Some(receiver)), started: AtomicBool::new(false), thread: Mutex::new(None) }
    }

    pub fn options(&self) -> &LemnosdOptions {
        &self.options
    }

    /// The inventory probe for lemnosd's devices.
    pub fn probe(&self) -> LemnosdProbe {
        LemnosdProbe::new(Arc::clone(&self.state))
    }

    pub fn state(&self) -> LemnosdState {
        lock(&self.state).clone()
    }

    /// Starts the thread (once). Commands sent before are handled once it runs.
    pub fn start(&self, notify: Notify) -> std::io::Result<()> {
        let Some(receiver) = lock(&self.receiver).take() else { return Ok(()) };
        let worker = Worker::new(self.options.clone(), Arc::clone(&self.state), notify);
        let handle = std::thread::Builder::new().name("lemnosd".into()).spawn(move || worker.run(receiver))?;
        *lock(&self.thread) = Some(handle);
        self.started.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// HeliOS's status on the status light (re-sent after every reconnection).
    pub fn set_status(&self, status: LedStatus) {
        self.send(Command::Status(status));
    }

    /// Sets the fan's duty for the request's duration; answers the duty lemnosd applied.
    pub async fn fan_override(&self, device: &str, request: FanOverrideRequest) -> Result<f64, String> {
        let (reply, answer) = oneshot::channel();
        self.request(Command::FanOverride { device: device.into(), request, reply }, answer).await
    }

    /// Ends HeliOS's override: the fan goes back to the kernel's governor.
    pub async fn fan_release(&self, device: &str) -> Result<(), String> {
        let (reply, answer) = oneshot::channel();
        self.request(Command::FanRelease { device: device.into(), reply }, answer).await
    }

    /// A control write under the device's write policy; answers the value applied.
    pub async fn set_control(&self, device: &str, control: &str, value: f64) -> Result<f64, String> {
        let (reply, answer) = oneshot::channel();
        self.request(Command::Set { device: device.into(), control: control.into(), value, reply }, answer).await
    }

    /// A raw GPIO, PWM, I2C or SPI action; answers what it returns (a claim id, a level, the
    /// bytes read, a lease's end).
    pub async fn raw(&self, action: RawAction) -> Result<Option<ObservedValue>, String> {
        let (reply, answer) = oneshot::channel();
        self.request(Command::Raw { action, reply }, answer).await
    }

    /// Tests: ends the thread without releasing anything (a crash).
    #[cfg(test)]
    pub fn abandon(&self) {
        self.send(Command::Abandon);
        self.stop();
    }

    /// Stops the thread: running overrides and raw claims are released first.
    pub fn stop(&self) {
        lock(&self.commands).take();
        if let Some(handle) = lock(&self.thread).take() {
            let _ = handle.join();
        }
    }

    async fn request<T>(&self, command: Command, answer: oneshot::Receiver<Result<T, String>>) -> Result<T, String> {
        if !self.started.load(Ordering::SeqCst) {
            return Err("the lemnosd connection is not running".into());
        }
        if !self.send(command) {
            return Err("the lemnosd connection has stopped".into());
        }
        answer.await.unwrap_or_else(|_| Err("the lemnosd connection has stopped".into()))
    }

    fn send(&self, command: Command) -> bool {
        lock(&self.commands).as_ref().is_some_and(|commands| commands.send(command).is_ok())
    }
}

impl Drop for LemnosdBridge {
    fn drop(&mut self) {
        self.stop();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

fn describe(error: ClientError) -> String {
    match error {
        ClientError::Refused(Refusal::Owned) => "lemnosd refused: a board device owns it (raw access is never given to a board device's line, channel or address)".into(),
        ClientError::Refused(Refusal::Claimed) => "lemnosd refused: another client holds it".into(),
        ClientError::Refused(refusal) => format!("lemnosd refused: {refusal}"),
        other => other.to_string(),
    }
}

const NOT_CONNECTED: &str = "lemnosd is not connected";

struct Worker {
    options: LemnosdOptions,
    state: Arc<Mutex<LemnosdState>>,
    notify: Notify,
    devices: Option<DeviceClient>,
    next_open: Instant,
    leds: Option<LedClient>,
    status: Option<LedStatus>,
    fans: FanOverrides,
    claims: Claims,
    changed: bool,
    readings_changed: bool,
    last_notify: Instant,
}

impl Worker {
    fn new(options: LemnosdOptions, state: Arc<Mutex<LemnosdState>>, notify: Notify) -> Self {
        let now = Instant::now();
        Self {
            options,
            state,
            notify,
            devices: None,
            next_open: now,
            leds: None,
            status: None,
            fans: FanOverrides::default(),
            claims: Claims::default(),
            changed: false,
            readings_changed: false,
            last_notify: now,
        }
    }

    fn run(mut self, commands: Receiver<Command>) {
        loop {
            if self.devices.is_none() {
                self.open();
            }
            for event in self.next_events() {
                self.handle(event);
            }
            let first = if self.devices.is_some() {
                commands.try_recv().map_err(|error| error == TryRecvError::Disconnected)
            } else {
                commands.recv_timeout(TICK).map_err(|error| error == RecvTimeoutError::Disconnected)
            };
            let mut next = first;
            loop {
                match next {
                    #[cfg(test)]
                    Ok(Command::Abandon) => return,
                    Ok(command) => self.command(command),
                    Err(true) => return self.shutdown(),
                    Err(false) => break,
                }
                next = commands.try_recv().map_err(|error| error == TryRecvError::Disconnected);
            }
            self.expire_overrides();
            self.expire_claims();
            self.flush();
        }
    }

    /// The device client: wait a moment for lemnosd, else connect in the background.
    fn open(&mut self) {
        if Instant::now() < self.next_open {
            return;
        }
        let options = ClientOptions::new(&self.options.socket, self.options.client.clone()).reconnecting();
        let devices = options.clone().wait(self.options.startup_wait).devices().or_else(|error| {
            warn!(socket = %self.options.socket.display(), error = %error, wait_ms = self.options.startup_wait.as_millis() as u64, "lemnosd is not up yet; connecting in the background");
            options.devices()
        });
        match devices {
            // Its `Connected` (or `Disconnected`) event is queued: the device list follows there.
            Ok(devices) => self.devices = Some(devices),
            Err(error) => {
                warn!(error = %error, "no lemnosd client; trying again");
                self.next_open = Instant::now() + OPEN_RETRY;
            }
        }
    }

    fn connected(&mut self) -> Result<&mut DeviceClient, String> {
        self.devices.as_mut().filter(|devices| devices.is_connected()).ok_or_else(|| NOT_CONNECTED.to_string())
    }

    fn next_events(&mut self) -> Vec<ClientEvent<Update>> {
        let mut events = Vec::new();
        let Some(devices) = self.devices.as_mut() else { return events };
        let mut wait = TICK;
        while events.len() < MAX_EVENTS_PER_PASS {
            match devices.next_event_timeout(wait) {
                Ok(Some(event)) => {
                    events.push(event);
                    wait = Duration::ZERO;
                }
                Ok(None) => break,
                Err(error) => {
                    // Only a client that does not reconnect gives up; start over.
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
            ClientEvent::Connected { reconnects } => self.on_connected(reconnects),
            ClientEvent::Disconnected { error } => self.on_disconnected(&error),
            ClientEvent::Data(Update::Reading(reading)) => self.on_reading(reading),
            ClientEvent::Data(Update::Event(event)) => self.on_event(event),
        }
    }

    fn on_connected(&mut self, reconnects: u64) {
        let Some(devices) = self.devices.as_mut() else { return };
        let list = devices.list().unwrap_or_else(|error| {
            warn!(error = %error, "listing lemnosd's devices failed; using the list from the connection");
            devices.devices().cloned().collect()
        });
        let period_ms = self.options.reading_interval.as_millis().clamp(1, u128::from(u32::MAX)) as u32;
        for device in list.iter().filter(|device| !device.channels.is_empty()) {
            if let Err(error) = devices.subscribe(&device.id, period_ms) {
                warn!(device = %device.id, error = %error, "subscribing to a lemnosd device failed");
            }
        }
        let board = devices.board().to_string();
        info!(board = %board, devices = list.len(), reconnects, "connected to lemnosd");
        {
            let mut state = lock(&self.state);
            state.connected = true;
            state.board = board;
            state.readings.retain(|device, _| list.iter().any(|known| &known.id == device));
            state.devices = list;
        }
        self.reclaim();
        if self.leds.is_none() {
            // LED clients get no events unless they ask: HeliOS only sends its status.
            match ClientOptions::new(&self.options.socket, self.options.client.clone()).reconnecting().leds() {
                Ok(leds) => self.leds = Some(leds),
                Err(error) => warn!(error = %error, "lemnosd LED client failed to connect"),
            }
        }
        self.send_status();
        self.changed = true;
    }

    /// Claims do not survive a lemnosd restart (nor a lost connection): claim every live lease
    /// again with its last settings. A claim lemnosd refuses now ends.
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

    fn on_disconnected(&mut self, error: &ClientError) {
        warn!(error = %error, "lost the lemnosd connection; its devices are offline until it is back");
        if self.fans.active().next().is_some() {
            info!("the fan override ended with the lemnosd connection; lemnosd handed the fan back");
        }
        self.fans.disconnected();
        self.claims.disconnected();
        let mut state = lock(&self.state);
        state.connected = false;
        state.readings.clear();
        self.changed = true;
    }

    fn on_reading(&mut self, reading: Reading) {
        let values = reading.values().map(|(channel, value)| (channel.to_string(), value)).collect();
        let mut state = lock(&self.state);
        if let Some(device) = state.devices.iter_mut().find(|device| device.id == reading.device)
            && device.status != reading.status
        {
            device.status = reading.status;
            self.changed = true;
        }
        state.readings.insert(reading.device, DeviceReading { observed_at_ms: now_ms(), timestamp_us: reading.timestamp_us, values });
        self.readings_changed = true;
    }

    fn on_event(&mut self, event: Event) {
        match event {
            Event::Status { device, status, error, .. } => {
                if let Some(error) = error {
                    debug!(device = %device, status = status.name(), error = %error, "lemnosd device status");
                }
                if let Some(known) = lock(&self.state).devices.iter_mut().find(|known| known.id == device) {
                    known.status = status;
                }
                self.changed = true;
            }
            Event::Control { device, by, .. } if by != self.options.client && self.fans.taken_over(&device) => {
                info!(device = %device, by = %by, "another client wrote the fan; HeliOS's override is over");
                self.changed = true;
            }
            Event::Edge { handle, rising, timestamp_ns, seq } => {
                if self.claims.edge(handle, EdgeView { rising, timestamp_ns, seq }) {
                    // Published like readings: at most once per reading interval.
                    self.readings_changed = true;
                }
            }
            Event::Dropped { count } => warn!(count, "lemnosd dropped events HeliOS did not read in time"),
            Event::Control { .. } | Event::LedOwner { .. } => {}
        }
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Status(status) => {
                self.status = Some(status);
                self.send_status();
            }
            Command::FanOverride { device, request, reply } => {
                let result = self.connected().and_then(|devices| devices.set(&device, FAN_DUTY_CONTROL, request.duty).map_err(describe));
                if let Ok(duty) = result {
                    let active = self.fans.start(&device, duty, Instant::now(), now_ms(), request.duration);
                    info!(device = %device, duty, until_ms = active.until_ms, "fan override started");
                    self.changed = true;
                }
                let _ = reply.send(result);
            }
            Command::FanRelease { device, reply } => {
                let result = self.release_fan(&device);
                if result.is_ok() {
                    info!(device = %device, "fan released to the kernel governor");
                }
                let _ = reply.send(result);
            }
            Command::Set { device, control, value, reply } => {
                let result = self.connected().and_then(|devices| devices.set(&device, &control, value).map_err(describe));
                let _ = reply.send(result);
            }
            Command::Raw { action, reply } => {
                let kind = action.kind();
                let result = self.raw(action);
                if let Err(error) = &result {
                    debug!(action = kind, error = %error, "raw action failed");
                }
                let _ = reply.send(result);
            }
            #[cfg(test)]
            Command::Abandon => {}
        }
    }

    /// Hands `device` back to the governor. Without a connection there is nothing to do:
    /// lemnosd undid the override when the connection closed.
    fn release_fan(&mut self, device: &str) -> Result<(), String> {
        let result = match self.devices.as_mut().filter(|devices| devices.is_connected()) {
            Some(devices) => devices.release(device),
            None => Ok(()),
        };
        self.fans.end(device);
        self.changed = true;
        match result {
            Ok(()) | Err(ClientError::Refused(Refusal::UnknownDevice)) | Err(ClientError::Closed) => Ok(()),
            Err(error) => Err(describe(error)),
        }
    }

    fn expire_overrides(&mut self) {
        for device in self.fans.expired(Instant::now()) {
            match self.release_fan(&device) {
                Ok(()) => info!(device = %device, "fan override ran out; back to the kernel governor"),
                Err(error) => warn!(device = %device, error = %error, "releasing the fan after its override failed"),
            }
        }
    }

    fn raw(&mut self, action: RawAction) -> Result<Option<ObservedValue>, String> {
        let (now, now_ms) = (Instant::now(), now_ms());
        let devices = self.devices.as_mut().filter(|devices| devices.is_connected()).ok_or_else(|| NOT_CONNECTED.to_string())?;
        let claims = &mut self.claims;
        let value = match action {
            RawAction::GpioClaim { claim, ttl } => {
                if claims.len() >= MAX_CLAIMS {
                    return Err(format!("at most {MAX_CLAIMS} raw claims at a time"));
                }
                let line = devices.claim_line_with(claim.target.clone(), claim.config, claim.on_release).map_err(describe)?;
                let id = claims.insert(ClaimSpec::Line(claim), Held::Line(line), ttl, now, now_ms);
                info!(claim = %id, "GPIO line claimed through lemnosd");
                self.changed = true;
                Some(ObservedValue::String(id))
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
                Some(ObservedValue::Bool(line.get(devices).map_err(describe)?))
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
                Some(ObservedValue::String(id))
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
            RawAction::I2cTransfer { bus, address, ops } => Some(ObservedValue::Bytes(devices.i2c(bus, address).transfer(devices, ops).map_err(describe)?)),
            RawAction::SpiTransfer { bus, chip_select, transfers } => Some(ObservedValue::Bytes(devices.spi(bus, chip_select).transfer(devices, transfers).map_err(describe)?)),
            RawAction::Renew { claim, ttl } => {
                let claim = claims.renew(&claim, None, ttl, now, now_ms)?;
                self.changed = true;
                Some(ObservedValue::UInt(claim.expires_at_ms))
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

    fn send_status(&mut self) {
        let (Some(leds), Some(status)) = (self.leds.as_mut(), self.status) else { return };
        // A stale connection fails the first send; the second reconnects.
        if leds.status(status).is_err()
            && let Err(error) = leds.status(status)
        {
            debug!(error = %error, status = status.name(), "LED status not sent; it is re-sent on the next connection");
        }
    }

    fn flush(&mut self) {
        let readings_due = self.readings_changed && self.last_notify.elapsed() >= self.options.reading_interval;
        if !(self.changed || readings_due) {
            return;
        }
        {
            let mut state = lock(&self.state);
            state.overrides = self.fans.active().map(|(device, active)| (device.to_string(), OverrideView { duty: active.duty, until_ms: active.until_ms })).collect();
            let mut raw = BTreeMap::new();
            self.claims.iter().for_each(|claim| claim.observe(&mut raw));
            state.claims = self.claims.len();
            state.raw = raw;
        }
        self.changed = false;
        self.readings_changed = false;
        self.last_notify = Instant::now();
        (self.notify)();
    }

    fn shutdown(mut self) {
        for device in self.fans.devices() {
            match self.release_fan(&device) {
                Ok(()) => info!(device = %device, "fan released on shutdown"),
                Err(error) => warn!(device = %device, error = %error, "fan release on shutdown failed; lemnosd hands it back when the connection closes"),
            }
        }
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
        if let Some(leds) = self.leds.as_mut() {
            let _ = leds.clear();
        }
    }
}

fn not_held(id: &str) -> String {
    format!("{id} is not held right now (lemnosd is reconnecting); try again")
}
