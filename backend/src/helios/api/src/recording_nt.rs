//! Recording over NetworkTables, and auto-record. helios-api keeps a second, low-rate NT4 client on
//! the robot's server (the connection of `nt4.json`, as the engine's publisher uses it):
//! - `<prefix>/record` (boolean): a change to true starts a recording from the default camera, to
//!   false stops the running one;
//! - `<prefix>/capture_last` (boolean): a rising edge writes the capture-last clip (the first camera
//!   with a ring);
//! - `<prefix>/map/save` (boolean): a rising edge saves the map of each running live-map pipeline,
//!   named after the match (`/FMSInfo`), as `POST /v1/pipelines/{id}/map/save` does;
//! - `<prefix>/recording/{active,bytes,dropped,file}` (boolean, int, int, string): the status, about
//!   once a second (`dropped`: frames and host messages not written);
//! - auto-record (the `auto_record` setting) reads the FMS topics `/FMSInfo/FMSControlData` (bit 0:
//!   enabled), `EventName`, `MatchType` and `MatchNumber`: one recording per enabled period, named
//!   after the match.
//!
//! Why here and not in the engine: the recorder and its state are helios-api's, so a command makes
//! the same call as the HTTP route. The engine's NT publisher is the one-way, high-rate path that
//! graph threads feed; it stays that way.

use std::time::Duration;

use helios_recorder::RecordingStatus;
use orion_nt4::{Client, ClientConfig, ClientEvent, ClientHandle, Publisher, SubscribeOptions, Subscription, TopicEvent, Value};
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{debug, info, warn};

use crate::{
    SharedState,
    recordings::{self, StartRecording},
    routes::{maps, pipelines},
};

pub const FMS_PREFIX: &str = "/FMSInfo";
pub const FMS_CONTROL_DATA: &str = "/FMSInfo/FMSControlData";
pub const FMS_EVENT_NAME: &str = "/FMSInfo/EventName";
pub const FMS_MATCH_TYPE: &str = "/FMSInfo/MatchType";
pub const FMS_MATCH_NUMBER: &str = "/FMSInfo/MatchNumber";

/// How often the settings are checked for a new server, prefix or NT control switch.
const CHECK_EVERY: Duration = Duration::from_secs(2);
/// How often the status is published.
const STATUS_EVERY: Duration = Duration::from_secs(1);

/// The topics of recording control and status, under the NT prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topics {
    pub record: String,
    pub capture_last: String,
    pub map_save: String,
    pub active: String,
    pub bytes: String,
    pub dropped: String,
    pub file: String,
}

pub fn topics(prefix: &str) -> Topics {
    let status = format!("{prefix}/recording");
    Topics {
        record: format!("{prefix}/record"),
        capture_last: format!("{prefix}/capture_last"),
        map_save: format!("{prefix}/map/save"),
        active: format!("{status}/active"),
        bytes: format!("{status}/bytes"),
        dropped: format!("{status}/dropped"),
        file: format!("{status}/file"),
    }
}

/// What a control topic asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Record(bool),
    CaptureLast,
    SaveMap,
}

/// Turns the control topics' values into commands. `record` acts on every change (its first value
/// included); `capture_last` on each rising edge, so a value left true at connect does not write a
/// clip.
#[derive(Debug, Default)]
pub struct Controls {
    record: Option<bool>,
    capture_last: Option<bool>,
    map_save: Option<bool>,
}

impl Controls {
    pub fn record(&mut self, value: bool) -> Option<Command> {
        (self.record.replace(value) != Some(value)).then_some(Command::Record(value))
    }

    pub fn capture_last(&mut self, value: bool) -> Option<Command> {
        let previous = self.capture_last.replace(value);
        (previous == Some(false) && value).then_some(Command::CaptureLast)
    }

    /// A rising edge of `map/save` saves the maps (a value left true at connect does not).
    pub fn map_save(&mut self, value: bool) -> Option<Command> {
        let previous = self.map_save.replace(value);
        (previous == Some(false) && value).then_some(Command::SaveMap)
    }
}

/// The FMS state the robot reports (`/FMSInfo`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fms {
    pub enabled: bool,
    pub event: String,
    pub match_type: i64,
    pub match_number: i64,
}

