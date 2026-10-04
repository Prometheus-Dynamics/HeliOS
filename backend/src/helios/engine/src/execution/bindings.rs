//! Resolving workload bindings against Orion resources.
//!
//! A bound resource whose endpoints include `styx-frames+unix://<socket>` is a camera served by
//! a Styx `CameraService`: the workload becomes frame-driven. Any other bound resource is fed to
//! the graph as JSON describing its current state.

use std::{collections::BTreeMap, path::PathBuf};

use orion::{
    ResourceId,
    control_plane::{ResourceRecord, config_json_value},
};
use styx::prelude::{FrameRequest, Frames};

use super::ExecutionError;
use crate::model::{ExecutionWorkload, FrameRequestOptions};

/// Endpoint scheme of a Styx camera service socket (`styx-frames+unix:///run/.../camera.sock`).
pub const STYX_FRAMES_ENDPOINT_SCHEME: &str = "styx-frames+unix";

/// A graph input fed by a Styx camera service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrameSourceSpec {
    pub input: String,
    pub resource_id: String,
    pub socket_path: PathBuf,
    pub request: FrameRequestOptions,
}

impl FrameSourceSpec {
    /// What the camera service is asked for: the latest luma frame at native size, unless the
    /// binding asks for a size or pyramid levels.
    pub fn request(&self) -> FrameRequest {
        let mut request = Frames::gray().latest();
        if let Some((width, height)) = self.request.output_resolution {
            request = request.size(width, height);
        }
        if let Some(levels) = self.request.pyramid_levels {
            request = request.pyramid(levels);
        }
        request
    }
}

/// A graph input fed with a resource's state as JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResourceInput {
    pub input: String,
    /// Changes whenever the resource record changes.
    pub revision: String,
    pub payload: String,
}

#[derive(Debug, Default)]
pub(crate) struct ResolvedBindings {
    pub frames: Vec<FrameSourceSpec>,
    pub resources: Vec<ResourceInput>,
}

pub(crate) fn resolve_bindings(workload: &ExecutionWorkload, resources: &BTreeMap<ResourceId, ResourceRecord>) -> Result<ResolvedBindings, ExecutionError> {
    let mut resolved = ResolvedBindings::default();
    for binding in &workload.bindings {
        let resource = resources.get(binding.resource_id.as_str()).ok_or_else(|| ExecutionError::MissingResource(binding.resource_id.clone()))?;
        if let Some(socket_path) = frame_socket_path(&resource.endpoints) {
            resolved.frames.push(FrameSourceSpec { input: binding.input.clone(), resource_id: binding.resource_id.clone(), socket_path, request: binding.frame_request.clone() });
            continue;
        }
        let payload = serde_json::to_string(&resource_binding_payload(resource)).map_err(|error| ExecutionError::Execute(error.to_string()))?;
        let revision = serde_json::to_string(resource).unwrap_or_else(|_| format!("{resource:?}"));
        resolved.resources.push(ResourceInput { input: binding.input.clone(), revision, payload });
    }
    Ok(resolved)
}

/// The camera service socket named by a `styx-frames+unix://` endpoint, if any.
pub(crate) fn frame_socket_path(endpoints: &[String]) -> Option<PathBuf> {
    endpoints.iter().find_map(|endpoint| endpoint.strip_prefix(STYX_FRAMES_ENDPOINT_SCHEME).and_then(|rest| rest.strip_prefix("://"))).filter(|path| !path.is_empty()).map(PathBuf::from)
}

fn resource_binding_payload(resource: &ResourceRecord) -> serde_json::Value {
    let action_result = resource.state.as_ref().and_then(|state| state.action_result.as_ref()).map(|result| {
        serde_json::json!({
            "action_kind": result.action_kind,
            "status": format!("{:?}", result.status),
            "data": result
                .data
                .as_ref()
                .and_then(|value| config_json_value(&BTreeMap::from([("value".to_string(), value.clone())])).ok())
                .and_then(|value| value.get("value").cloned())
                .unwrap_or(serde_json::Value::Null),
            "error": result.error,
        })
    });

    serde_json::json!({
        "resource_id": resource.resource_id.as_str(),
        "resource_type": resource.resource_type.as_str(),
        "provider_id": resource.provider_id.as_str(),
        "labels": resource.labels,
        "endpoints": resource.endpoints,
        "lease_state": format!("{:?}", resource.lease_state),
        "state": {
            "observed_at_ms": resource.state.as_ref().map(|state| state.observed_at_ms),
            "action_result": action_result,
            "config": resource.state.as_ref().and_then(|state| state.config.as_ref()).map(resource_config_to_json),
        },
    })
}

fn resource_config_to_json(config: &orion::control_plane::ResourceConfigState) -> serde_json::Value {
    config_json_value(&config.payload).unwrap_or_else(|error| {
        serde_json::json!({
            "decode_error": error.to_string(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_socket_path_reads_styx_frames_endpoint_only() {
        assert_eq!(frame_socket_path(&["unix:///run/x.sock".into(), "styx-frames+unix:///run/helios/cameras/front.sock".into()]), Some(PathBuf::from("/run/helios/cameras/front.sock")));
        assert_eq!(frame_socket_path(&["styx-frame-lease+unix:///run/old.sock".into()]), None);
        assert_eq!(frame_socket_path(&["styx-frames+unix://".into()]), None);
    }
}
