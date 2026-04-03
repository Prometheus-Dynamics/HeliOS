use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{FeatureSet, ProtocolVersion, Timestamp};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct ClientHello {
    pub protocol: ProtocolVersion,
    pub client_name: String,
    pub client_version: String,
    pub supported_features: FeatureSet,
    pub instance_id: Uuid,
}

impl ClientHello {
    #[must_use]
    pub fn new(protocol: ProtocolVersion, client_name: impl Into<String>, client_version: impl Into<String>, features: FeatureSet) -> Self {
        Self { protocol, client_name: client_name.into(), client_version: client_version.into(), supported_features: features, instance_id: Uuid::new_v4() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct ServerHello {
    pub protocol: ProtocolVersion,
    pub server_name: String,
    pub server_version: String,
    pub session_id: Uuid,
    pub accepted_features: FeatureSet,
    pub server_features: FeatureSet,
    pub snapshot_required: bool,
}

impl ServerHello {
    #[must_use]
    pub fn new(protocol: ProtocolVersion, server_name: impl Into<String>, server_version: impl Into<String>, accepted_features: FeatureSet, server_features: FeatureSet) -> Self {
        Self { protocol, server_name: server_name.into(), server_version: server_version.into(), session_id: Uuid::new_v4(), accepted_features, server_features, snapshot_required: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct HandshakeReject {
    pub protocol: ProtocolVersion,
    pub reason: String,
    #[rkyv(with = crate::archive::with::SerdeBytes)]
    pub retry_after: Option<Timestamp>,
    pub required_protocol: Option<ProtocolVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
pub enum HandshakeResponse {
    Accepted(ServerHello),
    Rejected(HandshakeReject),
}
