pub use orion::core::{NodeId, ResourceId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ResourceKind {
    /// A camera, served through a Styx `CameraService`.
    CaptureDevice,
    /// A device of lemnosd's board definition (sensor, fan, light, GPIO line).
    LemnosDevice,
    Virtual,
}

impl ResourceKind {
    pub const fn id_kind(self) -> &'static str {
        match self {
            Self::CaptureDevice => "capture_device",
            Self::LemnosDevice => "lemnos_device",
            Self::Virtual => "virtual",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceStatus {
    Available,
    Degraded,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceCapability {
    pub name: Box<str>,
    pub detail: Option<Box<str>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceEndpoint {
    pub protocol: Box<str>,
    pub address: Box<str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLink {
    pub target: ResourceId,
    pub relation: Box<str>,
}

/// An observed value published as resource state (a lemnosd reading, the fan override).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ObservedValue {
    Bool(bool),
    UInt(u64),
    F64(f64),
    String(String),
}

/// What a resource reports right now, published as its Orion resource state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceObservation {
    pub observed_at_ms: u64,
    pub values: BTreeMap<String, ObservedValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceDescriptor {
    pub id: ResourceId,
    pub owner: NodeId,
    pub kind: ResourceKind,
    pub display_name: Box<str>,
    pub status: ResourceStatus,
    pub capabilities: Vec<ResourceCapability>,
    pub labels: BTreeMap<String, Box<str>>,
    pub endpoints: Vec<ResourceEndpoint>,
    pub links: Vec<ResourceLink>,
    pub observation: Option<ResourceObservation>,
}

impl ResourceDescriptor {
    pub fn from_parts(owner: NodeId, kind: ResourceKind, local: impl AsRef<str>, display_name: impl Into<String>) -> Result<Self, orion::core::OrionError> {
        Ok(Self {
            id: ResourceId::try_new(format!("{}_{}_{}", kind.id_kind(), owner.as_str(), local.as_ref()))?,
            owner,
            kind,
            display_name: display_name.into().into_boxed_str(),
            status: ResourceStatus::Available,
            capabilities: Vec::new(),
            labels: BTreeMap::new(),
            endpoints: Vec::new(),
            links: Vec::new(),
            observation: None,
        })
    }

    pub fn add_capability(&mut self, name: impl Into<String>, detail: Option<String>) {
        self.capabilities.push(ResourceCapability { name: name.into().into_boxed_str(), detail: detail.map(String::into_boxed_str) });
    }

    pub fn has_capability(&self, name: &str) -> bool {
        self.capabilities.iter().any(|capability| capability.name.as_ref() == name)
    }

    pub fn set_label(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.labels.insert(key.into(), value.into().into_boxed_str());
    }

    pub fn label(&self, key: &str) -> Option<&str> {
        self.labels.get(key).map(Box::as_ref)
    }

    pub fn add_endpoint(&mut self, protocol: impl Into<String>, address: impl Into<String>) {
        self.endpoints.push(ResourceEndpoint { protocol: protocol.into().into_boxed_str(), address: address.into().into_boxed_str() });
    }

    pub fn endpoint(&self, protocol: &str) -> Option<&str> {
        self.endpoints.iter().find(|endpoint| endpoint.protocol.as_ref() == protocol).map(|endpoint| endpoint.address.as_ref())
    }

    pub fn add_link(&mut self, target: ResourceId, relation: impl Into<String>) {
        self.links.push(ResourceLink { target, relation: relation.into().into_boxed_str() });
    }
}

/// The result of a resource action, published as the resource's `action_result`.
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceActionOutcome {
    pub resource_id: ResourceId,
    pub action_kind: Box<str>,
    /// The value lemnosd applied (a fan duty, a control value), if any.
    pub value: Option<f64>,
}

impl ResourceActionOutcome {
    pub fn applied(resource_id: ResourceId, action_kind: impl Into<String>, value: Option<f64>) -> Self {
        Self { resource_id, action_kind: action_kind.into().into_boxed_str(), value }
    }
}
