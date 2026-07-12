use crate::model::{NodeId, ResourceDescriptor, ResourceId};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct DiscoveryContext {
    pub local_node_id: NodeId,
    pub observed_at_ms: u64,
}

impl DiscoveryContext {
    pub fn new(local_node_id: NodeId, observed_at_ms: u64) -> Self {
        Self { local_node_id, observed_at_ms }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DiscoverySnapshot {
    pub resources: Vec<ResourceDescriptor>,
}

impl DiscoverySnapshot {
    pub fn new(resources: Vec<ResourceDescriptor>) -> Self {
        Self { resources }
    }

    pub fn get(&self, resource_id: &ResourceId) -> Option<&ResourceDescriptor> {
        self.resources.iter().find(|resource| &resource.id == resource_id)
    }

    pub fn merge(mut self, other: Self) -> Self {
        let mut merged = self.resources.drain(..).map(|resource| (resource.id.clone(), resource)).collect::<BTreeMap<_, _>>();
        for resource in other.resources {
            merged.insert(resource.id.clone(), resource);
        }
        Self { resources: merged.into_values().collect() }
    }
}

pub trait DiscoveryProbe: Send + Sync {
    fn name(&self) -> &'static str;
    fn discover(&self, context: &DiscoveryContext) -> Result<DiscoverySnapshot, DiscoveryError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DiscoveryError {
    #[error("probe '{probe}' failed: {message}")]
    ProbeFailed { probe: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    pub probe: String,
    pub discovered_resources: usize,
    pub error: Option<String>,
}

impl ProbeReport {
    pub fn success(probe: impl Into<String>, discovered_resources: usize) -> Self {
        Self { probe: probe.into(), discovered_resources, error: None }
    }

    pub fn failure(probe: impl Into<String>, error: impl Into<String>) -> Self {
        Self { probe: probe.into(), discovered_resources: 0, error: Some(error.into()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ResourceKind;

    #[test]
    fn merge_prefers_newer_resource_entries() {
        let owner = NodeId::new("node1");
        let mut first = ResourceDescriptor::from_parts(owner.clone(), ResourceKind::GpioLine, "gpio0", "GPIO 0").expect("resource");
        first.add_capability("power", None::<String>);
        let mut second = ResourceDescriptor::from_parts(owner, ResourceKind::GpioLine, "gpio0", "GPIO 0").expect("resource");
        second.add_capability("reset", None::<String>);

        let merged = DiscoverySnapshot::new(vec![first]).merge(DiscoverySnapshot::new(vec![second]));
        assert_eq!(merged.resources.len(), 1);
        assert!(merged.resources[0].capabilities.iter().any(|capability| capability.name.as_ref() == "reset"));
    }
}
