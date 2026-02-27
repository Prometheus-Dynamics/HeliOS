use crate::types::{CommandId, Timestamp};
use serde::{Deserialize, Serialize};

use bincode::{Decode, Encode};

/// Command acknowledgement produced by servers once a request has been processed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
pub struct AckEvent {
    pub command_id: CommandId,
    #[bincode(with_serde)]
    pub processed_at: Timestamp,
}

/// Negative acknowledgement returned by servers when a request could not be processed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
pub struct NackEvent {
    pub command_id: CommandId,
    pub reason: String,
    pub retryable: bool,
}

/// Generic control-plane notifications that are shared by all IPC services.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
#[serde(rename_all = "snake_case")]
pub enum ControlEvent {
    Ack(AckEvent),
    Nack(NackEvent),
}
