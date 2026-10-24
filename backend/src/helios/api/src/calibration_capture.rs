//! Camera calibration on the device: views of a ChArUco board captured from the camera's Styx
//! service, solved with Eidos (`eidos_detectors::calibration_target`, `calibrate_camera`).
//!
//! `capture` takes one gray frame as a **low-priority** client of the camera service (as the
//! preview is: the vision client's plan does not change), finds the board and, when it is seen
//! well enough, keeps the view. `solve` fits the intrinsics to the views kept for that camera and
//! size; the result is returned, and stored only when the client `PUT`s it as the camera's
//! calibration. Views live in memory until solved, reset or the API restarts.

use std::{collections::HashMap, path::Path, time::Duration};

use eidos_calibration::{CalibrationView, CameraCalibrationConfig, calibrate_camera};
use eidos_core::catalog::ParamEnum;
use eidos_detectors::calibration_target::{CharucoBoard, CharucoDetector, view::charuco_view};
use serde::{Deserialize, Serialize};
use styx::{core::prelude::RecvOutcome, ipc::FrameClient, prelude::Frames};
use tokio::sync::Mutex;

use crate::{
    camera_context::{CameraCalibration, Distortion, LensModel, Orientation},
    camera_uncertainty::CalibrationUncertainty,
    error::{ApiError, ApiResult},
};

/// How long a capture waits for the camera service to connect and hand over a frame.
const FRAME_TIMEOUT: Duration = Duration::from_secs(3);
/// A view with fewer board corners than this is too little to constrain the fit.
pub const MIN_VIEW_CORNERS: usize = 8;
/// Views kept per camera at most; capture beyond replaces nothing and is refused.
const MAX_VIEWS: usize = 80;

/// The printed board (`POST .../calibration/capture`).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BoardSpec {
    /// Squares across and down (e.g. 8 x 6).
    pub squares_x: u32,
    pub squares_y: u32,
    /// Square and marker edge, millimetres, as printed.
    pub square_mm: f64,
    pub marker_mm: f64,
    /// ArUco dictionary of its markers (`aruco_4x4_50`).
    pub dictionary: String,
}

