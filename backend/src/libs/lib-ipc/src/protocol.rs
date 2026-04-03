use crate::types::{CommandId, Timestamp};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

/// Command acknowledgement produced by servers once a request has been processed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct AckEvent {
    pub command_id: CommandId,
    #[rkyv(with = crate::archive::with::SerdeBytes)]
    pub processed_at: Timestamp,
}

/// Negative acknowledgement returned by servers when a request could not be processed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct NackEvent {
    pub command_id: CommandId,
    pub reason: String,
    pub retryable: bool,
}

/// Generic control-plane notifications that are shared by all IPC services.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlEvent {
    Ack(AckEvent),
    Nack(NackEvent),
}