impl Fms {
    /// Applies one value of an FMS topic; other topics are left alone.
    pub fn apply(&mut self, topic: &str, value: &Value) {
        match topic {
            FMS_CONTROL_DATA => {
                if let Some(word) = value.as_i64() {
                    self.enabled = word & 1 == 1;
                }
            }
            FMS_EVENT_NAME => {
                if let Some(event) = value.as_str() {
                    self.event = event.trim().to_string();
                }
            }
            FMS_MATCH_TYPE => {
                if let Some(kind) = value.as_i64() {
                    self.match_type = kind;
                }
            }
            FMS_MATCH_NUMBER => {
                if let Some(number) = value.as_i64() {
                    self.match_number = number;
                }
            }
            _ => {}
        }
    }

    /// The file-name part of this match: `<event>-<p|q|e><number>` (practice, qualification,
    /// elimination), with whichever parts are known; `None` when there is neither an event nor a
    /// match number.
    pub fn label(&self) -> Option<String> {
        let letter = match self.match_type {
            1 => "p",
            2 => "q",
            3 => "e",
            _ => "",
        };
        let matched = (self.match_number > 0).then(|| format!("{letter}{}", self.match_number));
        let parts: Vec<&str> = [(!self.event.is_empty()).then_some(self.event.as_str()), matched.as_deref()].into_iter().flatten().collect();
        (!parts.is_empty()).then(|| parts.join("-"))
    }
}

/// What auto-record does when the robot's enable changes.
#[derive(Debug, Default)]
pub struct AutoRecord {
    enabled: bool,
    recording: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Auto {
    Start { label: Option<String> },
    Stop { id: String },
}

impl AutoRecord {
    /// The action an enable state makes (none when it is the state already).
    pub fn enabled(&mut self, enabled: bool, label: Option<String>) -> Option<Auto> {
        if enabled == self.enabled {
            return None;
        }
        self.enabled = enabled;
        if enabled { Some(Auto::Start { label }) } else { self.recording.take().map(|id| Auto::Stop { id }) }
    }

    /// The recording a start made (its stop comes with the disable).
    pub fn started(&mut self, id: String) {
        self.recording = Some(id);
    }
}

/// The status values published: active, bytes, dropped and the file being written.
pub fn status_values(status: Option<&RecordingStatus>) -> (bool, i64, i64, String) {
    match status {
        Some(status) => (status.state.is_active(), status.bytes as i64, (status.frames_dropped + status.messages_dropped) as i64, status.file.clone().unwrap_or_default()),
        None => (false, 0, 0, String::new()),
    }
}

/// The NT server and topics a connection uses.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Connection {
    host: String,
    port: u16,
    prefix: String,
}

/// Work for the worker, which runs one job at a time (a start opens a camera).
enum Job {
    Command(Command),
    Enabled { enabled: bool, label: Option<String> },
    /// `map/save`: the running live-map pipelines save, named `name` (the match, when known).
    SaveMap { name: Option<String> },
}

struct Publishers {
    active: Publisher,
    bytes: Publisher,
    dropped: Publisher,
    file: Publisher,
}

/// A live connection: its handle (the connection lives while it exists), its subscriptions, its
/// publishers and the task that drains the client's events.
struct Link {
    /// Keeps the connection alive (it lives while a handle does).
    _connection: ClientHandle,
    control: Option<Subscription>,
    fms: Option<Subscription>,
    publishers: Option<Publishers>,
    topics: Topics,
    events: JoinHandle<()>,
}

