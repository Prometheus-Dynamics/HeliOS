//! Camera calibration for helios-api, which has no Eidos of its own: find a ChArUco board in one
//! frame of a camera service, and fit a lens to the views the API kept. Both run as Orion actions
//! on the engine's runtime resource (`helios_calibration_wire`), off the async runtime.

use std::{collections::BTreeMap, path::Path, time::Duration};

use eidos_calibration::{CalibrationPointPair, CalibrationView, CameraCalibrationConfig, Point2, calibrate_camera};
use eidos_core::catalog::ParamEnum;
use eidos_detectors::calibration_target::{CharucoBoard, CharucoDetector, view::charuco_view};
use helios_calibration_wire::{ARG, Board, DETECT, DetectRequest, Detection, Distortion, Fit, Lens, PointPair, SOLVE, SolveRequest};
use orion::{
    client::ActionRequestWatch,
    control_plane::{ActionRequest, TypedConfigValue},
};
use styx::{core::prelude::RecvOutcome, ipc::FrameClient, prelude::Frames};
use tracing::warn;

/// How long a capture waits for the camera service to connect and hand over a frame.
const FRAME_TIMEOUT: Duration = Duration::from_secs(3);

/// Whether the engine answers action `name`.
pub fn is_calibration_action(name: &str) -> bool {
    name == DETECT || name == SOLVE
}

/// Answer the calibration actions sent to the engine until the watch ends; others are refused.
pub async fn serve(mut actions: ActionRequestWatch) {
    loop {
        let request = match actions.next().await {
            Ok(request) => request,
            Err(error) => {
                warn!(error = %error, "the engine's action watch ended");
                return;
            }
        };
        let reporter = actions.reporter();
        tokio::spawn(async move {
            let action_id = request.action_id.clone();
            if !is_calibration_action(&request.name) {
                let _ = reporter.reject(action_id, format!("helios-engine has no action {}", request.name)).await;
                return;
            }
            let answer = tokio::task::spawn_blocking(move || run(&request)).await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = match answer {
                Ok(json) => reporter.succeed(action_id, BTreeMap::from([("value".to_string(), TypedConfigValue::String(json))])).await,
                Err(reason) => reporter.fail(action_id, reason).await,
            };
        });
    }
}

/// One action, its request and answer as JSON.
fn run(request: &ActionRequest) -> Result<String, String> {
    let arg = match request.args.get(ARG) {
        Some(TypedConfigValue::String(json)) => json.as_str(),
        _ => return Err(format!("{} takes its request as the string argument `{ARG}`", request.name)),
    };
    let json = |error: serde_json::Error| format!("{}: {error}", request.name);
    if request.name == DETECT {
        serde_json::to_string(&detect(&serde_json::from_str(arg).map_err(json)?)?).map_err(json)
    } else {
        serde_json::to_string(&solve(&serde_json::from_str(arg).map_err(json)?)?).map_err(json)
    }
}

fn charuco(board: &Board) -> Result<CharucoBoard, String> {
    let dictionary = eidos_aruco::ArucoDictionaryKind::VARIANTS
        .iter()
        .position(|variant| variant.name == board.dictionary)
        .and_then(|index| eidos_aruco::ArucoDictionaryKind::from_index(index as u16))
        .ok_or_else(|| format!("unknown board dictionary {:?}", board.dictionary))?;
    // Object points in metres, so poses solved later come out in metres too.
    CharucoBoard::new(board.squares_x, board.squares_y, board.square_mm / 1000.0, board.marker_mm / 1000.0, dictionary).map_err(|error| format!("board: {error}"))
}

/// One gray frame from the camera service, as a low-priority client (the vision client's plan
/// does not change), and the board's corners in it.
pub fn detect(request: &DetectRequest) -> Result<Detection, String> {
    let board = charuco(&request.board)?;
    let mut frames = Frames::gray().latest();
    if let Some((width, height)) = request.width.zip(request.height) {
        frames = frames.size(width, height);
    }
    let client = FrameClient::options(Path::new(&request.socket)).low_priority().timeout(FRAME_TIMEOUT).request(&frames).map_err(|error| format!("camera service: {error}"))?;
    let frame = match client.recv(FRAME_TIMEOUT) {
        RecvOutcome::Data(frame) => frame,
        RecvOutcome::Empty => return Err("the camera sent no frame in time".into()),
        RecvOutcome::Closed => return Err(format!("camera service closed: {}", client.last_error().map(|error| error.to_string()).unwrap_or_default())),
    };
    let (width, height) = (frame.meta().format.resolution.width.get(), frame.meta().format.resolution.height.get());
    let detection = CharucoDetector::new(board.clone()).detect(&frame).map_err(|error| format!("board detection: {error}"))?;
    let points = detection
        .map(|detection| charuco_view(&board, &detection).points.iter().map(|pair| PointPair { object: [pair.object.x, pair.object.y], image: [pair.image.x, pair.image.y] }).collect())
        .unwrap_or_default();
    Ok(Detection { width, height, points })
}

