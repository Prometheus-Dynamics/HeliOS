use std::collections::BTreeMap;

use orion::control_plane::{ConfigDecodeError, DesiredState, WorkloadRecord, deserialize_config};
use serde::Deserialize;

use crate::model::{EngineExecutionRuntime, ExecutionBinding, ExecutionWorkload, GraphRef, PluginRequirement};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct EngineWorkloadConfig {
    #[serde(default)]
    graph: GraphConfig,
    #[serde(default)]
    binding: BTreeMap<String, BindingConfig>,
    #[serde(default)]
    plugin: Vec<PluginConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct GraphConfig {
    #[serde(default)]
    kind: GraphKind,
    #[serde(default)]
    artifact_id: Option<String>,
    #[serde(default)]
    resource_id: Option<String>,
    #[serde(default)]
    inline: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
enum GraphKind {
    #[default]
    Artifact,
    Resource,
    Inline,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingConfig {
    resource_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginConfig {
    name: String,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum WorkloadDecodeError {
    #[error("workload {workload_id} is not assigned to node {node_id}")]
    WrongNode { workload_id: String, node_id: String },
    #[error("workload {workload_id} is not an engine execution workload")]
    WrongRuntime { workload_id: String },
    #[error("workload {workload_id} is not desired running")]
    NotRunning { workload_id: String },
    #[error("workload {workload_id} is missing config field {field}")]
    MissingField { workload_id: String, field: String },
    #[error("workload {workload_id} has invalid field {field}: {message}")]
    InvalidField { workload_id: String, field: String, message: String },
}

pub fn decode_assigned_workload(record: &WorkloadRecord, node_id: &str) -> Result<ExecutionWorkload, WorkloadDecodeError> {
    let workload_id = record.workload_id.as_str().to_string();
    if record.runtime_type != EngineExecutionRuntime::runtime_type() {
        return Err(WorkloadDecodeError::WrongRuntime { workload_id });
    }
    if record.desired_state != DesiredState::Running {
        return Err(WorkloadDecodeError::NotRunning { workload_id });
    }
    let assigned_node_id = record
        .assigned_node_id
        .as_ref()
        .map(|node| node.as_str().to_string())
        .ok_or_else(|| WorkloadDecodeError::WrongNode { workload_id: record.workload_id.as_str().to_string(), node_id: node_id.to_string() })?;
    if assigned_node_id != node_id {
        return Err(WorkloadDecodeError::WrongNode { workload_id: record.workload_id.as_str().to_string(), node_id: node_id.to_string() });
    }

    let decoded = match record.config.as_ref() {
        Some(config) => deserialize_engine_config(record, config)?,
        None => EngineWorkloadConfig::default(),
    };

    let graph_ref = decode_graph_ref(record, &decoded)?;
    let bindings = decode_bindings(record, &decoded)?;
    let plugin_requirements = decode_plugin_requirements(&decoded);

    Ok(ExecutionWorkload { workload_id: record.workload_id.as_str().to_string(), artifact_id: record.artifact_id.as_str().to_string(), assigned_node_id, graph_ref, bindings, plugin_requirements })
}

fn decode_graph_ref(record: &WorkloadRecord, decoded: &EngineWorkloadConfig) -> Result<GraphRef, WorkloadDecodeError> {
    match decoded.graph.kind {
        GraphKind::Artifact => Ok(GraphRef::ArtifactId(decoded.graph.artifact_id.clone().unwrap_or_else(|| record.artifact_id.as_str().to_string()))),
        GraphKind::Resource => decoded
            .graph
            .resource_id
            .clone()
            .map(GraphRef::ResourceId)
            .ok_or_else(|| WorkloadDecodeError::MissingField { workload_id: record.workload_id.as_str().to_string(), field: "graph.resource_id".into() }),
        GraphKind::Inline => decoded
            .graph
            .inline
            .clone()
            .map(GraphRef::InlineSpec)
            .ok_or_else(|| WorkloadDecodeError::MissingField { workload_id: record.workload_id.as_str().to_string(), field: "graph.inline".into() }),
    }
}

fn decode_bindings(record: &WorkloadRecord, decoded: &EngineWorkloadConfig) -> Result<Vec<ExecutionBinding>, WorkloadDecodeError> {
    let bound_nodes = record.resource_bindings.iter().map(|binding| (binding.resource_id.as_str().to_string(), binding.node_id.as_str().to_string())).collect::<BTreeMap<_, _>>();

    let mut bindings = Vec::new();
    for (input, binding) in &decoded.binding {
        let node_id = bound_nodes.get(&binding.resource_id).cloned().unwrap_or_else(|| record.assigned_node_id.as_ref().map(|node| node.as_str().to_string()).unwrap_or_default());
        bindings.push(ExecutionBinding { input: input.clone(), resource_id: binding.resource_id.clone(), node_id });
    }

    if bindings.is_empty() {
        bindings.extend(record.resource_bindings.iter().map(|binding| ExecutionBinding {
            input: binding.resource_id.as_str().to_string(),
            resource_id: binding.resource_id.as_str().to_string(),
            node_id: binding.node_id.as_str().to_string(),
        }));
    }

    Ok(bindings)
}

fn decode_plugin_requirements(decoded: &EngineWorkloadConfig) -> Vec<PluginRequirement> {
    decoded.plugin.iter().filter(|plugin| !plugin.name.is_empty()).map(|plugin| PluginRequirement { plugin_name: plugin.name.clone(), version: plugin.version.clone() }).collect()
}

fn deserialize_engine_config(record: &WorkloadRecord, config: &orion::control_plane::WorkloadConfig) -> Result<EngineWorkloadConfig, WorkloadDecodeError> {
    deserialize_config(&config.payload).map_err(|error| map_config_decode_error(record, error))
}

fn map_config_decode_error(record: &WorkloadRecord, error: ConfigDecodeError) -> WorkloadDecodeError {
    let workload_id = record.workload_id.as_str().to_string();
    match error {
        ConfigDecodeError::MissingField { field, .. } => WorkloadDecodeError::MissingField { workload_id, field },
        ConfigDecodeError::InvalidType { field, expected, actual } => {
            let message = format!("config field '{field}' expected {expected}, got {actual}");
            WorkloadDecodeError::InvalidField { workload_id, field, message }
        }
        ConfigDecodeError::InvalidValue { field, message } | ConfigDecodeError::InvalidStructure { field, message } | ConfigDecodeError::Deserialize { field, message } => {
            WorkloadDecodeError::InvalidField { workload_id, field, message: format!("config decode failed: {message}") }
        }
    }
}

#[cfg(test)]
mod tests {
    use orion::{
        control_plane::{DesiredState, TypedConfigValue, WorkloadConfig, WorkloadRecord},
        core::{ArtifactId, WorkloadId},
    };

    use super::*;

    const NODE_ID: &str = "node-a";

    #[test]
    fn decode_assigned_workload_uses_artifact_default_and_plugin_requirements() {
        let record = WorkloadRecord::builder(WorkloadId::new("workload.graph"), EngineExecutionRuntime::runtime_type(), ArtifactId::new("artifact.graph"))
            .desired_state(DesiredState::Running)
            .assigned_to(NODE_ID)
            .bind_resource("camera.front.left", NODE_ID)
            .config(
                WorkloadConfig::new("schema.exec")
                    .field("plugin.0.name", TypedConfigValue::String("cv".into()))
                    .field("plugin.0.version", TypedConfigValue::String("1.2.3".into()))
                    .field("binding.camera.resource_id", TypedConfigValue::String("camera.front.left".into())),
            )
            .build();

        let decoded = decode_assigned_workload(&record, NODE_ID).expect("decode");
        assert_eq!(decoded.graph_ref, GraphRef::ArtifactId("artifact.graph".into()));
        assert_eq!(decoded.bindings[0].input, "camera");
        assert_eq!(decoded.plugin_requirements[0].plugin_name, "cv");
        assert_eq!(decoded.plugin_requirements[0].version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn decode_assigned_workload_uses_named_binding_groups() {
        let record = WorkloadRecord::builder(WorkloadId::new("workload.binding"), EngineExecutionRuntime::runtime_type(), ArtifactId::new("artifact.binding"))
            .desired_state(DesiredState::Running)
            .assigned_to(NODE_ID)
            .bind_resource("resource.camera", NODE_ID)
            .config(
                WorkloadConfig::new("schema.exec")
                    .field("binding.camera.resource_id", TypedConfigValue::String("resource.camera".into()))
                    .field("binding.depth.resource_id", TypedConfigValue::String("resource.depth".into())),
            )
            .build();

        let decoded = decode_assigned_workload(&record, NODE_ID).expect("decode");
        assert_eq!(decoded.bindings.len(), 2);
        assert!(decoded.bindings.iter().any(|binding| binding.input == "camera" && binding.resource_id == "resource.camera" && binding.node_id == NODE_ID));
        assert!(decoded.bindings.iter().any(|binding| binding.input == "depth" && binding.resource_id == "resource.depth" && binding.node_id == NODE_ID));
    }

    #[test]
    fn decode_assigned_workload_rejects_wrong_runtime() {
        let record = WorkloadRecord::builder("workload.graph", "other.runtime", "artifact.graph").desired_state(DesiredState::Running).assigned_to(NODE_ID).build();

        let error = decode_assigned_workload(&record, NODE_ID).expect_err("wrong runtime");
        assert!(matches!(error, WorkloadDecodeError::WrongRuntime { .. }));
    }

    #[test]
    fn decode_assigned_workload_defaults_to_artifact_graph_without_graph_section() {
        let record = WorkloadRecord::builder(WorkloadId::new("workload.default-graph"), EngineExecutionRuntime::runtime_type(), ArtifactId::new("artifact.default"))
            .desired_state(DesiredState::Running)
            .assigned_to(NODE_ID)
            .config(WorkloadConfig::new("schema.exec"))
            .build();

        let decoded = decode_assigned_workload(&record, NODE_ID).expect("decode");
        assert_eq!(decoded.graph_ref, GraphRef::ArtifactId("artifact.default".into()));
    }

    #[test]
    fn decode_assigned_workload_rejects_unknown_fields() {
        let record = WorkloadRecord::builder(WorkloadId::new("workload.strict"), EngineExecutionRuntime::runtime_type(), ArtifactId::new("artifact.strict"))
            .desired_state(DesiredState::Running)
            .assigned_to(NODE_ID)
            .config(WorkloadConfig::new("schema.exec").field("plugin.0.name", TypedConfigValue::String("cv".into())).field("plugin.0.extra", TypedConfigValue::String("bad".into())))
            .build();

        let error = decode_assigned_workload(&record, NODE_ID).expect_err("unknown fields should fail");
        assert!(matches!(
            error,
            WorkloadDecodeError::InvalidField { ref field, ref message, .. }
                if field == "plugin[0].extra" && message.contains("unknown field")
        ));
    }
}