impl Link {
    fn open(connection: &Connection) -> Self {
        let mut config = ClientConfig::new(connection.host.clone(), "helios-api-recording");
        config.port = connection.port;
        let (handle, mut events) = Client::start(config).split();
        // A lost connection is worth a warning; a server that is not up yet (no robot program
        // running) is retried every few seconds and only logged at debug, so it does not flood the log.
        let events = tokio::spawn(async move {
            let mut connected = false;
            while let Some(event) = events.recv().await {
                match event {
                    ClientEvent::Connected => {
                        connected = true;
                        info!("recording NT: connected");
                    }
                    ClientEvent::Disconnected { reason } if connected => {
                        connected = false;
                        warn!(%reason, "recording NT: disconnected");
                    }
                    ClientEvent::Disconnected { reason } => debug!(%reason, "recording NT: server not reachable yet"),
                    _ => {}
                }
            }
        });
        let topics = topics(&connection.prefix);
        let control = handle.subscribe(&[topics.record.as_str(), topics.capture_last.as_str(), topics.map_save.as_str()], SubscribeOptions::default()).ok();
        let fms = handle.subscribe(&[FMS_PREFIX], SubscribeOptions { prefix: true, ..SubscribeOptions::default() }).ok();
        let publish = |name: &str, type_name: &str| handle.publish(name, type_name).ok();
        let publishers = match (publish(&topics.active, "boolean"), publish(&topics.bytes, "int"), publish(&topics.dropped, "int"), publish(&topics.file, "string")) {
            (Some(active), Some(bytes), Some(dropped), Some(file)) => Some(Publishers { active, bytes, dropped, file }),
            _ => {
                warn!("recording NT: status topics could not be published");
                None
            }
        };
        Self { _connection: handle, control, fms, publishers, topics, events }
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.events.abort();
    }
}

/// Starts the NT client and the worker. Settings are read from the stores; with `nt_control` off,
/// or no NT4 server, nothing connects.
pub fn spawn(state: SharedState) -> JoinHandle<()> {
    let (jobs_tx, jobs_rx) = mpsc::unbounded_channel();
    tokio::spawn(work(state.clone(), jobs_rx));
    tokio::spawn(run(state, jobs_tx))
}

/// The connection the settings ask for (none with `nt_control` off or no NT4 server).
async fn wanted(state: &SharedState) -> Option<Connection> {
    let nt4 = state.store.nt4_settings().await.unwrap_or_default();
    let controller = state.store.controller_facts().await.unwrap_or_default();
    let recorder = state.store.recording_settings().await.unwrap_or_default();
    let host = helios_robot::resolve(&nt4, controller.as_ref()).nt4_server.value?;
    (recorder.nt_control).then(|| Connection { host, port: nt4.port, prefix: nt4.topic_prefix() })
}

async fn run(state: SharedState, jobs: mpsc::UnboundedSender<Job>) {
    let mut link: Option<Link> = None;
    let mut connection: Option<Connection> = None;
    let mut fms = Fms::default();
    let mut controls = Controls::default();
    let mut check = tokio::time::interval(CHECK_EVERY);
    let mut status = tokio::time::interval(STATUS_EVERY);
    let mut control_sub: Option<Subscription> = None;
    let mut fms_sub: Option<Subscription> = None;
    loop {
        tokio::select! {
            _ = check.tick() => {
                let wanted = wanted(&state).await;
                if wanted != connection {
                    connection = wanted;
                    control_sub = None;
                    fms_sub = None;
                    link = connection.as_ref().map(Link::open);
                    if let Some(open) = link.as_mut() {
                        control_sub = open.control.take();
                        fms_sub = open.fms.take();
                    }
                }
            }
            event = async { match control_sub.as_mut() { Some(sub) => sub.next().await, None => std::future::pending().await } } => {
                let Some(event) = event else {
                    // The subscription ended (the client stopped): reconnect on the next check.
                    control_sub = None;
                    fms_sub = None;
                    link = None;
                    connection = None;
                    continue;
                };
                let Some(open) = link.as_ref() else { continue };
                if let TopicEvent::Value { name, value, .. } = event
                    && let Some(on) = value.as_bool()
                {
                    let command = if name == open.topics.record {
                        controls.record(on)
                    } else if name == open.topics.capture_last {
                        controls.capture_last(on)
                    } else if name == open.topics.map_save {
                        controls.map_save(on)
                    } else {
                        None
                    };
                    if let Some(command) = command {
                        let job = match command {
                            Command::SaveMap => Job::SaveMap { name: fms.label() },
                            command => Job::Command(command),
                        };
                        let _ = jobs.send(job);
                    }
                }
            }
            event = async { match fms_sub.as_mut() { Some(sub) => sub.next().await, None => std::future::pending().await } } => {
                let Some(TopicEvent::Value { name, value, .. }) = event else { continue };
                let before = fms.enabled;
                fms.apply(&name, &value);
                if name == FMS_CONTROL_DATA && fms.enabled != before {
                    let _ = jobs.send(Job::Enabled { enabled: fms.enabled, label: fms.label() });
                }
            }
            _ = status.tick() => {
                if let Some(publishers) = link.as_ref().and_then(|open| open.publishers.as_ref()) {
                    let recording = state.recorder.status();
                    let (active, bytes, dropped, file) = status_values(recording.as_ref());
                    let _ = publishers.active.set(Value::Boolean(active));
                    let _ = publishers.bytes.set(Value::Int(bytes));
                    let _ = publishers.dropped.set(Value::Int(dropped));
                    let _ = publishers.file.set(Value::String(file));
                }
            }
        }
    }
}

