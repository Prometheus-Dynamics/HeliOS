//! Workloads without a camera binding: ticked on the engine's execution interval when their
//! bound resources change (or every interval when they have no bindings).

use std::{collections::BTreeMap, time::Instant};

use daedalus::runtime::RuntimeSink;

use super::{
    ExecutionError,
    bindings::ResourceInput,
    graph::CompiledWorkloadGraph,
    outputs::{output_artifact, payload_message, session_id_for, telemetry_artifact},
};
use crate::model::{ExecutionArtifactRecord, ExecutionSessionState, ExecutionSessionStatus, ExecutionWorkload};

pub(crate) struct PolledExecution {
    pub workload: ExecutionWorkload,
    graph: CompiledWorkloadGraph,
    output_sinks: Vec<RuntimeSink>,
    last_input_revisions: BTreeMap<String, String>,
    tick_count: u64,
}

impl PolledExecution {
    pub fn new(workload: ExecutionWorkload, graph: CompiledWorkloadGraph) -> Self {
        let output_sinks = graph.output_ports.iter().map(|port| RuntimeSink::node_id("io.host_bridge").port(port.clone())).collect();
        Self { workload, graph, output_sinks, last_input_revisions: BTreeMap::new(), tick_count: 0 }
    }

    pub fn tick(&mut self, inputs: Vec<ResourceInput>, observed_at_ms: u64) -> Result<(ExecutionSessionState, Vec<ExecutionArtifactRecord>), ExecutionError> {
        let started_at = Instant::now();
        let inputs_changed = inputs.iter().any(|input| self.last_input_revisions.get(input.input.as_str()) != Some(&input.revision));
        if !inputs.is_empty() && !inputs_changed {
            let message = serde_json::json!({
                "status": "idle",
                "graph_resident": true,
                "skipped_unchanged_inputs": true,
                "tick_count": self.tick_count,
                "timings_ms": { "planning_cached": self.graph.planning_ms, "execution": 0.0, "total_engine": started_at.elapsed().as_secs_f64() * 1000.0 },
            });
            return Ok((self.session(observed_at_ms, message), Vec::new()));
        }

        let binding_started_at = Instant::now();
        let revisions = inputs.iter().map(|input| (input.input.clone(), input.revision.clone())).collect::<BTreeMap<_, _>>();
        // Every bound input is pushed on each run so multi-input nodes always see a full set.
        for input in inputs {
            self.graph.host_graph.push(input.input, input.payload);
        }
        let binding_ms = binding_started_at.elapsed().as_secs_f64() * 1000.0;

        let execute_started_at = Instant::now();
        let host_graph = &mut self.graph.host_graph;
        let telemetry = if !self.workload.bindings.is_empty() && !self.output_sinks.is_empty() {
            Some(host_graph.tick_selected(self.output_sinks.clone()).map_err(|error| ExecutionError::Execute(error.to_string()))?)
        } else if !self.workload.bindings.is_empty() {
            host_graph.tick_until_idle().map_err(|error| ExecutionError::Execute(error.to_string()))?
        } else {
            Some(host_graph.run_executor_once().map_err(|error| ExecutionError::Execute(error.to_string()))?.telemetry)
        };
        let execution_ms = execute_started_at.elapsed().as_secs_f64() * 1000.0;
        let node_metrics_count = telemetry.as_ref().map(|telemetry| telemetry.node_metrics.len()).unwrap_or_default();
        self.last_input_revisions = revisions;
        self.tick_count = self.tick_count.saturating_add(1);

        let mut artifacts = Vec::new();
        for port in &self.graph.output_ports {
            for payload in self.graph.host_graph.drain_payloads(port) {
                let message = payload_message(&payload, self.graph.host_graph.value_serializers());
                artifacts.push(output_artifact(&self.workload, port, message, observed_at_ms));
            }
        }
        let message = serde_json::json!({
            "status": "running",
            "graph_resident": true,
            "tick_count": self.tick_count,
            "node_metrics_count": node_metrics_count,
            "timings_ms": {
                "planning_cached": self.graph.planning_ms,
                "binding_injection": binding_ms,
                "execution": execution_ms,
                "total_engine": started_at.elapsed().as_secs_f64() * 1000.0
            },
        });
        artifacts.push(telemetry_artifact(&self.workload, message.to_string(), observed_at_ms));
        Ok((self.session(observed_at_ms, message), artifacts))
    }

    fn session(&self, observed_at_ms: u64, message: serde_json::Value) -> ExecutionSessionState {
        ExecutionSessionState {
            workload_id: self.workload.workload_id.clone(),
            session_id: session_id_for(&self.workload),
            status: ExecutionSessionStatus::Running,
            observed_at_ms,
            graph_ref: self.workload.graph_ref.clone(),
            bindings: self.workload.bindings.clone(),
            plugin_requirements: self.workload.plugin_requirements.clone(),
            message: Some(message.to_string()),
        }
    }
}
