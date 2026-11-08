//! Resolving workload bindings against Orion resources.
//!
//! A bound resource whose endpoints include `styx-frames+unix://<socket>` is a camera served by
//! a Styx `CameraService`: the workload becomes frame-driven. Any other bound resource is fed to
//! the graph as a typed `BoundResource` (`helios:bound_resource`) with its current state.

use std::{collections::BTreeMap, path::PathBuf};

use orion::{ResourceId, control_plane::ResourceRecord};
use styx::prelude::{FrameRequest, Frames};

use super::ExecutionError;
use crate::{
    model::{ExecutionWorkload, FrameRequestOptions},
    plugins::resource::BoundResource,
};

/// Endpoint scheme of a Styx camera service socket (`styx-frames+unix:///run/.../camera.sock`).
pub const STYX_FRAMES_ENDPOINT_SCHEME: &str = "styx-frames+unix";

/// A graph input fed by a Styx camera service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrameSourceSpec {
    pub input: String,
    pub resource_id: String,
    pub socket_path: PathBuf,
    pub request: FrameRequestOptions,
    /// Ask for frames at their first slice (still filling, with progress): only for graphs whose
    /// frame readers handle that (see `graph_takes_sliced_frames`).
    pub sliced: bool,
}

impl FrameSourceSpec {
    /// What the camera service is asked for: the latest luma frame at native size, unless the
    /// binding asks for a size, pyramid levels or a frame rate. A stored rate is an exact one,
    /// requested with every frame so it applies from the first frame of every (re)connection,
    /// without waiting for the camera controls to be restored.
    pub(super) fn request(&self) -> FrameRequest {
        let mut request = Frames::gray().latest();
        if let Some((width, height)) = self.request.output_resolution {
            request = request.size(width, height);
        }
        if let Some(levels) = self.request.pyramid_levels {
            request = request.pyramid(levels);
        }
        if let Some(fps) = self.request.fps {
            request = request.fps(fps);
        }
        request
    }
}

/// Nodes that start on a frame's first rows and wait for the rest themselves. Every Eidos node
/// waits for a sliced frame to complete before reading it, so a graph with one of these gains
/// latency and the others stay correct; graphs without one get complete frames. The tracked groups
/// are the ids of a graph that holds a group as one node; `track_plan` is their first stage, the id
/// of an expanded graph.
const SLICED_FRAME_NODES: &[&str] = &["eidos:detectors.apriltag_tracked", "eidos:detectors.aruco_tracked", "eidos:detectors.track_plan"];

/// Whether a workload's graph should get frames at their first slice.
pub(crate) fn graph_takes_sliced_frames(graph_json: &str) -> bool {
    SLICED_FRAME_NODES.iter().any(|id| graph_json.contains(&format!("\"{id}\"")))
}

/// A graph input fed with a resource's state (`helios:bound_resource`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResourceInput {
    pub input: String,
    /// Changes whenever the resource record changes.
    pub revision: String,
    pub payload: BoundResource,
}

#[derive(Debug, Default)]
pub(crate) struct ResolvedBindings {
    pub frames: Vec<FrameSourceSpec>,
    pub resources: Vec<ResourceInput>,
}

pub(crate) fn resolve_bindings(workload: &ExecutionWorkload, resources: &BTreeMap<ResourceId, ResourceRecord>) -> Result<ResolvedBindings, ExecutionError> {
    let mut resolved = ResolvedBindings::default();
    for binding in &workload.bindings {
        // Another pipeline's output: fed on the bus by the driver, not from Orion.
        if binding.pipeline.is_some() {
            continue;
        }
        let resource = resources.get(binding.resource_id.as_str()).ok_or_else(|| ExecutionError::MissingResource(binding.resource_id.clone()))?;
        if let Some(socket_path) = frame_socket_path(&resource.endpoints) {
            resolved.frames.push(FrameSourceSpec { input: binding.input.clone(), resource_id: binding.resource_id.clone(), socket_path, request: binding.frame_request.clone(), sliced: false });
            continue;
        }
        let payload = BoundResource::from(resource);
        let revision = serde_json::to_string(resource).unwrap_or_else(|_| format!("{resource:?}"));
        resolved.resources.push(ResourceInput { input: binding.input.clone(), revision, payload });
    }
    Ok(resolved)
}

/// The camera service socket named by a `styx-frames+unix://` endpoint, if any.
pub(crate) fn frame_socket_path(endpoints: &[String]) -> Option<PathBuf> {
    endpoints.iter().find_map(|endpoint| endpoint.strip_prefix(STYX_FRAMES_ENDPOINT_SCHEME).and_then(|rest| rest.strip_prefix("://"))).filter(|path| !path.is_empty()).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracked_detector_graphs_take_sliced_frames() {
        for id in SLICED_FRAME_NODES {
            assert!(graph_takes_sliced_frames(&format!(r#"{{"graph":{{"nodes":[{{"id":"{id}"}}]}}}}"#)), "{id}");
        }
        // The full-search groups read a whole frame, and an id only counts as a whole string.
        assert!(!graph_takes_sliced_frames(r#"{"graph":{"nodes":[{"id":"eidos:detectors.apriltag"}]}}"#));
        assert!(!graph_takes_sliced_frames(r#"{"graph":{"nodes":[{"id":"eidos:detectors.track_plan_v2"}]}}"#));
    }

    #[test]
    fn frame_socket_path_reads_styx_frames_endpoint_only() {
        assert_eq!(frame_socket_path(&["unix:///run/x.sock".into(), "styx-frames+unix:///run/helios/cameras/front.sock".into()]), Some(PathBuf::from("/run/helios/cameras/front.sock")));
        assert_eq!(frame_socket_path(&["styx-frame-lease+unix:///run/old.sock".into()]), None);
        assert_eq!(frame_socket_path(&["styx-frames+unix://".into()]), None);
    }
}
