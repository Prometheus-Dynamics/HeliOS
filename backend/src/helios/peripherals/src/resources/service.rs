use crate::model::NodeId;
use crate::resources::{DiscoveryContext, DiscoveryError, DiscoveryProbe, DiscoverySnapshot, ProbeReport};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Default)]
pub struct InventoryRefreshReport {
    pub snapshot: DiscoverySnapshot,
    pub probe_reports: Vec<ProbeReport>,
}

#[derive(Clone)]
pub struct PeripheralInventoryService {
    local_node_id: NodeId,
    probes: Vec<Arc<dyn DiscoveryProbe>>,
    last_snapshot: Arc<RwLock<DiscoverySnapshot>>,
}

impl PeripheralInventoryService {
    pub fn new(local_node_id: NodeId) -> Self {
        Self { local_node_id, probes: Vec::new(), last_snapshot: Arc::new(RwLock::new(DiscoverySnapshot::default())) }
    }

    pub fn register_probe(&mut self, probe: Arc<dyn DiscoveryProbe>) -> &mut Self {
        self.probes.push(probe);
        self
    }

    pub fn probe_names(&self) -> Vec<&'static str> {
        self.probes.iter().map(|probe| probe.name()).collect()
    }

    pub fn refresh_report(&self, observed_at_ms: u64) -> InventoryRefreshReport {
        let context = DiscoveryContext::new(self.local_node_id.clone(), observed_at_ms);
        let mut snapshot = DiscoverySnapshot::default();
        let mut probe_reports = Vec::with_capacity(self.probes.len());

        for probe in &self.probes {
            match probe.discover(&context) {
                Ok(discovered) => {
                    probe_reports.push(ProbeReport::success(probe.name(), discovered.resources.len()));
                    snapshot = snapshot.merge(discovered);
                }
                Err(DiscoveryError::ProbeFailed { message, .. }) => {
                    probe_reports.push(ProbeReport::failure(probe.name(), message));
                }
            }
        }

        *self.last_snapshot.write().expect("inventory snapshot poisoned") = snapshot.clone();
        InventoryRefreshReport { snapshot, probe_reports }
    }

    pub fn snapshot(&self) -> DiscoverySnapshot {
        self.last_snapshot.read().expect("inventory snapshot poisoned").clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::ResourceKind, resources::ResourceBuilder};

    struct StaticProbe {
        name: &'static str,
        resources: Vec<crate::model::ResourceDescriptor>,
    }

    impl DiscoveryProbe for StaticProbe {
        fn name(&self) -> &'static str {
            self.name
        }

        fn discover(&self, _context: &DiscoveryContext) -> Result<DiscoverySnapshot, DiscoveryError> {
            Ok(DiscoverySnapshot::new(self.resources.clone()))
        }
    }

    struct FailingProbe;

    impl DiscoveryProbe for FailingProbe {
        fn name(&self) -> &'static str {
            "failing"
        }

        fn discover(&self, _context: &DiscoveryContext) -> Result<DiscoverySnapshot, DiscoveryError> {
            Err(DiscoveryError::ProbeFailed { probe: self.name().to_string(), message: "hardware missing".into() })
        }
    }

    #[test]
    fn inventory_service_merges_probe_results() {
        let local_node_id = NodeId::new("node1");
        let mut service = PeripheralInventoryService::new(local_node_id.clone());
        let gpio = ResourceBuilder::new(local_node_id.clone(), ResourceKind::GpioLine, "gpio0", "GPIO 0").expect("gpio").capability("power", None::<String>).build();
        let pwm = ResourceBuilder::new(local_node_id, ResourceKind::PwmChannel, "pwm0", "PWM 0").expect("pwm").capability("fan", None::<String>).build();
        service.register_probe(Arc::new(StaticProbe { name: "gpio", resources: vec![gpio] })).register_probe(Arc::new(StaticProbe { name: "pwm", resources: vec![pwm] }));
        let report = service.refresh_report(123);
        assert_eq!(service.probe_names(), vec!["gpio", "pwm"]);
        assert_eq!(report.snapshot.resources.len(), 2);
        assert_eq!(service.snapshot().resources.len(), 2);
    }

    #[test]
    fn inventory_service_reports_probe_failures_without_losing_other_resources() {
        let local_node_id = NodeId::new("node1");
        let mut service = PeripheralInventoryService::new(local_node_id.clone());
        let gpio = ResourceBuilder::new(local_node_id, ResourceKind::GpioLine, "gpio0", "GPIO 0").expect("gpio").build();
        service.register_probe(Arc::new(StaticProbe { name: "gpio", resources: vec![gpio] })).register_probe(Arc::new(FailingProbe));
        let report = service.refresh_report(123);
        assert_eq!(report.snapshot.resources.len(), 1);
        assert!(report.probe_reports.iter().any(|probe| probe.error.is_some()));
    }
}
