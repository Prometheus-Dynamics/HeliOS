//! What an export reads from a recording besides its frames: the metadata records, the attachments
//! and the host channels (`/helios/outputs/<port>`, and `/styx/clock` for the clock relation).

use std::collections::{BTreeMap, HashMap};

use mcap::{read::ChunkFlattener, records::Record};
use serde_json::Value;

use crate::{error::RecorderError, host::TOPIC_OUTPUT_PREFIX};

/// The clock samples Styx writes on `/styx/clock` (`styx/msg/ClockSample`).
pub const CLOCK_TOPIC: &str = "/styx/clock";

/// One pipeline output as recorded: when the recorder took it (`CLOCK_BOOTTIME`), its port and value.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputSample {
    pub log_time_ns: u64,
    pub port: String,
    pub value: Value,
}

/// Monotonic and boot time read together (`CLOCK_MONOTONIC`, `CLOCK_BOOTTIME`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockSample {
    pub monotonic_ns: u64,
    pub boottime_ns: u64,
}

/// The host facts of a recording, read whole.
#[derive(Debug, Default)]
pub struct Facts {
    /// Metadata records by name (`helios.session`, `helios.camera`, ...), key and value as text.
    pub metadata: BTreeMap<String, BTreeMap<String, String>>,
    /// Attachments by name (`helios.field`, ...).
    pub attachments: BTreeMap<String, Vec<u8>>,
    /// Sorted by time.
    pub outputs: Vec<OutputSample>,
    /// In file order.
    pub clock: Vec<ClockSample>,
}

/// Reads the facts of an MCAP recording held in `bytes`.
pub fn read(bytes: &[u8]) -> Result<Facts, RecorderError> {
    let mut facts = Facts::default();
    let mut topics: HashMap<u16, String> = HashMap::new();
    let flattened = ChunkFlattener::new(bytes).map_err(|error| RecorderError::Mcap(error.to_string()))?;
    for record in flattened {
        match record.map_err(|error| RecorderError::Mcap(error.to_string()))? {
            Record::Channel(channel) => {
                topics.insert(channel.id, channel.topic.clone());
            }
            Record::Message { header, data } => {
                let topic = topics.get(&header.channel_id).map(String::as_str).unwrap_or_default();
                if topic == CLOCK_TOPIC {
                    facts.clock.extend(clock_sample(&data));
                } else if let Some(port) = topic.strip_prefix(TOPIC_OUTPUT_PREFIX)
                    && let Ok(message) = serde_json::from_slice::<Value>(&data)
                {
                    facts.outputs.push(OutputSample { log_time_ns: header.log_time, port: port.to_string(), value: message["value"].clone() });
                }
            }
            Record::Metadata(metadata) => {
                facts.metadata.insert(metadata.name, metadata.metadata);
            }
            Record::Attachment { header, data, .. } => {
                facts.attachments.insert(header.name, data.into_owned());
            }
            _ => {}
        }
    }
    facts.outputs.sort_by_key(|output| output.log_time_ns);
    Ok(facts)
}

/// A `ClockSample` message: a little-endian CDR encapsulation header, then `uint64 monotonic_ns`,
/// `uint64 boottime_ns` (8-byte aligned from the header's end).
pub fn clock_sample(data: &[u8]) -> Option<ClockSample> {
    if data.len() < 20 || data[..4] != [0x00, 0x01, 0x00, 0x00] {
        return None;
    }
    let word = |at: usize| u64::from_le_bytes(data[at..at + 8].try_into().unwrap_or([0; 8]));
    Some(ClockSample { monotonic_ns: word(4), boottime_ns: word(12) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clock_sample_is_read_from_its_cdr_message() {
        let mut message = vec![0x00, 0x01, 0x00, 0x00];
        message.extend(1_000u64.to_le_bytes());
        message.extend(3_500u64.to_le_bytes());
        message.extend(9u64.to_le_bytes());
        message.extend(7u32.to_le_bytes());
        assert_eq!(clock_sample(&message), Some(ClockSample { monotonic_ns: 1_000, boottime_ns: 3_500 }));
        assert_eq!(clock_sample(&message[..10]), None, "too short");
        assert_eq!(clock_sample(&[0x00, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), None, "big-endian is not read");
    }
}
