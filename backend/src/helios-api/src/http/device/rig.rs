mod layout;
mod peer_forward;
mod routes;
mod state;
#[cfg(test)]
mod tests;
mod types;

use axum::{
    Router,
    routing::{get, patch, put},
};

use crate::http::AppState;

pub use helios_engine::ipc::{PoseRotation, PoseVector, RigPose};
pub use types::{CameraLayoutCameraResponse, CameraLayoutResponse, RobotDimensions, UpdateCameraPoseRequest, UpdateRobotDimensionsRequest};

pub(crate) use layout::{__path_get_camera_layout, get_camera_layout};
pub(crate) use routes::{__path_clear_camera_pose, __path_update_camera_pose, __path_update_robot_dimensions, clear_camera_pose, update_camera_pose, update_robot_dimensions};
pub(crate) use state::camera_uid_from_keys;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/camera-layout", get(get_camera_layout))
        .route("/robot-dimensions", patch(update_robot_dimensions))
        .route("/cameras/{camera_uid}/pose", put(update_camera_pose).delete(clear_camera_pose))
}
