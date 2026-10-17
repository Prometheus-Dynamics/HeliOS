//! Pipeline outputs on NetworkTables (NT4), for robot code.
//!
//! Every graph tick's host outputs go to the robot's NT4 server as they are produced, not at
//! the Orion publish interval: `<prefix>/<pipeline>/<port>` (`string` holding the same JSON as
//! `GET /v1/outputs`), plus `<prefix>/<pipeline>/latency_ms` (input to outputs, `double`),
//! `fps` (`double`) and `capture_time_us` (`int`: when the frame came in, on the NT server's
//! clock, for latency compensation). Every topic carries properties `{"helios": 1, "kind": ..}`
//! (the output port, or `latency_ms`, `fps`, `capture_time_us`). The graph thread only hands the tick's payloads over (one
//! channel send, dropped when the publisher is behind); serializing and sending happen here.
//!
//! Settings are helios-api's (`PUT /v1/device/nt4`), read from its state file whenever it
//! changes: off, or the server (a team number's `10.TE.AM.2`, or a host) and the prefix
//! (`/helios` by default).

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime},
};

use daedalus::{runtime::host_bridge::ValueSerializerMap, transport::Payload};
use orion_nt4::{Client, ClientConfig, ClientEvent, ClientHandle, Publisher, Value};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::execution::outputs::payload_message;

/// Where helios-api keeps the NT4 settings.
pub const DEFAULT_SETTINGS_PATH: &str = "/var/lib/helios/api/nt4.json";
/// How often the settings file is checked for changes.
const SETTINGS_CHECK: Duration = Duration::from_secs(2);
/// Ticks waiting to be published; more are dropped (the newest tick matters, not every tick).
const QUEUE: usize = 8;
pub const DEFAULT_PREFIX: &str = "/helios";
const DEFAULT_PORT: u16 = 5810;

/// helios-api's NT4 settings (`nt4.json`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Nt4Settings {
    pub enabled: bool,
    /// FRC team number: the server is the roboRIO at `10.TE.AM.2`.
    pub team: Option<u32>,
    /// A server host instead of the team's roboRIO.
    pub server: Option<String>,
    pub port: u16,
    pub prefix: String,
}

impl Default for Nt4Settings {
    fn default() -> Self {
        Self { enabled: true, team: None, server: None, port: DEFAULT_PORT, prefix: DEFAULT_PREFIX.into() }
    }
}

impl Nt4Settings {
    /// The server to connect to, when publishing is on and there is one.
    pub fn host(&self) -> Option<String> {
        if !self.enabled {
            return None;
        }
        if let Some(server) = self.server.as_deref().map(str::trim).filter(|server| !server.is_empty()) {
            return Some(server.to_string());
        }
        self.team.filter(|team| (1..=25_599).contains(team)).map(|team| format!("10.{}.{}.2", team / 100, team % 100))
    }

    fn prefix(&self) -> String {
        let prefix = self.prefix.trim().trim_end_matches('/');
        if prefix.is_empty() {
            DEFAULT_PREFIX.into()
        } else if prefix.starts_with('/') {
            prefix.into()
        } else {
            format!("/{prefix}")
        }
    }
}

/// One graph tick's outputs.
pub struct Nt4Tick {
    pub pipeline: String,
    pub outputs: Vec<(String, Payload)>,
    pub serializers: ValueSerializerMap,
    pub latency_ms: f64,
    pub fps: f64,
}

/// Where graph threads hand their ticks over. Cheap to clone; a no-op while NT4 is off.
#[derive(Clone, Default)]
pub struct Nt4Sink {
    inner: Option<Arc<SinkInner>>,
}

struct SinkInner {
    tx: mpsc::Sender<Nt4Tick>,
    /// Connected to a server: ticks are only built while it is.
    active: AtomicBool,
}

impl Nt4Sink {
    /// Whether a tick handed over now would be published.
    pub fn active(&self) -> bool {
        self.inner.as_ref().is_some_and(|inner| inner.active.load(Ordering::Acquire))
    }

