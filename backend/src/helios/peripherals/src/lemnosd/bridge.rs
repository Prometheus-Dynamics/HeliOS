//! The connection to lemnosd: one thread owns a reconnecting `DeviceClient` and `LedClient`
//! (both blocking, like Styx's clients), keeps [`LemnosdState`] current for the resource probe,
//! runs fan overrides and their releases, and holds HeliOS's status on the status light.
//!
//! - Connected (first time or after a lemnosd restart): list the devices, subscribe to every
//!   device with channels, send the owed fan releases, re-send the LED status.
//! - Disconnected: the devices go missing, readings are dropped, and running fan overrides end
//!   (their releases are owed).
//! - The runtime is told about changes through `notify`: at once for connection, device-list,
//!   status and override changes, and at most once per reading interval for new readings.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use lemnos_ipc::{ClientError, ClientEvent, ClientOptions, DeviceClient, Event, LedClient, LedStatus, Reading, Refusal, Update};
use tokio::sync::oneshot;
use tracing::{debug, info, warn};

use crate::lemnosd::fan::{FAN_DUTY_CONTROL, FanOverrideRequest, FanOverrides, read_marker, write_marker};
use crate::lemnosd::resources::{DeviceReading, LemnosdProbe, LemnosdState, OverrideView};

/// How long the thread waits for lemnosd's events before it looks at commands and timers.
const TICK: Duration = Duration::from_millis(50);
const CONNECT_RETRY_MIN: Duration = Duration::from_millis(500);
const CONNECT_RETRY_MAX: Duration = Duration::from_secs(5);
const OWED_RETRY: Duration = Duration::from_secs(1);
/// Events handled per pass before commands get a turn.
const MAX_EVENTS_PER_PASS: usize = 256;

#[derive(Debug, Clone)]
pub struct LemnosdOptions {
    pub socket: PathBuf,
    /// The client name; the fan's board `writers` must list it.
    pub client: String,
    /// The subscription period, and the most often readings are republished.
    pub reading_interval: Duration,
    /// Where the fans that still need a release are recorded, across a crash.
    pub fan_marker: Option<PathBuf>,
}

/// Called from the bridge thread when [`LemnosdState`] changed.
pub type Notify = Box<dyn Fn() + Send + 'static>;

enum Command {
    Status(LedStatus),
    FanOverride { device: String, request: FanOverrideRequest, reply: oneshot::Sender<Result<f64, String>> },
    FanRelease { device: String, reply: oneshot::Sender<Result<(), String>> },
    Set { device: String, control: String, value: f64, reply: oneshot::Sender<Result<f64, String>> },
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

    /// Stops the thread: running overrides are released first.
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
        ClientError::Refused(refusal) => format!("lemnosd refused: {refusal}"),
        other => other.to_string(),
    }
}

