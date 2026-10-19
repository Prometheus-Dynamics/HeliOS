//! The host's own channels in a recording: IMU, magnetometer and power samples from Lemnos,
//! pipeline outputs and events from helios-api. Each is a JSON message on its topic.
//!
//! Log time is `CLOCK_BOOTTIME` in nanoseconds: Lemnos stamps its samples on that clock (the
//! sample's `t_boottime_us` is in the JSON), and frames carry the capture clock (`CLOCK_MONOTONIC`,
//! the same clock until the board suspends); `/styx/clock` records relate the two. Outputs and
//! events are stamped when the recorder receives them (their `observed_at_ms` is wall-clock time).

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
};

use serde_json::{Value, json};

pub const TOPIC_IMU: &str = "/helios/imu";
pub const TOPIC_MAG: &str = "/helios/mag";
pub const TOPIC_POWER: &str = "/helios/power";
pub const TOPIC_EVENTS: &str = "/helios/events";
/// `/helios/outputs/<port>`.
pub const TOPIC_OUTPUT_PREFIX: &str = "/helios/outputs/";
/// Messages waiting between the senders and the recording thread; more are dropped and counted.
const CHANNEL_CAPACITY: usize = 4096;

/// Nanoseconds on `CLOCK_BOOTTIME`.
pub fn boottime_ns() -> u64 {
    nix::time::clock_gettime(nix::time::ClockId::CLOCK_BOOTTIME).map(|t| (t.tv_sec() as u64) * 1_000_000_000 + t.tv_nsec() as u64).unwrap_or_default()
}

/// A message for a host channel, stamped with the time the recorder took it.
#[derive(Debug, Clone, PartialEq)]
pub struct HostMessage {
    pub topic: String,
    pub log_time_ns: u64,
    pub data: Vec<u8>,
}

/// Hands host messages to the recording thread of the active recording. Cheap to clone; sends
/// never block (a full queue drops the message and counts it).
#[derive(Clone)]
pub struct HostSender {
    tx: SyncSender<HostMessage>,
    dropped: Arc<AtomicU64>,
}

impl HostSender {
    /// A sender and the receiver the recording thread drains.
    pub fn channel() -> (Self, mpsc::Receiver<HostMessage>, Arc<AtomicU64>) {
        let (tx, rx) = mpsc::sync_channel(CHANNEL_CAPACITY);
        let dropped = Arc::new(AtomicU64::new(0));
        (Self { tx, dropped: dropped.clone() }, rx, dropped)
    }

    /// Records `data` on `topic`, stamped now.
    pub fn send(&self, topic: &str, data: Vec<u8>) {
        self.send_at(topic, boottime_ns(), data);
    }

    /// Records `data` on `topic` at `log_time_ns` (`CLOCK_BOOTTIME`).
    pub fn send_at(&self, topic: &str, log_time_ns: u64, data: Vec<u8>) {
        let message = HostMessage { topic: topic.to_string(), log_time_ns, data };
        if let Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) = self.tx.try_send(message) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Metadata of a host channel: where its messages come from and their clock.
pub fn channel_metadata(topic: &str) -> BTreeMap<String, String> {
    let (source, clock_note) = if topic.starts_with(TOPIC_OUTPUT_PREFIX) || topic == TOPIC_EVENTS {
        ("helios-api", "log_time is when the recorder took the message")
    } else {
        ("lemnosd", "log_time is the sample's t_boottime_us")
    };
    BTreeMap::from([("source".into(), source.into()), ("clock".into(), "CLOCK_BOOTTIME".into()), ("log_time".into(), clock_note.into())])
}

/// A device sample: `{"device", "t_boottime_us", "values": {channel: value or null}}`.
pub fn sample_json(device: &str, timestamp_us: u64, values: impl Iterator<Item = (String, Option<f64>)>) -> Vec<u8> {
    let values: serde_json::Map<String, Value> = values.map(|(name, value)| (name, value.map_or(Value::Null, |v| json!(v)))).collect();
    json!({ "device": device, "t_boottime_us": timestamp_us, "values": values }).to_string().into_bytes()
}

/// A pipeline output: `{"pipeline", "port", "value", "observed_at_ms"}`.
pub fn output_json(pipeline: &str, port: &str, value: &Value, observed_at_ms: u64) -> Vec<u8> {
    json!({ "pipeline": pipeline, "port": port, "value": value, "observed_at_ms": observed_at_ms }).to_string().into_bytes()
}

/// An event of the API's stream: `{"type", "at_ms", "data"}`.
pub fn event_json(kind: &str, at_ms: u64, data: &Value) -> Vec<u8> {
    json!({ "type": kind, "at_ms": at_ms, "data": data }).to_string().into_bytes()
}

/// The topic of pipeline output `port`.
pub fn output_topic(port: &str) -> String {
    format!("{TOPIC_OUTPUT_PREFIX}{port}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_and_outputs_are_json_with_their_clock() {
        let sample: Value = serde_json::from_slice(&sample_json("imu", 42, [("acceleration.x".to_string(), Some(0.5)), ("gyro".to_string(), None)].into_iter())).expect("json");
        assert_eq!(sample["t_boottime_us"], 42);
        assert_eq!(sample["values"]["acceleration.x"], 0.5);
        assert!(sample["values"]["gyro"].is_null());
        let output: Value = serde_json::from_slice(&output_json("p", "tags", &json!([1]), 7)).expect("json");
        assert_eq!((output["pipeline"].as_str(), output["port"].as_str()), (Some("p"), Some("tags")));
        assert_eq!(output_topic("tags"), "/helios/outputs/tags");
        assert_eq!(channel_metadata(TOPIC_IMU)["source"], "lemnosd");
        assert_eq!(channel_metadata(TOPIC_EVENTS)["source"], "helios-api");
    }

    #[test]
    fn a_full_channel_drops_and_counts() {
        let (sender, rx, dropped) = HostSender::channel();
        for _ in 0..(CHANNEL_CAPACITY + 3) {
            sender.send_at(TOPIC_IMU, 1, vec![0]);
        }
        assert_eq!(dropped.load(Ordering::Relaxed), 3);
        assert_eq!(rx.try_iter().count(), CHANNEL_CAPACITY);
    }
}
