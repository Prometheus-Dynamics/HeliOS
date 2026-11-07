//! Camera calibration on the device: views of a ChArUco board captured from the camera's Styx
//! service and the lens fitted to them. helios-engine does both (its `calibration` actions,
//! `helios_calibration_wire`): the API has no Eidos and keeps only the views, plain points.
//!
//! `capture` has the engine take one gray frame as a **low-priority** client of the camera service
//! (as the preview is: the vision client's plan does not change) and find the board; when it is
//! seen well enough, the view is kept. `solve` has the engine fit the intrinsics to the views kept
//! for that camera and size; the result is returned, and stored only when the client `PUT`s it as
//! the camera's calibration. Views live in memory until solved, reset or the API restarts.

use std::{collections::HashMap, path::Path, time::Duration};

use helios_calibration_wire::{self as wire, Board, DetectRequest, Detection, Fit, PointPair};
use orion::control_plane::{ActionState, TypedConfigValue};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::sync::Mutex;

use crate::{
    camera_context::{CameraCalibration, Distortion, LensModel, Orientation},
    camera_uncertainty::CalibrationUncertainty,
    error::{ApiError, ApiResult},
    orion::ActionCall,
};

/// How long the engine has for a capture (a camera connect and a frame) or a solve.
const ENGINE_WAIT: Duration = Duration::from_secs(20);
/// A view with fewer board corners than this is too little to constrain the fit.
pub const MIN_VIEW_CORNERS: usize = 8;
/// Views kept per camera at most; capture beyond replaces nothing and is refused.
const MAX_VIEWS: usize = 80;

/// The printed board (`POST .../calibration/capture`).
pub type BoardSpec = Board;

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
    views: Vec<Vec<PointPair>>,
}

/// The views captured per camera.
#[derive(Default)]
pub struct CalibrationSessions {
    sessions: Mutex<HashMap<String, Session>>,
}