    /// Hand a tick over without waiting; dropped when the publisher is behind.
    pub fn offer(&self, tick: Nt4Tick) {
        if let Some(inner) = &self.inner {
            let _ = inner.tx.try_send(tick);
        }
    }
}

/// What the engine shows about the NT4 connection (on its `engine.runtime` resource).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Nt4Status {
    pub server: Option<String>,
    pub connected: bool,
}

/// Start the publisher on the current Tokio runtime; returns the sink graph threads use and the
/// connection status.
pub fn spawn(settings_path: PathBuf, client_name: String) -> (Nt4Sink, Arc<std::sync::Mutex<Nt4Status>>) {
    let (tx, rx) = mpsc::channel(QUEUE);
    let inner = Arc::new(SinkInner { tx, active: AtomicBool::new(false) });
    let status = Arc::new(std::sync::Mutex::new(Nt4Status::default()));
    tokio::spawn(run(settings_path, client_name, rx, inner.clone(), status.clone()));
    (Nt4Sink { inner: Some(inner) }, status)
}

struct Connection {
    client: Client,
    handle: ClientHandle,
    prefix: String,
    publishers: HashMap<String, Publisher>,
}

impl Connection {
    /// Set `topic`, publishing it first (with properties `{"helios": 1, "kind": kind}`).
    fn set(&mut self, topic: String, type_name: &str, value: Value, kind: &str) {
        let publisher = match self.publishers.get(&topic) {
            Some(publisher) => publisher,
            None => match self.handle.publish(&topic, type_name) {
                Ok(publisher) => {
                    let mut properties = orion_nt4::Properties::new();
                    properties.insert("helios".into(), 1.into());
                    properties.insert("kind".into(), kind.into());
                    let _ = self.handle.set_properties(&topic, properties);
                    self.publishers.entry(topic).or_insert(publisher)
                }
                Err(error) => {
                    warn!(%topic, %error, "NT4 publish failed");
                    return;
                }
            },
        };
        let _ = publisher.set(value);
    }

    fn publish(&mut self, tick: Nt4Tick) {
        let base = format!("{}/{}", self.prefix, topic_segment(&tick.pipeline));
        let now_server_us = self.handle.server_time_us();
        let latency_us = (tick.latency_ms * 1000.0) as i64;
        self.set(format!("{base}/capture_time_us"), "int", Value::Int(now_server_us - latency_us), "capture_time_us");
        self.set(format!("{base}/latency_ms"), "double", Value::Double(tick.latency_ms), "latency_ms");
        self.set(format!("{base}/fps"), "double", Value::Double(tick.fps), "fps");
        for (port, payload) in &tick.outputs {
            let message = payload_message(payload, &tick.serializers);
            self.set(format!("{base}/{}", topic_segment(port)), "string", Value::String(message), port);
        }
    }
}

async fn run(path: PathBuf, client_name: String, mut rx: mpsc::Receiver<Nt4Tick>, sink: Arc<SinkInner>, status: Arc<std::sync::Mutex<Nt4Status>>) {
    let mut check = tokio::time::interval(SETTINGS_CHECK);
    let mut seen: Option<(Option<SystemTime>, Nt4Settings)> = None;
    let mut connection: Option<Connection> = None;
    loop {
        tokio::select! {
            _ = check.tick() => {
                let modified = std::fs::metadata(&path).and_then(|meta| meta.modified()).ok();
                if seen.as_ref().is_some_and(|(at, _)| *at == modified) {
                    continue;
                }
                let settings = read_settings(&path);
                if seen.as_ref().is_some_and(|(_, before)| *before == settings) {
                    seen = Some((modified, settings));
                    continue;
                }
                connection = settings.host().map(|host| {
                    info!(%host, port = settings.port, prefix = %settings.prefix(), "NT4 publishing");
                    let mut config = ClientConfig::new(host, client_name.clone());
                    config.port = settings.port;
                    let client = Client::start(config);
                    Connection { handle: client.handle(), client, prefix: settings.prefix(), publishers: HashMap::new() }
                });
                if connection.is_none() {
                    info!("NT4 publishing off (disabled, or no team number or server)");
                }
                sink.active.store(false, Ordering::Relaxed);
                *status.lock().unwrap_or_else(|poison| poison.into_inner()) = Nt4Status { server: settings.host(), connected: false };
                seen = Some((modified, settings));
            }
            Some(event) = async { match connection.as_mut() { Some(c) => c.client.next_event().await, None => std::future::pending().await } } => {
                let connected = match event {
                    ClientEvent::Connected => true,
                    ClientEvent::Disconnected { .. } => false,
                    _ => continue,
                };
                status.lock().unwrap_or_else(|poison| poison.into_inner()).connected = connected;
                sink.active.store(connected, Ordering::Release);
            }
            Some(tick) = rx.recv() => {
                if let Some(connection) = connection.as_mut() {
                    connection.publish(tick);
                }
            }
        }
    }
}