/// Runs the jobs one at a time: commands, and the auto-record starts and stops.
async fn work(state: SharedState, mut jobs: mpsc::UnboundedReceiver<Job>) {
    let mut auto = AutoRecord::default();
    while let Some(job) = jobs.recv().await {
        match job {
            Job::Command(Command::Record(true)) => {
                if state.recorder.status().is_some_and(|status| status.state.is_active()) {
                    continue;
                }
                match start_from_robot(&state, None).await {
                    Ok(status) => info!(id = %status.id, "recording started over NetworkTables"),
                    Err(error) => warn!(error = %error.message, "recording over NetworkTables not started"),
                }
            }
            Job::Command(Command::Record(false)) => {
                if let Some(status) = state.recorder.status().filter(|status| status.state.is_active())
                    && let Err(error) = recordings::stop_recording(&state, &status.id).await
                {
                    warn!(error = %error.message, "recording over NetworkTables not stopped");
                }
            }
            Job::Command(Command::CaptureLast) => match recordings::capture_clip(&state, None, None).await {
                Ok(clip) => info!(file = %clip.file, "capture-last clip written over NetworkTables"),
                Err(error) => warn!(error = %error.message, "capture-last over NetworkTables refused"),
            },
            Job::Command(Command::SaveMap) => {}
            Job::SaveMap { name } => save_maps(&state, name).await,
            Job::Enabled { enabled, label } => {
                let auto_on = state.store.recording_settings().await.map(|settings| settings.auto_record).unwrap_or(false);
                match auto.enabled(enabled && auto_on, label) {
                    Some(Auto::Start { label }) => match start_from_robot(&state, label).await {
                        Ok(status) => {
                            info!(id = %status.id, "auto-record started: the robot is enabled");
                            auto.started(status.id);
                        }
                        Err(error) => warn!(error = %error.message, "auto-record not started"),
                    },
                    Some(Auto::Stop { id }) => match recordings::stop_recording(&state, &id).await {
                        Ok(_) => info!(%id, "auto-record stopped: the robot is disabled"),
                        Err(error) => warn!(%id, error = %error.message, "auto-record not stopped"),
                    },
                    None => {}
                }
            }
        }
    }
}

/// Saves the map of each running live-map pipeline (`map/save`), each named `name`.
async fn save_maps(state: &SharedState, name: Option<String>) {
    let ids = match state.orion.view().await {
        Ok(view) => pipelines::live_map_pipelines(&view),
        Err(error) => return warn!(error = %error.message, "maps over NetworkTables not saved"),
    };
    for id in ids {
        match maps::save_pipeline(state, &id, name.clone()).await {
            Ok(entry) => info!(pipeline = %id, map = %entry.id, "map saved over NetworkTables"),
            Err(error) => warn!(pipeline = %id, error = %error.message, "map over NetworkTables not saved"),
        }
    }
}

