//! Graph host outputs and engine stats as Orion artifact records.

use daedalus::{
    data::{json::to_plain_json, model::Value},
    runtime::host_bridge::{ValueSerializerMap, inspect_payload},
    transport::Payload,
};
use styx::imports::framelease::FrameLease;

use crate::{
    model::{ExecutionArtifactRecord, ExecutionWorkload},
    stream_io,
};

/// Artifact kind of a graph host output (`host_output:<port>`).
pub(crate) fn host_output_kind(port: &str) -> String {
    format!("host_output:{port}")
}

pub(crate) const TELEMETRY_ARTIFACT_KIND: &str = "execution.telemetry";

/// Render a host output payload as the JSON published in Orion.
///
/// Structured values go through the registry's value serializers (`ToValue`) and come out as
/// plain JSON objects; bare scalars are wrapped as `{"value": ...}`; strings are passed through
/// as-is (nodes that emit JSON text). Frames are described, never copied. Types without a
/// serializer are described by their type key, Rust type and residency.
pub(crate) fn payload_message(payload: &Payload, serializers: &ValueSerializerMap) -> String {
    if let Some(frame) = payload.get_ref::<FrameLease>() {
        return stream_io::framelease_descriptor_json(frame).to_string();
    }
    if let Some(text) = payload.get_ref::<String>() {
        return text.clone();
    }
    let inspection = inspect_payload(payload, serializers);
    match inspection.value() {
        Some(Value::String(text)) => text.to_string(),
        Some(value) => match to_plain_json(value) {
            object @ serde_json::Value::Object(_) => object.to_string(),
            other => serde_json::json!({ "value": other }).to_string(),
        },
        None => inspection.to_json().to_string(),
    }
}

pub(crate) fn output_artifact(workload: &ExecutionWorkload, port: &str, message: String, observed_at_ms: u64) -> ExecutionArtifactRecord {
    ExecutionArtifactRecord {
        workload_id: workload.workload_id.clone(),
        session_id: session_id_for(workload),
        artifact_id: format!("{}.{}", session_id_for(workload), sanitize_id_component(port)),
        kind: host_output_kind(port),
        observed_at_ms,
        message: Some(message),
        endpoints: Vec::new(),
    }
}

pub(crate) fn telemetry_artifact(workload: &ExecutionWorkload, message: String, observed_at_ms: u64) -> ExecutionArtifactRecord {
    ExecutionArtifactRecord {
        workload_id: workload.workload_id.clone(),
        session_id: session_id_for(workload),
        artifact_id: format!("{}.telemetry", session_id_for(workload)),
        kind: TELEMETRY_ARTIFACT_KIND.into(),
        observed_at_ms,
        message: Some(message),
        endpoints: Vec::new(),
    }
}

pub(crate) fn session_id_for(workload: &ExecutionWorkload) -> String {
    format!("session.{}", workload.workload_id)
}

fn sanitize_id_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => ch,
            _ => '_',
        })
        .collect()
}