impl BoardSpec {
    fn board(&self) -> ApiResult<CharucoBoard> {
        let dictionary = eidos_aruco::ArucoDictionaryKind::VARIANTS
            .iter()
            .position(|variant| variant.name == self.dictionary)
            .and_then(|index| eidos_aruco::ArucoDictionaryKind::from_index(index as u16))
            .ok_or_else(|| ApiError::unprocessable(format!("unknown board dictionary {:?}", self.dictionary)))?;
        // Object points in metres, so poses solved later come out in metres too.
        CharucoBoard::new(self.squares_x, self.squares_y, self.square_mm / 1000.0, self.marker_mm / 1000.0, dictionary).map_err(|error| ApiError::unprocessable(format!("board: {error}")))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureRequest {
    pub board: BoardSpec,
    /// Image size to calibrate at (the size the camera's pipelines run at); the camera's
    /// native size when absent.
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    /// Keep the view when the board is seen (default); false only looks.
    #[serde(default = "yes")]
    pub keep: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CaptureResult {
    /// Board corners found, pixels.
    pub corners: Vec<[f64; 2]>,
    pub width: u32,
    pub height: u32,
    /// The view was kept (enough corners, `keep`).
    pub kept: bool,
    /// Views kept for this camera and size so far.
    pub views: usize,
    /// Why nothing was kept, when nothing was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolveRequest {
    #[serde(default)]
    pub model: LensModel,
    /// View indices to leave out (the worst ones after a first solve).
    #[serde(default)]
    pub drop: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SolveResult {
    /// Not stored yet: `PUT /v1/cameras/{id}/calibration` stores it.
    pub calibration: CameraCalibration,
    /// Reprojection error of each view used, pixels, in view order.
    pub view_rms: Vec<f64>,
    pub views: usize,
    pub converged: bool,
}

struct Session {
    board: BoardSpec,
    width: u32,
    height: u32,
    views: Vec<CalibrationView>,
}

/// The views captured per camera.
#[derive(Default)]
pub struct CalibrationSessions {
    sessions: Mutex<HashMap<String, Session>>,
}

impl CalibrationSessions {
    pub async fn capture(&self, id: &str, socket: &Path, request: CaptureRequest) -> ApiResult<CaptureResult> {
        let board = request.board.board()?;
        let size = request.width.zip(request.height);
        let socket = socket.to_path_buf();
        let (corners, view, width, height) = tokio::task::spawn_blocking(move || detect(&socket, board, size)).await.map_err(|error| ApiError::internal(error.to_string()))??;

        let mut sessions = self.sessions.lock().await;
        // A different board or size starts over.
        let session = sessions.entry(id.to_string()).or_insert_with(|| Session { board: request.board.clone(), width, height, views: Vec::new() });
        if session.board != request.board || (session.width, session.height) != (width, height) {
            *session = Session { board: request.board.clone(), width, height, views: Vec::new() };
        }
        let message = if corners.len() < MIN_VIEW_CORNERS {
            Some(if corners.is_empty() { "no board in view".to_string() } else { format!("only {} board corners in view (at least {MIN_VIEW_CORNERS})", corners.len()) })
        } else if session.views.len() >= MAX_VIEWS {
            Some(format!("{MAX_VIEWS} views is the most kept; solve or start over"))
        } else {
            None
        };
        let kept = request.keep && message.is_none();
        if kept {
            session.views.push(view);
        }
        Ok(CaptureResult { corners, width, height, kept, views: session.views.len(), message: if request.keep { message } else { None } })
    }

    /// Solves the views kept for `id`; the calibration says it was taken in `orientation`.
    pub async fn solve(&self, id: &str, request: SolveRequest, orientation: Orientation) -> ApiResult<SolveResult> {
        let sessions = self.sessions.lock().await;
        let session = sessions.get(id).ok_or_else(|| ApiError::conflict("no views captured for this camera"))?;
        let views: Vec<CalibrationView> = session.views.iter().enumerate().filter(|(index, _)| !request.drop.contains(index)).map(|(_, view)| view.clone()).collect();
        let (board, width, height) = (session.board.clone(), session.width, session.height);
        drop(sessions);
        let model = request.model;
        let config = CameraCalibrationConfig { lens_model: eidos_lens(model), min_views: 4, min_points_per_view: MIN_VIEW_CORNERS, ..CameraCalibrationConfig::default() };
        let fit = tokio::task::spawn_blocking(move || calibrate_camera(&views, config))
            .await
            .map_err(|error| ApiError::internal(error.to_string()))?
            .map_err(|error| ApiError::unprocessable(format!("calibration did not solve: {error:?}")))?;
        let d = fit.calibration.distortion;
        let i = fit.calibration.intrinsics;
        let calibration = CameraCalibration {
            width,
            height,
            model,
            fx: i.fx,
            fy: i.fy,
            cx: i.cx,
            cy: i.cy,
            distortion: Distortion { k1: d.k1, k2: d.k2, k3: d.k3, k4: d.k4, k5: d.k5, k6: d.k6, p1: d.p1, p2: d.p2 },
            rms_px: Some(fit.rms),
            uncertainty: Some(CalibrationUncertainty::solved(&fit.covariance, fit.rms, fit.view_rms.len(), model)),
            orientation,
            source: Some(format!("helios charuco {}x{} {} mm {}, {} views", board.squares_x, board.squares_y, board.square_mm, board.dictionary, fit.view_rms.len())),
            saved_at_ms: 0,
            provenance: None,
        };
        calibration.validate()?;
        Ok(SolveResult { calibration, views: fit.view_rms.len(), view_rms: fit.view_rms, converged: fit.converged })
    }

    pub async fn reset(&self, id: &str) {
        self.sessions.lock().await.remove(id);
    }

    /// The views kept by the capture session with the most (`None`: no session is open). The LED
    /// director shows their coverage.
    pub async fn most_views(&self) -> Option<u32> {
        self.sessions.lock().await.values().map(|session| session.views.len() as u32).max()
    }
}

fn eidos_lens(model: LensModel) -> eidos_calibration::LensModel {
    match model {
        LensModel::Pinhole => eidos_calibration::LensModel::Pinhole,
        LensModel::Fisheye => eidos_calibration::LensModel::Fisheye,
    }
}

/// One gray frame from the camera service at `socket`, and the board in it.
fn detect(socket: &Path, board: CharucoBoard, size: Option<(u32, u32)>) -> ApiResult<(Vec<[f64; 2]>, CalibrationView, u32, u32)> {
    let mut request = Frames::gray().latest();
    if let Some((width, height)) = size {
        request = request.size(width, height);
    }
    let client = FrameClient::options(socket).low_priority().timeout(FRAME_TIMEOUT).request(&request).map_err(|error| ApiError::backend(format!("camera service: {error}")))?;
    let frame = match client.recv(FRAME_TIMEOUT) {
        RecvOutcome::Data(frame) => frame,
        RecvOutcome::Empty => return Err(ApiError::backend("the camera sent no frame in time")),
        RecvOutcome::Closed => return Err(ApiError::backend(format!("camera service closed: {}", client.last_error().map(|error| error.to_string()).unwrap_or_default()))),
    };
    let (width, height) = (frame.meta().format.resolution.width.get(), frame.meta().format.resolution.height.get());
    let mut detector = CharucoDetector::new(board.clone());
    let detection = detector.detect(&frame).map_err(|error| ApiError::internal(format!("board detection: {error}")))?;
    let Some(detection) = detection else {
        return Ok((Vec::new(), CalibrationView::new(Vec::new()), width, height));
    };
    let corners = detection.corners.iter().map(|corner| [corner.position.u, corner.position.v]).collect();
    Ok((corners, charuco_view(&board, &detection), width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eidos_calibration::{CalibrationPointPair, Point2};

    fn board() -> BoardSpec {
        BoardSpec { squares_x: 8, squares_y: 6, square_mm: 30.0, marker_mm: 22.0, dictionary: "aruco_4x4_50".into() }
    }

    /// The board's inner corners seen by a pinhole camera (fx = fy = 600, centre 640x400 / 2)
    /// from a pose tilted by `(ax, ay)` radians at `z` metres.
    fn view(ax: f64, ay: f64, z: f64) -> CalibrationView {
        let (fx, cx, cy) = (600.0, 320.0, 200.0);
        let mut points = Vec::new();
        for row in 1..6 {
            for col in 1..8 {
                let (x, y) = (f64::from(col) * 0.03 - 0.12, f64::from(row) * 0.03 - 0.09);
                // Rotate about y then x, then push out along z.
                let (x1, z1) = (x * ay.cos(), -x * ay.sin());
                let (y2, z2) = (y * ax.cos() - z1 * ax.sin(), y * ax.sin() + z1 * ax.cos());
                let depth = z + z2;
                points.push(CalibrationPointPair { object: Point2 { x: f64::from(col) * 0.03, y: f64::from(row) * 0.03 }, image: Point2 { x: fx * x1 / depth + cx, y: fx * y2 / depth + cy } });
            }
        }
        CalibrationView::new(points)
    }

    #[tokio::test]
    async fn solves_known_intrinsics_from_views() {
        let sessions = CalibrationSessions::default();
        let views = [(0.0, 0.3, 0.6), (0.3, 0.0, 0.7), (-0.25, 0.2, 0.5), (0.2, -0.3, 0.65), (-0.1, -0.2, 0.55), (0.35, 0.25, 0.8)].map(|(ax, ay, z)| view(ax, ay, z)).to_vec();
        sessions.sessions.lock().await.insert("cam".into(), Session { board: board(), width: 640, height: 400, views });
        let result = sessions.solve("cam", SolveRequest::default(), Orientation::default()).await.expect("solves");
        let c = &result.calibration;
        assert!((c.fx - 600.0).abs() < 1.0 && (c.fy - 600.0).abs() < 1.0, "{c:?}");
        assert!((c.cx - 320.0).abs() < 1.0 && (c.cy - 200.0).abs() < 1.0, "{c:?}");
        assert!(c.rms_px.is_some_and(|rms| rms < 0.01));
        let uncertainty = c.uncertainty.as_ref().expect("solved uncertainty");
        assert_eq!((uncertainty.kind, uncertainty.covariance.len(), uncertainty.sigma.distortion.len()), (crate::camera_uncertainty::UncertaintyKind::Solved, 81, 5), "{uncertainty:?}");
        assert!(uncertainty.sigma.fx > 0.0 && uncertainty.sigma.cx > 0.0, "{uncertainty:?}");
        assert_eq!((c.width, c.height, result.views), (640, 400, 6));
        // Dropping views leaves too few.
        assert!(sessions.solve("cam", SolveRequest { drop: vec![0, 1, 2], ..SolveRequest::default() }, Orientation::default()).await.is_err());
    }

    #[test]
    fn board_spec_checks_dictionary_and_sizes() {
        assert!(board().board().is_ok());
        assert!(BoardSpec { dictionary: "nope".into(), ..board() }.board().is_err());
        assert!(BoardSpec { marker_mm: 40.0, ..board() }.board().is_err());
    }
}
