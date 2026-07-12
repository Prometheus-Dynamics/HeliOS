use crate::model::{SystemUpdateRuntime, UpdateArtifactClass, UpdateWorkload};
use orion::control_plane::{ConfigDecodeError, DesiredState, WorkloadRecord, deserialize_config};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateWorkloadConfig {
    update: UpdateSection,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateSection {
    version: String,
    artifact_class: ArtifactClassConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ArtifactClassConfig {
    OsImage,
    PayloadUpdate,
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum UpdateWorkloadDecodeError {
    #[error("workload {workload_id} is not assigned to node {node_id}")]
    WrongNode { workload_id: String, node_id: String },
    #[error("workload {workload_id} is not a system update workload")]
    WrongRuntime { workload_id: String },
    #[error("workload {workload_id} is not desired running")]
    NotRunning { workload_id: String },
    #[error("workload {workload_id} is missing config field {field}")]
    MissingField { workload_id: String, field: String },
    #[error("workload {workload_id} has invalid field {field}: {message}")]
    InvalidField { workload_id: String, field: String, message: String },
}

pub fn decode_assigned_workload(record: &WorkloadRecord, node_id: &str) -> Result<UpdateWorkload, UpdateWorkloadDecodeError> {
    let workload_id = record.workload_id.as_str().to_string();
    if record.runtime_type != SystemUpdateRuntime::runtime_type() {
        return Err(UpdateWorkloadDecodeError::WrongRuntime { workload_id });
    }
    if record.desired_state != DesiredState::Running {
        return Err(UpdateWorkloadDecodeError::NotRunning { workload_id });
    }
    let assigned_node_id = record
        .assigned_node_id
        .as_ref()
        .map(|node| node.as_str().to_string())
        .ok_or_else(|| UpdateWorkloadDecodeError::WrongNode { workload_id: record.workload_id.as_str().to_string(), node_id: node_id.to_string() })?;
    if assigned_node_id != node_id {
        return Err(UpdateWorkloadDecodeError::WrongNode { workload_id: record.workload_id.as_str().to_string(), node_id: node_id.to_string() });
    }

    let config = record.config.as_ref().ok_or_else(|| UpdateWorkloadDecodeError::MissingField { workload_id: workload_id.clone(), field: "update.version".into() })?;
    let decoded: UpdateWorkloadConfig = deserialize_config(&config.payload).map_err(|error| map_config_decode_error(record, error))?;
    let version = decoded.update.version;
    let artifact_class = decode_artifact_class(decoded.update.artifact_class);

    Ok(UpdateWorkload { workload_id: record.workload_id.as_str().to_string(), artifact_id: record.artifact_id.as_str().to_string(), assigned_node_id, version, artifact_class })
}

fn decode_artifact_class(value: ArtifactClassConfig) -> UpdateArtifactClass {
    match value {
        ArtifactClassConfig::OsImage => UpdateArtifactClass::OsImage,
        ArtifactClassConfig::PayloadUpdate => UpdateArtifactClass::PayloadUpdate,
    }
}

fn map_config_decode_error(record: &WorkloadRecord, error: ConfigDecodeError) -> UpdateWorkloadDecodeError {
    let workload_id = record.workload_id.as_str().to_string();
    match error {
        ConfigDecodeError::MissingField { field, .. } => UpdateWorkloadDecodeError::MissingField { workload_id, field },
        ConfigDecodeError::InvalidType { field, expected, actual } => {
            let message = format!("config field '{field}' expected {expected}, got {actual}");
            UpdateWorkloadDecodeError::InvalidField { workload_id, field, message }
        }
        ConfigDecodeError::InvalidValue { field, message } | ConfigDecodeError::InvalidStructure { field, message } | ConfigDecodeError::Deserialize { field, message } => {
            UpdateWorkloadDecodeError::InvalidField { workload_id, field, message: format!("config decode failed: {message}") }
        }
    }
}

#[cfg(test)]
mod tests {
    use orion::{
        control_plane::{TypedConfigValue, WorkloadConfig, WorkloadRecord},
        core::{ArtifactId, WorkloadId},
    };

    use super::*;

    #[test]
    fn decode_assigned_update_workload() {
        let record = WorkloadRecord::builder(WorkloadId::new("update.node-local.1"), SystemUpdateRuntime::runtime_type(), ArtifactId::new("artifact.os.v2026.2.0"))
            .desired_state(DesiredState::Running)
            .assigned_to("node-local")
            .config(
                WorkloadConfig::new("schema.update").field("update.version", TypedConfigValue::String("2026.2.0".into())).field("update.artifact_class", TypedConfigValue::String("os-image".into())),
            )
            .build();

        let decoded = decode_assigned_workload(&record, "node-local").expect("decode");
        assert_eq!(decoded.version, "2026.2.0");
        assert_eq!(decoded.artifact_class, UpdateArtifactClass::OsImage);
    }

    #[test]
    fn decode_assigned_update_workload_rejects_unknown_artifact_class() {
        let record = WorkloadRecord::builder(WorkloadId::new("update.node-local.2"), SystemUpdateRuntime::runtime_type(), ArtifactId::new("artifact.os.v2026.2.1"))
            .desired_state(DesiredState::Running)
            .assigned_to("node-local")
            .config(WorkloadConfig::new("schema.update").field("update.version", TypedConfigValue::String("2026.2.1".into())).field("update.artifact_class", TypedConfigValue::String("delta".into())))
            .build();

        let error = decode_assigned_workload(&record, "node-local").expect_err("unknown artifact class should fail");
        assert!(matches!(
            error,
            UpdateWorkloadDecodeError::InvalidField { ref field, ref message, .. }
                if field == "update.artifact_class" && message.contains("delta")
        ));
    }

    #[test]
    fn decode_assigned_update_workload_rejects_unknown_fields() {
        let record = WorkloadRecord::builder(WorkloadId::new("update.node-local.3"), SystemUpdateRuntime::runtime_type(), ArtifactId::new("artifact.os.v2026.2.2"))
            .desired_state(DesiredState::Running)
            .assigned_to("node-local")
            .config(
                WorkloadConfig::new("schema.update")
                    .field("update.version", TypedConfigValue::String("2026.2.2".into()))
                    .field("update.artifact_class", TypedConfigValue::String("os-image".into()))
                    .field("update.extra", TypedConfigValue::String("bad".into())),
            )
            .build();

        let error = decode_assigned_workload(&record, "node-local").expect_err("unknown fields should fail");
        assert!(matches!(
            error,
            UpdateWorkloadDecodeError::InvalidField { ref field, ref message, .. }
                if field == "update.extra" && message.contains("unknown field")
        ));
    }
}
