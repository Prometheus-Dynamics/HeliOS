//! Pipeline bindings: a graph input fed with another pipeline's host output.
//!
//! A binding names either a resource (`source` absent or `"resource"`: a camera, whose frames
//! pace the graph) or another pipeline's output (`source: "pipeline"`, with `pipeline` the
//! producer's id and `port` its host output port). The consumer takes the producer's latest value
//! of that output, paired with the frame it was made from (helios-engine's
//! `execution::pipeline_inputs`), so a pipeline input always comes with a camera binding.
//!
//! ```json
//! "bindings": {
//!   "frame": { "resource_id": "camera.front", "pyramid": 1 },
//!   "pose":  { "source": "pipeline", "pipeline": "live-map", "port": "pose", "max_wait_ms": 50 }
//! }
//! ```

use orion::control_plane::TypedConfigValue;
use serde::{Deserialize, Serialize};

use super::pipelines::{Binding, PipelineSpec, WORKLOAD_PREFIX};
use crate::{
    error::{ApiError, ApiResult},
    orion::StateView,
};

/// The longest a frame may wait for a producer's value (`max_wait_ms`).
pub const MAX_PIPELINE_INPUT_WAIT_MS: u32 = 1000;

/// Where a graph input's value comes from.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BindingSource {
    /// A resource (a camera, or a resource whose state is fed as JSON).
    #[default]
    Resource,
    /// Another pipeline's host output (`pipeline`, `port`).
    Pipeline,
}

impl BindingSource {
    pub fn is_resource(&self) -> bool {
        *self == Self::Resource
    }
}

/// A template's graph input that takes another pipeline's output: the input, the producer's port,
/// and the template of the producing pipeline (`GET /v1/pipelines/templates` names it, so the UI
/// can offer the pipeline that runs it).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PipelineInputHint {
    pub input: &'static str,
    pub port: &'static str,
    pub producer_template: &'static str,
}

/// The pipeline inputs of the golden graph `template`. None takes one today: the dense mapper
/// runs in the live map's pipeline (all of a camera's Eidos work shares one core), not on a
/// second pipeline fed the live map's pose.
pub fn template_pipeline_inputs(_template: &str) -> Vec<PipelineInputHint> {
    Vec::new()
}

/// Check one binding of a spec. `resource_exists` says whether a resource id is known.
pub fn validate(input: &str, binding: &Binding, view: &StateView) -> ApiResult<()> {
    match binding.source {
        BindingSource::Resource => {
            if binding.pipeline.is_some() || binding.port.is_some() || binding.max_wait_ms.is_some() {
                return Err(ApiError::unprocessable(format!("binding {input}: pipeline, port and max_wait_ms need source \"pipeline\"")));
            }
            if binding.resource_id.is_empty() {
                return Err(ApiError::unprocessable(format!("binding {input} needs a resource_id (or source \"pipeline\")")));
            }
            if !view.resources.contains_key(&binding.resource_id) {
                return Err(ApiError::unprocessable(format!("binding {input} names unknown resource {}", binding.resource_id)));
            }
            Ok(())
        }
        BindingSource::Pipeline => {
            if !binding.resource_id.is_empty() || binding.camera.is_some() || binding.output_width.is_some() || binding.output_height.is_some() || binding.pyramid.is_some() {
                return Err(ApiError::unprocessable(format!("binding {input}: a pipeline input takes pipeline, port and max_wait_ms only")));
            }
            let (Some(pipeline), Some(port)) = (binding.pipeline.as_deref(), binding.port.as_deref()) else {
                return Err(ApiError::unprocessable(format!("binding {input}: source \"pipeline\" needs pipeline and port")));
            };
            if !super::valid_id(pipeline) {
                return Err(ApiError::unprocessable(format!("binding {input}: {pipeline:?} is not a pipeline id")));
            }
            if port.is_empty() || !port.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                return Err(ApiError::unprocessable(format!("binding {input}: {port:?} is not an output port name")));
            }
            if !view.workloads.contains_key(&format!("{WORKLOAD_PREFIX}{pipeline}")) {
                return Err(ApiError::unprocessable(format!("binding {input} takes an output of pipeline {pipeline}, which does not exist")));
            }
            if binding.max_wait_ms.is_some_and(|ms| ms > MAX_PIPELINE_INPUT_WAIT_MS) {
                return Err(ApiError::unprocessable(format!("binding {input}: max_wait_ms is 0 to {MAX_PIPELINE_INPUT_WAIT_MS}")));
            }
            Ok(())
        }
    }
}

