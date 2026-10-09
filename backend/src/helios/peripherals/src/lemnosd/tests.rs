//! The bridge against a fake lemnosd: a Unix socket speaking `lemnos_ipc::wire`.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lemnos_device::DeviceStatus;
use lemnos_ipc::{Event, LedShow, LedStatus, Message, RawReading, Request, VERSION, decode_request};

use super::resources::tests::raze_devices;
use super::{FanOverrideRequest, LemnosdBridge, LemnosdOptions};

/// Records every request and answers like lemnosd (every write accepted).
struct FakeLemnosd {
    requests: Arc<Mutex<Vec<Request>>>,
    streams: Arc<Mutex<Vec<UnixStream>>>,
}

impl FakeLemnosd {
    fn serve(path: &Path) -> Self {
        let listener = UnixListener::bind(path).expect("bind");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let streams = Arc::new(Mutex::new(Vec::new()));
        let (accept_requests, accept_streams) = (Arc::clone(&requests), Arc::clone(&streams));
        std::thread::spawn(move || {
            for (client_id, stream) in listener.incoming().enumerate() {
                let Ok(stream) = stream else { return };
                accept_streams.lock().unwrap().push(stream.try_clone().expect("clone"));
                let requests = Arc::clone(&accept_requests);
                std::thread::spawn(move || serve_client(stream, client_id as u32, &requests));
            }
        });
        Self { requests, streams }
    }

    fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }

    fn count(&self, matches: impl Fn(&Request) -> bool) -> usize {
        self.requests().iter().filter(|request| matches(request)).count()
    }

    /// Drops every client connection, as a lemnosd restart does.
    fn kick(&self) {
        for stream in self.streams.lock().unwrap().drain(..) {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }

    fn broadcast(&self, message: &Message) {
        for mut stream in self.streams.lock().unwrap().iter() {
            let _ = stream.write_all(&message.encode());
        }
    }
}

fn serve_client(mut stream: UnixStream, client_id: u32, requests: &Mutex<Vec<Request>>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
        while let Ok(Some((request, used))) = decode_request(&buf) {
            buf.drain(..used);
            requests.lock().unwrap().push(request.clone());
            let answer = match request {
                Request::Hello { .. } => Some(Message::Welcome { version: VERSION, board: "raze".into(), client_id }),
                Request::List => Some(Message::Devices(raze_devices())),
                Request::Subscribe { device, period_ms } if period_ms > 0 && device == "fan" => {
                    Some(Message::Reading(RawReading { device, timestamp_us: 1_000, status: DeviceStatus::Available, values: vec![702, 3_000] }))
                }
                Request::Set { id, value, .. } => Some(Message::Reply { id, result: Ok(value) }),
                Request::Release { id, .. } | Request::Get { id, .. } => Some(Message::Reply { id, result: Ok(0.0) }),
                _ => None,
            };
            if let Some(answer) = answer
                && stream.write_all(&answer.encode()).is_err()
            {
                return;
            }
        }
    }
}

fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn is_status(request: &Request, status: LedStatus) -> bool {
    matches!(request, Request::Led(led) if led.show == LedShow::Status(status))
}

fn is_release(request: &Request) -> bool {
    matches!(request, Request::Release { device, .. } if device == "fan")
}

fn bridge(socket: PathBuf, marker: PathBuf) -> (LemnosdBridge, Arc<AtomicUsize>) {
    let bridge = LemnosdBridge::new(LemnosdOptions { socket, client: "helios".into(), reading_interval: Duration::from_millis(20), fan_marker: Some(marker) });
    let notified = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&notified);
    bridge
        .start(Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }))
        .expect("start");
    (bridge, notified)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime")
}