/// The lens fit to `request`'s views.
pub fn solve(request: &SolveRequest) -> Result<Fit, String> {
    let views: Vec<CalibrationView> = request
        .views
        .iter()
        .map(|view| {
            CalibrationView::new(
                view.iter().map(|pair| CalibrationPointPair { object: Point2 { x: pair.object[0], y: pair.object[1] }, image: Point2 { x: pair.image[0], y: pair.image[1] } }).collect(),
            )
        })
        .collect();
    let lens_model = match request.lens {
        Lens::Pinhole => eidos_calibration::LensModel::Pinhole,
        Lens::Fisheye => eidos_calibration::LensModel::Fisheye,
    };
    let config = CameraCalibrationConfig { lens_model, min_views: request.min_views, min_points_per_view: request.min_points_per_view, ..CameraCalibrationConfig::default() };
    let fit = calibrate_camera(&views, config).map_err(|error| format!("calibration did not solve: {error:?}"))?;
    let (i, d) = (fit.calibration.intrinsics, fit.calibration.distortion);
    Ok(Fit {
        fx: i.fx,
        fy: i.fy,
        cx: i.cx,
        cy: i.cy,
        distortion: Distortion { k1: d.k1, k2: d.k2, k3: d.k3, k4: d.k4, k5: d.k5, k6: d.k6, p1: d.p1, p2: d.p2 },
        rms: fit.rms,
        view_rms: fit.view_rms,
        covariance: fit.covariance.to_vec(),
        converged: fit.converged,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The board's inner corners seen by a pinhole camera (fx = fy = 600, centre 640x400 / 2)
    /// from a pose tilted by `(ax, ay)` radians at `z` metres.
    fn view(ax: f64, ay: f64, z: f64) -> Vec<PointPair> {
        let (fx, cx, cy) = (600.0, 320.0, 200.0);
        let mut points = Vec::new();
        for row in 1..6 {
            for col in 1..8 {
                let (x, y) = (f64::from(col) * 0.03 - 0.12, f64::from(row) * 0.03 - 0.09);
                // Rotate about y then x, then push out along z.
                let (x1, z1) = (x * ay.cos(), -x * ay.sin());
                let (y2, z2) = (y * ax.cos() - z1 * ax.sin(), y * ax.sin() + z1 * ax.cos());
                let depth = z + z2;
                points.push(PointPair { object: [f64::from(col) * 0.03, f64::from(row) * 0.03], image: [fx * x1 / depth + cx, fx * y2 / depth + cy] });
            }
        }
        points
    }

    #[test]
    fn solves_known_intrinsics_from_views() {
        let views = [(0.0, 0.3, 0.6), (0.3, 0.0, 0.7), (-0.25, 0.2, 0.5), (0.2, -0.3, 0.65), (-0.1, -0.2, 0.55), (0.35, 0.25, 0.8)].map(|(ax, ay, z)| view(ax, ay, z)).to_vec();
        let fit = solve(&SolveRequest { lens: Lens::Pinhole, views, min_views: 4, min_points_per_view: 8 }).expect("solves");
        assert!((fit.fx - 600.0).abs() < 1.0 && (fit.fy - 600.0).abs() < 1.0, "{fit:?}");
        assert!((fit.cx - 320.0).abs() < 1.0 && (fit.cy - 200.0).abs() < 1.0, "{fit:?}");
        assert!(fit.rms < 0.01, "{fit:?}");
        assert_eq!((fit.covariance.len(), fit.view_rms.len()), (81, 6));
    }

    #[test]
    fn a_board_takes_a_known_dictionary_and_markers_inside_squares() {
        let board = Board { squares_x: 8, squares_y: 6, square_mm: 30.0, marker_mm: 22.0, dictionary: "aruco_4x4_50".into() };
        assert!(charuco(&board).is_ok());
        assert!(charuco(&Board { dictionary: "nope".into(), ..board.clone() }).is_err());
        assert!(charuco(&Board { marker_mm: 40.0, ..board }).is_err());
    }

    #[test]
    fn an_action_without_its_request_is_refused_with_why() {
        let request = ActionRequest::new("a", orion::control_plane::ActionTarget::Resource(orion::core::ResourceId::new("engine.runtime.n")), SOLVE);
        assert!(run(&request).unwrap_err().contains(ARG));
    }
}