/// Whether `spec` takes an output of its own pipeline `id` (a pipeline cannot feed itself).
pub fn check_not_self(id: &str, spec: &PipelineSpec) -> ApiResult<()> {
    match spec.bindings.iter().find(|(_, binding)| binding.source == BindingSource::Pipeline && binding.pipeline.as_deref() == Some(id)) {
        Some((input, _)) => Err(ApiError::unprocessable(format!("binding {input} takes an output of pipeline {id} itself"))),
        None => Ok(()),
    }
}

/// The Orion config fields of a pipeline input `input` (`binding.<input>.pipeline`, `.port`,
/// `.max_wait_ms`), written in place of a resource's.
pub fn config_fields(input: &str, binding: &Binding) -> Vec<(String, TypedConfigValue)> {
    let mut fields = vec![
        (format!("binding.{input}.pipeline"), TypedConfigValue::String(binding.pipeline.clone().unwrap_or_default())),
        (format!("binding.{input}.port"), TypedConfigValue::String(binding.port.clone().unwrap_or_default())),
    ];
    if let Some(ms) = binding.max_wait_ms {
        fields.push((format!("binding.{input}.max_wait_ms"), TypedConfigValue::Int(i64::from(ms))));
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_bindings_round_trip_and_resource_bindings_stay_as_they_were() {
        let pose: Binding = serde_json::from_value(serde_json::json!({ "source": "pipeline", "pipeline": "live-map", "port": "pose" })).expect("pipeline binding");
        assert_eq!(pose.source, BindingSource::Pipeline);
        let json = serde_json::to_value(&pose).expect("json");
        assert_eq!(json, serde_json::json!({ "source": "pipeline", "pipeline": "live-map", "port": "pose" }), "no empty resource fields");

        // A stored resource binding has no `source`, and writes none back.
        let frame: Binding = serde_json::from_value(serde_json::json!({ "resource_id": "camera.front", "pyramid": 1 })).expect("resource binding");
        assert_eq!(frame.source, BindingSource::Resource);
        assert_eq!(serde_json::to_value(&frame).expect("json"), serde_json::json!({ "resource_id": "camera.front", "pyramid": 1 }));
    }

    #[test]
    fn a_pipeline_input_names_a_pipeline_and_a_port_only() {
        let mut view = StateView::default();
        view.workloads.insert("pipeline.live-map".into(), orion::control_plane::WorkloadRecord::builder(orion::core::WorkloadId::new("pipeline.live-map"), "helios.engine.execution.v1", orion::core::ArtifactId::new("a")).build());
        let good = Binding { source: BindingSource::Pipeline, pipeline: Some("live-map".into()), port: Some("pose".into()), ..Binding::default() };
        assert!(validate("pose", &good, &view).is_ok());
        let missing = Binding { source: BindingSource::Pipeline, pipeline: Some("nope".into()), port: Some("pose".into()), ..Binding::default() };
        assert!(validate("pose", &missing, &view).is_err(), "unknown producer");
        let no_port = Binding { source: BindingSource::Pipeline, pipeline: Some("live-map".into()), ..Binding::default() };
        assert!(validate("pose", &no_port, &view).is_err(), "no port");
        let with_camera = Binding { resource_id: "camera.front".into(), ..good.clone() };
        assert!(validate("pose", &with_camera, &view).is_err(), "a resource and a pipeline");
        let slow = Binding { max_wait_ms: Some(MAX_PIPELINE_INPUT_WAIT_MS + 1), ..good.clone() };
        assert!(validate("pose", &slow, &view).is_err(), "wait is bounded");
        let resource_with_port = Binding { resource_id: "camera.front".into(), port: Some("pose".into()), ..Binding::default() };
        assert!(validate("frame", &resource_with_port, &view).is_err(), "pipeline fields need source pipeline");
    }

    #[test]
    fn a_pipeline_cannot_feed_itself() {
        let spec = PipelineSpec {
            name: "loop".into(),
            graph: serde_json::json!({}),
            bindings: std::collections::BTreeMap::from([("pose".to_string(), Binding { source: BindingSource::Pipeline, pipeline: Some("loop".into()), port: Some("pose".into()), ..Binding::default() })]),
            enabled: true,
            search_mode: None,
            full_search_every: None,
            loss_full_search_after: None,
            margin_growth_misses: None,
            field_layout: None,
            dictionary: None,
            cpu: None,
            nice: None,
        };
        assert!(check_not_self("loop", &spec).is_err());
        assert!(check_not_self("other", &spec).is_ok());
    }

    #[test]
    fn the_dense_template_needs_no_other_pipeline() {
        assert!(template_pipeline_inputs("dense-map").is_empty(), "the dense mapper is in the live map's pipeline");
    }
}
