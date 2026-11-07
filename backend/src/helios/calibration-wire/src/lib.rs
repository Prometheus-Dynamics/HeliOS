//! A camera calibration as helios-api and helios-engine exchange it: plain data and nothing from
//! Eidos, so the API stays a thin client. The engine finds the board and fits the lens (Eidos);
//! the API keeps the views between captures and stores what the user accepts.
//!
//! Both run as Orion actions on the engine's runtime resource (`engine.runtime.<node>`), each
//! with one argument, [`ARG`], holding the request as JSON, and the answer as JSON in the
//! `value` output.

use serde::{Deserialize, Serialize};

/// Find the board in one frame of a camera service: [`DetectRequest`] -> [`Detection`].
pub const DETECT: &str = "calibration.detect";
/// Fit a lens to views: [`SolveRequest`] -> [`Fit`].
pub const SOLVE: &str = "calibration.solve";
/// The one action argument (and the one output key is `value`).
pub const ARG: &str = "request";

/// A printed ChArUco board.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Board {
    /// Squares across and down (e.g. 8 x 6).
    pub squares_x: u32,
    pub squares_y: u32,
    /// Square and marker edge, millimetres, as printed.
    pub square_mm: f64,
    pub marker_mm: f64,
    /// ArUco dictionary of its markers (`aruco_4x4_50`).
    pub dictionary: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectRequest {
    /// The camera service's frames socket.
    pub socket: String,
    pub board: Board,
    /// The size to take the frame at; the camera's native size when absent.
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

/// A board corner: where it is on the board (metres) and where it was seen (pixels).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PointPair {
    pub object: [f64; 2],
    pub image: [f64; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Detection {
    /// The frame's size.
    pub width: u32,
    pub height: u32,
    /// The board's corners found (empty: no board in view).
    pub points: Vec<PointPair>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lens {
    #[default]
    Pinhole,
    Fisheye,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolveRequest {
    pub lens: Lens,
    pub views: Vec<Vec<PointPair>>,
    pub min_views: usize,
    pub min_points_per_view: usize,
}

/// Distortion coefficients (unused ones zero; a fisheye uses k1-k4).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Distortion {
    pub k1: f64,
    pub k2: f64,
    pub k3: f64,
    pub k4: f64,
    pub k5: f64,
    pub k6: f64,
    pub p1: f64,
    pub p2: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fit {
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
    pub distortion: Distortion,
    /// Reprojection error over every view, and of each view, pixels.
    pub rms: f64,
    pub view_rms: Vec<f64>,
    /// The parameters' covariance, row-major (fx, fy, cx, cy, then the lens's distortion terms).
    pub covariance: Vec<f64>,
    pub converged: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_read_back_as_written() {
        let detect = DetectRequest {
            socket: "/run/styx/cam.sock".into(),
            board: Board { squares_x: 8, squares_y: 6, square_mm: 30.0, marker_mm: 22.0, dictionary: "aruco_4x4_50".into() },
            width: Some(640),
            height: None,
        };
        let json = serde_json::to_string(&detect).expect("json");
        assert_eq!(serde_json::from_str::<DetectRequest>(&json).expect("back"), detect);
        let solve = SolveRequest { lens: Lens::Fisheye, views: vec![vec![PointPair { object: [0.03, 0.06], image: [101.5, 88.25] }]], min_views: 4, min_points_per_view: 8 };
        let json = serde_json::to_string(&solve).expect("json");
        assert!(json.contains("\"lens\":\"fisheye\""), "{json}");
        assert_eq!(serde_json::from_str::<SolveRequest>(&json).expect("back"), solve);
    }
}