/// Starts a recording from the default camera, named with `label`.
async fn start_from_robot(state: &SharedState, label: Option<String>) -> crate::error::ApiResult<RecordingStatus> {
    let camera = recordings::default_camera(state).await?;
    let request = StartRecording { camera, ..StartRecording::default() };
    recordings::start_recording(state, request, label).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_topics_follow_the_nt_prefix() {
        let names = topics("/helios");
        assert_eq!(names.record, "/helios/record");
        assert_eq!(names.capture_last, "/helios/capture_last");
        assert_eq!(names.active, "/helios/recording/active");
        assert_eq!(names.file, "/helios/recording/file");
        assert_eq!(names.bytes, "/helios/recording/bytes");
        assert_eq!(names.dropped, "/helios/recording/dropped");
        assert_eq!(topics("/cams/front").record, "/cams/front/record");
    }

    #[test]
    fn record_follows_every_change_and_capture_last_only_rising_edges() {
        let mut controls = Controls::default();
        assert_eq!(controls.record(true), Some(Command::Record(true)));
        assert_eq!(controls.record(true), None);
        assert_eq!(controls.record(false), Some(Command::Record(false)));
        // A clip is triggered by true after false; a value left true at connect is not one.
        let mut edges = Controls::default();
        assert_eq!(edges.capture_last(true), None);
        assert_eq!(edges.capture_last(false), None);
        assert_eq!(edges.capture_last(true), Some(Command::CaptureLast));
        assert_eq!(edges.capture_last(true), None);
    }

    #[test]
    fn map_save_is_a_rising_edge_on_its_own_topic() {
        assert_eq!(topics("/helios").map_save, "/helios/map/save");
        let mut controls = Controls::default();
        assert_eq!(controls.map_save(true), None, "a value true at connect saves nothing");
        assert_eq!(controls.map_save(true), None, "held true is not a new save");
        assert_eq!(controls.map_save(false), None);
        assert_eq!(controls.map_save(true), Some(Command::SaveMap));
    }

    #[test]
    fn fms_control_data_bit_zero_is_enabled() {
        let mut fms = Fms::default();
        fms.apply(FMS_CONTROL_DATA, &Value::Int(0b0_0001));
        assert!(fms.enabled);
        fms.apply(FMS_CONTROL_DATA, &Value::Int(0b0_0010));
        assert!(!fms.enabled, "autonomous without enable is not enabled");
        fms.apply(FMS_CONTROL_DATA, &Value::Int(0b1_0101));
        assert!(fms.enabled);
        // Other topics leave the state as it is.
        let before = fms.clone();
        fms.apply("/FMSInfo/StationNumber", &Value::Int(2));
        assert_eq!(fms, before);
    }

    #[test]
    fn the_label_names_the_match() {
        let mut fms = Fms::default();
        assert_eq!(fms.label(), None);
        fms.apply(FMS_EVENT_NAME, &Value::String(" ORNC ".into()));
        assert_eq!(fms.label().as_deref(), Some("ORNC"));
        fms.apply(FMS_MATCH_TYPE, &Value::Int(2));
        fms.apply(FMS_MATCH_NUMBER, &Value::Int(12));
        assert_eq!(fms.label().as_deref(), Some("ORNC-q12"));
        fms.apply(FMS_MATCH_TYPE, &Value::Int(3));
        fms.apply(FMS_MATCH_NUMBER, &Value::Int(0));
        assert_eq!(fms.label().as_deref(), Some("ORNC"), "no match number: the event alone");
        fms.apply(FMS_EVENT_NAME, &Value::String(String::new()));
        fms.apply(FMS_MATCH_NUMBER, &Value::Int(3));
        assert_eq!(fms.label().as_deref(), Some("e3"), "no event: the match alone");
        fms.apply(FMS_MATCH_TYPE, &Value::Int(1));
        assert_eq!(fms.label().as_deref(), Some("p3"));
    }

    #[test]
    fn one_recording_per_enabled_period() {
        let mut auto = AutoRecord::default();
        assert_eq!(auto.enabled(false, None), None, "disabled stays disabled");
        assert_eq!(auto.enabled(true, Some("ORNC-q1".into())), Some(Auto::Start { label: Some("ORNC-q1".into()) }));
        assert_eq!(auto.enabled(true, None), None, "a second enable message changes nothing");
        auto.started("board-cam-1".into());
        assert_eq!(auto.enabled(false, None), Some(Auto::Stop { id: "board-cam-1".into() }));
        assert_eq!(auto.enabled(false, None), None);
        // A start that was refused (a recording already ran) has no stop to make.
        assert_eq!(auto.enabled(true, None), Some(Auto::Start { label: None }));
        assert_eq!(auto.enabled(false, None), None);
    }

    #[test]
    fn status_values_are_idle_without_a_recording() {
        assert_eq!(status_values(None), (false, 0, 0, String::new()));
    }
}