struct Worker {
    options: LemnosdOptions,
    state: Arc<Mutex<LemnosdState>>,
    notify: Notify,
    devices: Option<DeviceClient>,
    leds: Option<LedClient>,
    status: Option<LedStatus>,
    fans: FanOverrides,
    next_connect: Instant,
    connect_backoff: Duration,
    connect_warned: bool,
    next_owed_retry: Instant,
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
            leds: None,
            status: None,
            fans: FanOverrides::default(),
            next_connect: now,
            connect_backoff: CONNECT_RETRY_MIN,
            connect_warned: false,
            next_owed_retry: now,
            changed: false,
            readings_changed: false,
            last_notify: now,
        }
    }

    fn run(mut self, commands: Receiver<Command>) {
        // Fans a previous helios-peripherals left overridden (it crashed): release them first.
        if let Some(marker) = &self.options.fan_marker {
            for device in read_marker(marker) {
                info!(device = %device, "releasing a fan override left by a previous run");
                self.fans.owe(&device);
            }
        }
        loop {
            if self.devices.is_none() {
                self.connect();
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
                    Ok(command) => self.command(command),
                    Err(true) => return self.shutdown(),
                    Err(false) => break,
                }
                next = commands.try_recv().map_err(|error| error == TryRecvError::Disconnected);
            }
            self.expire_overrides();
            self.flush();
        }
    }

    fn connect(&mut self) {
        let now = Instant::now();
        if now < self.next_connect {
            return;
        }
        match ClientOptions::new(&self.options.socket, self.options.client.clone()).reconnecting().devices() {
            Ok(devices) => {
                // Its `Connected` event is queued: the device list and subscriptions follow there.
                self.devices = Some(devices);
                self.connect_backoff = CONNECT_RETRY_MIN;
                self.connect_warned = false;
            }
            Err(error) => {
                if self.connect_warned {
                    debug!(socket = %self.options.socket.display(), error = %error, "lemnosd is not reachable yet");
                } else {
                    warn!(socket = %self.options.socket.display(), error = %error, "lemnosd is not reachable; retrying");
                    self.connect_warned = true;
                }
                self.next_connect = now + self.connect_backoff;
                self.connect_backoff = (self.connect_backoff * 2).min(CONNECT_RETRY_MAX);
            }
        }
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
        self.release_owed();
        if self.leds.is_none() {
            match ClientOptions::new(&self.options.socket, self.options.client.clone()).reconnecting().leds() {
                Ok(leds) => self.leds = Some(leds),
                Err(error) => warn!(error = %error, "lemnosd LED client failed to connect"),
            }
        }
        self.send_status();
        self.changed = true;
    }

    fn on_disconnected(&mut self, error: &ClientError) {
        warn!(error = %error, "lost the lemnosd connection; its devices are offline until it is back");
        if self.fans.active().next().is_some() {
            warn!("the fan override ended with the lemnosd connection");
        }
        self.fans.disconnected();
        self.write_marker();
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
            Event::Status { device, status, error } => {
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
                self.write_marker();
                self.changed = true;
            }
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
                let result = match self.devices.as_mut() {
                    Some(devices) => devices.set(&device, FAN_DUTY_CONTROL, request.duty).map_err(describe),
                    None => Err("lemnosd is not connected".into()),
                };
                if let Ok(duty) = result {
                    let active = self.fans.start(&device, duty, Instant::now(), now_ms(), request.duration);
                    info!(device = %device, duty, until_ms = active.until_ms, "fan override started");
                    self.write_marker();
                    self.changed = true;
                }
                let _ = reply.send(result);
            }
            Command::FanRelease { device, reply } => {
                let result = self.release(&device);
                if result.is_ok() {
                    info!(device = %device, "fan released to the kernel governor");
                }
                let _ = reply.send(result);
            }
            Command::Set { device, control, value, reply } => {
                let result = match self.devices.as_mut() {
                    Some(devices) => devices.set(&device, &control, value).map_err(describe),
                    None => Err("lemnosd is not connected".into()),
                };
                let _ = reply.send(result);
            }
        }
    }

    /// Releases `device`; a release lemnosd cannot take now is owed.
    fn release(&mut self, device: &str) -> Result<(), String> {
        let result = match self.devices.as_mut() {
            Some(devices) => devices.release(device),
            None => Err(ClientError::Closed),
        };
        match result {
            Ok(()) | Err(ClientError::Refused(Refusal::UnknownDevice)) => {
                self.fans.end(device);
                self.write_marker();
                self.changed = true;
                Ok(())
            }
            Err(ClientError::Refused(refusal)) => {
                // Not ours to release (the write policy changed): nothing to retry.
                self.fans.end(device);
                self.write_marker();
                self.changed = true;
                Err(format!("lemnosd refused: {refusal}"))
            }
            Err(error) => {
                self.fans.owe(device);
                self.write_marker();
                self.changed = true;
                Err(describe(error))
            }
        }
    }

    fn release_owed(&mut self) {
        for device in self.fans.owed() {
            match self.release(&device) {
                Ok(()) => info!(device = %device, "owed fan release sent"),
                Err(error) => warn!(device = %device, error = %error, "fan release failed"),
            }
        }
        self.next_owed_retry = Instant::now() + OWED_RETRY;
    }

    fn expire_overrides(&mut self) {
        let now = Instant::now();
        for device in self.fans.expired(now) {
            match self.release(&device) {
                Ok(()) => info!(device = %device, "fan override ran out; back to the kernel governor"),
                Err(error) => warn!(device = %device, error = %error, "releasing the fan after its override failed; retrying"),
            }
        }
        let connected = self.devices.as_ref().is_some_and(DeviceClient::is_connected);
        if connected && now >= self.next_owed_retry && !self.fans.owed().is_empty() {
            self.release_owed();
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

    fn write_marker(&self) {
        if let Some(marker) = &self.options.fan_marker
            && let Err(error) = write_marker(marker, &self.fans.needing_release())
        {
            warn!(path = %marker.display(), error = %error, "failed to record the fans that need a release");
        }
    }

    fn flush(&mut self) {
        let readings_due = self.readings_changed && self.last_notify.elapsed() >= self.options.reading_interval;
        if !(self.changed || readings_due) {
            return;
        }
        lock(&self.state).overrides = self.fans.active().map(|(device, active)| (device.to_string(), OverrideView { duty: active.duty, until_ms: active.until_ms })).collect();
        self.changed = false;
        self.readings_changed = false;
        self.last_notify = Instant::now();
        (self.notify)();
    }

    fn shutdown(mut self) {
        for device in self.fans.needing_release() {
            match self.release(&device) {
                Ok(()) => info!(device = %device, "fan released on shutdown"),
                Err(error) => warn!(device = %device, error = %error, "fan release on shutdown failed"),
            }
        }
        if let Some(leds) = self.leds.as_mut() {
            let _ = leds.clear();
        }
    }
}