impl CalibrationSessions {
    /// Has the engine on `node_id` find the board in a frame of the camera service at `socket`,
    /// and keeps the view for camera `id` when it is good enough.
    pub async fn capture(&self, engine: &impl ActionCall, node_id: &str, id: &str, socket: &Path, request: CaptureRequest) -> ApiResult<CaptureResult> {
        let detect = DetectRequest { socket: socket.to_string_lossy().into_owned(), board: request.board.clone(), width: request.width, height: request.height };
        let Detection { width, height, points } = call(engine, node_id, wire::DETECT, &detect).await?;
        let corners = points.iter().map(|pair| pair.image).collect::<Vec<_>>();

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
            session.views.push(points);
        }
        Ok(CaptureResult { corners, width, height, kept, views: session.views.len(), message: if request.keep { message } else { None } })
    }

    /// Has the engine on `node_id` solve the views kept for `id`; the calibration says it was
    /// taken in `orientation`.
    pub async fn solve(&self, engine: &impl ActionCall, node_id: &str, id: &str, request: SolveRequest, orientation: Orientation) -> ApiResult<SolveResult> {
        let sessions = self.sessions.lock().await;
        let session = sessions.get(id).ok_or_else(|| ApiError::conflict("no views captured for this camera"))?;
        let views: Vec<Vec<PointPair>> = session.views.iter().enumerate().filter(|(index, _)| !request.drop.contains(index)).map(|(_, view)| view.clone()).collect();
        let (board, width, height) = (session.board.clone(), session.width, session.height);
        drop(sessions);
        let model = request.model;
        let lens = match model {
            LensModel::Pinhole => wire::Lens::Pinhole,
            LensModel::Fisheye => wire::Lens::Fisheye,
        };
        let fit: Fit = call(engine, node_id, wire::SOLVE, &wire::SolveRequest { lens, views, min_views: 4, min_points_per_view: MIN_VIEW_CORNERS }).await?;
        let d = fit.distortion;
        let calibration = CameraCalibration {
            width,
            height,
            model,
            fx: fit.fx,
            fy: fit.fy,
            cx: fit.cx,
            cy: fit.cy,
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

/// Runs engine action `name` with `request` and reads its answer. A refusal the engine explains
/// (a bad board, too few views) is the client's to fix; no answer is the engine's.
async fn call<T: DeserializeOwned>(engine: &impl ActionCall, node_id: &str, name: &str, request: &impl Serialize) -> ApiResult<T> {
    let json = serde_json::to_string(request).map_err(|error| ApiError::internal(error.to_string()))?;
    let action = crate::routes::resources::action_request(&format!("engine.runtime.{node_id}"), name, [(wire::ARG.to_string(), TypedConfigValue::String(json))].into());
    let result = engine.call_action(action, ENGINE_WAIT).await?;
    match result.state {
        ActionState::Succeeded => match result.output.get("value") {
            Some(TypedConfigValue::String(json)) => serde_json::from_str(json).map_err(|error| ApiError::backend(format!("helios-engine answered {name} with {error}"))),
            _ => Err(ApiError::backend(format!("helios-engine answered {name} without a value"))),
        },
        ActionState::Failed { reason } => Err(ApiError::unprocessable(reason)),
        ActionState::Rejected { reason } => Err(ApiError::backend(format!("helios-engine refused {name}: {reason}"))),
        ActionState::TimedOut | ActionState::Accepted | ActionState::Running { .. } => Err(ApiError::backend(format!("helios-engine did not finish {name} in time"))),
        ActionState::Cancelled => Err(ApiError::backend(format!("{name} was cancelled"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orion::control_plane::{ActionRequest, ActionResult};

    /// An engine that answers each action with a canned JSON value, and keeps what it was sent.
    struct FakeEngine {
        answer: ActionState,
        value: Option<String>,
        sent: std::sync::Mutex<Vec<ActionRequest>>,
    }

    impl FakeEngine {
        fn answering(value: impl Serialize) -> Self {
            Self { answer: ActionState::Succeeded, value: Some(serde_json::to_string(&value).expect("json")), sent: std::sync::Mutex::default() }
        }

        fn sent(&self) -> Vec<ActionRequest> {
            self.sent.lock().expect("sent").clone()
        }

        fn request<T: DeserializeOwned>(&self, index: usize) -> T {
            match self.sent()[index].args.get(wire::ARG) {
                Some(TypedConfigValue::String(json)) => serde_json::from_str(json).expect("request"),
                other => panic!("no request argument: {other:?}"),
            }
        }
    }

    impl ActionCall for FakeEngine {
        async fn call_action(&self, request: ActionRequest, _timeout: Duration) -> ApiResult<ActionResult> {
            self.sent.lock().expect("sent").push(request.clone());
            let mut result = ActionResult::new(request.action_id, request.target, request.name, orion::core::NodeId::new("node-local"), self.answer.clone());
            if let Some(value) = &self.value {
                result = result.with_output("value", TypedConfigValue::String(value.clone()));
            }
            Ok(result)
        }
    }

    fn board() -> BoardSpec {
        BoardSpec { squares_x: 8, squares_y: 6, square_mm: 30.0, marker_mm: 22.0, dictionary: "aruco_4x4_50".into() }
    }

    fn view(corners: usize) -> Vec<PointPair> {
        (0..corners).map(|n| PointPair { object: [n as f64 * 0.03, 0.03], image: [10.0 * n as f64, 20.0] }).collect()
    }

    fn capture_request() -> CaptureRequest {
        CaptureRequest { board: board(), width: Some(640), height: Some(400), keep: true }
    }

    #[tokio::test]
    async fn a_capture_asks_the_engine_for_the_board_and_keeps_a_good_view() {
        let sessions = CalibrationSessions::default();
        let engine = FakeEngine::answering(Detection { width: 640, height: 400, points: view(35) });
        let result = sessions.capture(&engine, "raze-1", "cam", Path::new("/run/styx/cam.sock"), capture_request()).await.expect("capture");
        assert!(result.kept && result.message.is_none(), "{result:?}");
        assert_eq!((result.corners.len(), result.views, result.corners[1]), (35, 1, [10.0, 20.0]));

        let sent = engine.sent();
        assert_eq!((sent[0].name.as_str(), sent[0].target.id()), (wire::DETECT, "engine.runtime.raze-1"));
        let detect: DetectRequest = engine.request(0);
        assert_eq!((detect.socket.as_str(), detect.board, detect.width, detect.height), ("/run/styx/cam.sock", board(), Some(640), Some(400)));
    }

    #[tokio::test]
    async fn too_few_corners_are_not_kept_and_say_why() {
        let sessions = CalibrationSessions::default();
        let few = FakeEngine::answering(Detection { width: 640, height: 400, points: view(MIN_VIEW_CORNERS - 1) });
        let result = sessions.capture(&few, "n", "cam", Path::new("/s"), capture_request()).await.expect("capture");
        assert!(!result.kept && result.message.as_deref().is_some_and(|m| m.contains("board corners")), "{result:?}");
        let none = FakeEngine::answering(Detection { width: 640, height: 400, points: Vec::new() });
        let result = sessions.capture(&none, "n", "cam", Path::new("/s"), capture_request()).await.expect("capture");
        assert_eq!(result.message.as_deref(), Some("no board in view"));
    }

    #[tokio::test]
    async fn a_solve_sends_the_views_kept_less_the_dropped_and_reads_the_fit() {
        let sessions = CalibrationSessions::default();
        let views = (0..6).map(|n| view(20 + n)).collect::<Vec<_>>();
        sessions.sessions.lock().await.insert("cam".into(), Session { board: board(), width: 640, height: 400, views: views.clone() });
        let n = 4 + crate::camera_uncertainty::distortion_len(LensModel::Pinhole);
        let covariance = (0..n * n).map(|i| if i % (n + 1) == 0 { 0.25 } else { 0.0 }).collect();
        let fit =
            Fit { fx: 600.0, fy: 601.0, cx: 320.0, cy: 200.0, distortion: wire::Distortion { k1: 0.01, ..wire::Distortion::default() }, rms: 0.2, view_rms: vec![0.2; 5], covariance, converged: true };
        let engine = FakeEngine::answering(fit);
        let result = sessions.solve(&engine, "n", "cam", SolveRequest { model: LensModel::Pinhole, drop: vec![2] }, Orientation::default()).await.expect("solve");

        let sent: wire::SolveRequest = engine.request(0);
        assert_eq!(engine.sent()[0].name, wire::SOLVE);
        assert_eq!((sent.lens, sent.min_views, sent.min_points_per_view), (wire::Lens::Pinhole, 4, MIN_VIEW_CORNERS));
        assert_eq!(sent.views, [&views[..2], &views[3..]].concat(), "view 2 is left out");

        let c = &result.calibration;
        assert_eq!((c.fx, c.fy, c.cx, c.cy, c.distortion.k1, c.width, c.height), (600.0, 601.0, 320.0, 200.0, 0.01, 640, 400));
        assert_eq!((c.rms_px, result.views, result.converged), (Some(0.2), 5, true));
        let uncertainty = c.uncertainty.as_ref().expect("solved uncertainty");
        assert!((uncertainty.sigma.fx - 0.5).abs() < 1e-12, "{uncertainty:?}");
        assert!(c.source.as_deref().is_some_and(|s| s.contains("8x6") && s.contains("5 views")), "{c:?}");
    }

    #[tokio::test]
    async fn the_engine_s_reasons_reach_the_client() {
        let sessions = CalibrationSessions::default();
        let failing = FakeEngine { answer: ActionState::Failed { reason: "unknown board dictionary \"nope\"".into() }, value: None, sent: std::sync::Mutex::default() };
        let error = sessions.capture(&failing, "n", "cam", Path::new("/s"), capture_request()).await.expect_err("fails");
        assert!(error.message.contains("nope"), "{error:?}");
        assert!(sessions.solve(&failing, "n", "nothing-captured", SolveRequest::default(), Orientation::default()).await.is_err());
    }
}
