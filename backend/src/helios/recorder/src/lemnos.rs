//! IMU, magnetometer and power samples from lemnosd, as host channels. A thread holds a device
//! client (reconnecting), subscribes to the devices of each class at the configured period, and
//! hands every reading to the recording with its `CLOCK_BOOTTIME` stamp.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use lemnos_ipc::{ClientEvent, ClientOptions, DeviceClass, DeviceClient, DeviceDesc, Update};
use tracing::{info, warn};

use crate::host::{self, HostSender};

const POLL: Duration = Duration::from_millis(100);
const RECONNECT: Duration = Duration::from_secs(2);

/// Which lemnosd to read, as which client, and how often to sample each class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LemnosSource {
    pub socket: PathBuf,
    pub client_name: String,
    pub imu_period_ms: u32,
    pub mag_period_ms: u32,
    pub power_period_ms: u32,
}

/// The host channel of a device class (`None`: not recorded).
pub fn topic_for(class: DeviceClass) -> Option<&'static str> {
    match class {
        DeviceClass::Imu | DeviceClass::Accelerometer | DeviceClass::Gyroscope => Some(host::TOPIC_IMU),
        DeviceClass::Magnetometer => Some(host::TOPIC_MAG),
        DeviceClass::PowerMonitor => Some(host::TOPIC_POWER),
        _ => None,
    }
}

/// Runs until `stop` is set, reconnecting when lemnosd goes away.
pub fn spawn(source: LemnosSource, sender: HostSender, stop: Arc<AtomicBool>) -> JoinHandle<()> {
    std::thread::Builder::new().name("helios-recorder-lemnos".into()).spawn(move || run(&source, &sender, &stop)).expect("spawn lemnos reader")
}

fn run(source: &LemnosSource, sender: &HostSender, stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        let options = ClientOptions::new(&source.socket, source.client_name.clone()).reconnecting().wait(RECONNECT);
        match options.devices() {
            Ok(devices) => {
                info!(socket = %source.socket.display(), "recording: connected to lemnosd");
                pump(devices, source, sender, stop);
            }
            Err(error) => warn!(error = %error, "recording: no lemnosd client; trying again"),
        }
        sleep_unless(stop, RECONNECT);
    }
}

/// Reads until the connection is lost or `stop` is set.
fn pump(mut devices: DeviceClient, source: &LemnosSource, sender: &HostSender, stop: &AtomicBool) {
    let mut topics: HashMap<String, &'static str> = HashMap::new();
    while !stop.load(Ordering::Relaxed) {
        match devices.next_event_timeout(POLL) {
            Ok(Some(ClientEvent::Connected { .. })) => subscribe_all(&mut devices, source, &mut topics),
            Ok(Some(ClientEvent::Disconnected { error })) => {
                warn!(error = %error, "recording: lost lemnosd");
                return;
            }
            Ok(Some(ClientEvent::Data(Update::Reading(reading)))) => {
                if let Some(topic) = topics.get(&reading.device) {
                    let values = reading.values().map(|(name, value)| (name.to_string(), value));
                    sender.send_at(topic, reading.timestamp_us.saturating_mul(1000), host::sample_json(&reading.device, reading.timestamp_us, values));
                }
            }
            Ok(Some(ClientEvent::Data(Update::Event(_)))) | Ok(None) => {}
            Err(error) => {
                warn!(error = %error, "recording: lemnosd connection closed");
                return;
            }
        }
    }
}

/// Subscribes to every device with channels in a recorded class, at that class's period.
fn subscribe_all(devices: &mut DeviceClient, source: &LemnosSource, topics: &mut HashMap<String, &'static str>) {
    let list: Vec<DeviceDesc> = devices.list().unwrap_or_else(|error| {
        warn!(error = %error, "recording: listing lemnosd's devices failed");
        devices.devices().cloned().collect()
    });
    topics.clear();
    for device in list.iter().filter(|device| !device.channels.is_empty()) {
        let Some(topic) = topic_for(device.class) else { continue };
        let period = match topic {
            host::TOPIC_IMU => source.imu_period_ms,
            host::TOPIC_MAG => source.mag_period_ms,
            _ => source.power_period_ms,
        };
        match devices.subscribe(&device.id, period) {
            Ok(_) => {
                topics.insert(device.id.clone(), topic);
            }
            Err(error) => warn!(device = %device.id, error = %error, "recording: subscribing to a lemnosd device failed"),
        }
    }
}

fn sleep_unless(stop: &AtomicBool, wait: Duration) {
    let end = std::time::Instant::now() + wait;
    while !stop.load(Ordering::Relaxed) && std::time::Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
}