fn read_settings(path: &Path) -> Nt4Settings {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            warn!(path = %path.display(), %error, "NT4 settings are not valid; using the defaults");
            Nt4Settings::default()
        }),
        Err(_) => Nt4Settings::default(),
    }
}

/// A topic path segment: letters, digits, `_`, `-` and `.`; anything else becomes `_`.
fn topic_segment(name: &str) -> String {
    let segment: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') { c } else { '_' }).collect();
    if segment.is_empty() { "_".into() } else { segment }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_pick_the_server() {
        let team = Nt4Settings { team: Some(6390), ..Nt4Settings::default() };
        assert_eq!(team.host().as_deref(), Some("10.63.90.2"));
        assert_eq!(Nt4Settings { server: Some(" 10.0.0.5 ".into()), ..team.clone() }.host().as_deref(), Some("10.0.0.5"));
        assert_eq!(Nt4Settings { enabled: false, ..team.clone() }.host(), None);
        assert_eq!(Nt4Settings::default().host(), None);
        assert_eq!(Nt4Settings { prefix: "cams/front/".into(), ..team }.prefix(), "/cams/front");
        assert_eq!(topic_segment("AprilTag front"), "AprilTag_front");
    }

    #[test]
    fn settings_file_defaults() {
        let dir = std::env::temp_dir().join(format!("helios-nt4-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("nt4.json");
        assert_eq!(read_settings(&path), Nt4Settings::default());
        std::fs::write(&path, r#"{"team": 254}"#).unwrap();
        assert_eq!(read_settings(&path).host().as_deref(), Some("10.2.54.2"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn ticks_reach_an_nt4_server() {
        use orion_nt4::{Server, ServerConfig};
        let server = Server::start(ServerConfig { bind: "127.0.0.1:0".parse().unwrap(), ..ServerConfig::default() }).await.expect("server");
        let port = server.local_addr().port();
        let dir = std::env::temp_dir().join(format!("helios-nt4-e2e-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("nt4.json");
        std::fs::write(&path, format!(r#"{{"server": "127.0.0.1", "port": {port}, "prefix": "/helios"}}"#)).unwrap();
        let (sink, status) = spawn(path, "helios-test".into());
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !sink.active() {
            assert!(tokio::time::Instant::now() < deadline, "never connected");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(status.lock().unwrap().connected);
        let serializers = ValueSerializerMap::default();
        sink.offer(Nt4Tick { pipeline: "front".into(), outputs: vec![("count".into(), Payload::owned("test:count", 3_i64))], serializers, latency_ms: 4.5, fps: 30.0 });
        let host = server.handle();
        while host.value("/helios/front/count").is_none() {
            assert!(tokio::time::Instant::now() < deadline, "no value");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(host.value("/helios/front/latency_ms"), Some(Value::Double(4.5)));
        assert_eq!(host.value("/helios/front/fps"), Some(Value::Double(30.0)));
        let _ = std::fs::remove_dir_all(dir);
    }
}