#[test]
fn bridge_publishes_devices_runs_a_timed_override_and_survives_a_restart() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("lemnosd.sock");
    let marker = dir.path().join("fan-override");
    let lemnosd = FakeLemnosd::serve(&socket);
    let (bridge, notified) = bridge(socket, marker.clone());
    bridge.set_status(LedStatus::Busy);

    // Connected: devices listed, the fan subscribed, its reading kept, the status sent.
    wait_until("the device list", || bridge.state().connected && bridge.state().devices.len() == 4);
    wait_until("the fan reading", || bridge.state().readings.get("fan").is_some_and(|reading| reading.values.contains(&("duty".to_string(), Some(0.702)))));
    wait_until("the LED status", || lemnosd.count(|request| is_status(request, LedStatus::Busy)) >= 1);
    assert!(lemnosd.requests().iter().any(|request| matches!(request, Request::Hello { client, .. } if client == "helios")));
    assert!(lemnosd.requests().iter().any(|request| matches!(request, Request::Subscribe { device, period_ms: 20 } if device == "imu")));
    assert!(!lemnosd.requests().iter().any(|request| matches!(request, Request::Subscribe { device, .. } if device == "status-ring")), "lights have no channels");
    assert!(notified.load(Ordering::SeqCst) > 0);

    // A timed override: set the duty, then release when it runs out.
    let rt = runtime();
    let request = FanOverrideRequest { duty: 0.5, duration: Duration::from_millis(200) };
    assert_eq!(rt.block_on(bridge.fan_override("fan", request)), Ok(0.5));
    assert!(lemnosd.requests().iter().any(|request| matches!(request, Request::Set { device, control, value, .. } if device == "fan" && control == "duty" && *value == 0.5)));
    assert!(bridge.state().overrides.contains_key("fan"));
    assert_eq!(std::fs::read_to_string(&marker).expect("marker"), "fan\n");
    wait_until("the release after the override", || lemnosd.count(is_release) == 1);
    wait_until("the override to clear", || bridge.state().overrides.is_empty());
    assert!(!marker.exists());

    // An explicit release.
    rt.block_on(bridge.fan_override("fan", FanOverrideRequest { duty: 1.0, duration: Duration::from_secs(60) })).expect("override");
    assert_eq!(rt.block_on(bridge.fan_release("fan")), Ok(()));
    assert_eq!(lemnosd.count(is_release), 2);
    assert!(bridge.state().overrides.is_empty());

    // lemnosd restarts during an override: devices go offline, then the status is re-sent and the
    // owed release goes out on the new connection.
    rt.block_on(bridge.fan_override("fan", FanOverrideRequest { duty: 1.0, duration: Duration::from_secs(60) })).expect("override");
    bridge.set_status(LedStatus::Ok);
    wait_until("the Ok status", || lemnosd.count(|request| is_status(request, LedStatus::Ok)) >= 1);
    let hellos = lemnosd.count(|request| matches!(request, Request::Hello { .. }));
    lemnosd.kick();
    wait_until("the reconnection", || lemnosd.count(|request| matches!(request, Request::Hello { .. })) > hellos && bridge.state().connected);
    wait_until("the owed release", || lemnosd.count(is_release) == 3);
    wait_until("the status after reconnecting", || lemnosd.count(|request| is_status(request, LedStatus::Ok)) >= 2);
    assert!(bridge.state().overrides.is_empty(), "a reconnect does not resume the override");

    // Another client writing the fan ends the override without a release.
    rt.block_on(bridge.fan_override("fan", FanOverrideRequest { duty: 0.9, duration: Duration::from_secs(60) })).expect("override");
    lemnosd.broadcast(&Message::Event(Event::Control { device: "fan".into(), control: "duty".into(), value: 0.3, by: "board-selftest".into() }));
    wait_until("the takeover", || bridge.state().overrides.is_empty());
    assert_eq!(lemnosd.count(is_release), 3);

    // Stopping during an override releases the fan.
    rt.block_on(bridge.fan_override("fan", FanOverrideRequest { duty: 0.9, duration: Duration::from_secs(60) })).expect("override");
    bridge.stop();
    assert_eq!(lemnosd.count(is_release), 4);
    assert!(!marker.exists());
}

#[test]
fn a_crashed_run_leaves_a_marker_and_the_next_run_releases_the_fan() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("lemnosd.sock");
    let marker = dir.path().join("fan-override");
    std::fs::write(&marker, "fan\n").expect("marker");
    let lemnosd = FakeLemnosd::serve(&socket);
    let (bridge, _) = bridge(socket, marker.clone());
    wait_until("the release left from the crashed run", || lemnosd.count(is_release) == 1);
    wait_until("the marker to go", || !marker.exists());
    drop(bridge);
    assert_eq!(lemnosd.count(is_release), 1, "nothing else to release on stop");
}

#[test]
fn requests_fail_cleanly_without_lemnosd() {
    let dir = tempfile::tempdir().expect("tempdir");
    let not_started = LemnosdBridge::new(LemnosdOptions { socket: dir.path().join("none.sock"), client: "helios".into(), reading_interval: Duration::from_millis(20), fan_marker: None });
    let rt = runtime();
    assert!(rt.block_on(not_started.fan_release("fan")).is_err());

    let (bridge, _) = bridge(dir.path().join("missing.sock"), dir.path().join("fan-override"));
    let error = rt.block_on(bridge.fan_override("fan", FanOverrideRequest { duty: 0.5, duration: Duration::from_secs(1) })).expect_err("no lemnosd");
    assert!(error.contains("not connected"), "{error}");
    assert!(!bridge.state().connected);
    assert!(bridge.state().devices.is_empty());
}
