use daedalus::data::model::{StructFieldValue, Value as DaedalusValue};
use image::{DynamicImage, GrayImage, Rgba, RgbaImage};
use lib_cv::calibration::{solve_camera_intrinsics_with_hint, CalibrationPointPair, CalibrationSolveConfig as CvSolveConfig, CalibrationView};
use lib_cv::localization::CameraIntrinsics;
use lib_cv::modules::aruco::tag::{ArucoTagDecoding, ArucoTagFamily};
use lib_cv::modules::aruco::ArucoDetection2D;
use lib_cv::modules::aruco::{aruco_dictionary_from_name, ArucoDictionary};
use lib_cv::Point;
use nalgebra::{DMatrix, Matrix3};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;
use std::time::Instant;

use crate::ipc::{CalibrationBoard, CalibrationSolveConfig, CalibrationSolveDebugView, CalibrationSolveRequest, CalibrationSolveResponse, StreamCalibration};
use crate::services::StreamManager;

#[derive(Debug, Clone)]
pub enum CalibrationSolveFailure {
    InvalidInput(String),
    NotFound(String),
    Internal(String),
}

impl CalibrationSolveFailure {
    pub fn reason(&self) -> String {
        match self {
            CalibrationSolveFailure::InvalidInput(reason) | CalibrationSolveFailure::NotFound(reason) | CalibrationSolveFailure::Internal(reason) => reason.clone(),
        }
    }
}

impl StreamManager {
    pub async fn solve_calibration(&self, request: CalibrationSolveRequest) -> Result<CalibrationSolveResponse, CalibrationSolveFailure> {
        let _ = self;
        tokio::task::spawn_blocking(move || solve_calibration_sync(request)).await.map_err(|_| CalibrationSolveFailure::Internal("calibration solve task canceled".into()))?
    }
}

fn solve_calibration_sync(request: CalibrationSolveRequest) -> Result<CalibrationSolveResponse, CalibrationSolveFailure> {
    let solve_start = Instant::now();
    if request.images.is_empty() {
        return Err(CalibrationSolveFailure::InvalidInput("no calibration images provided".into()));
    }

    if request.images.len() > 200 {
        return Err(CalibrationSolveFailure::InvalidInput("too many calibration images (max 200)".into()));
    }

    validate_board(&request.board)?;

    let detections_port = normalize_port_name(request.detections_port.as_deref()).unwrap_or_else(|| "detections".into());

    let mut warnings: Vec<String> = Vec::new();
    let mut include_overlays = request.include_overlays;
    let overlay_mode = overlay_save_mode_from_request(&request);
    let overlay_dir = request.overlay_output_dir.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(std::path::PathBuf::from);
    if include_overlays && overlay_dir.is_none() {
        warnings.push("overlay output dir not provided; skipping overlays".into());
        include_overlays = false;
    }
    // Always use the calibration graph's detections so the solver exactly matches the guided
    // calibration overlay settings (same quads, same decode knobs, same dictionary).
    let use_fast_points = false;
    let use_fast_detection = false;
    tracing::info!(images = request.images.len(), include_overlays, use_fast_detection, ?overlay_mode, "calibration: solve start");
    let graph_needed = include_overlays || !use_fast_points;
    let graph = if !graph_needed {
        None
    } else {
        let graph_json = load_graph_json(&request)?;
        // We generate overlays ourselves from the input frame + detections, so the graph does not
        // need to expose a dedicated overlay output port.
        let graph = crate::graph::GraphHandle::from_json(2, &graph_json).map_err(|err| CalibrationSolveFailure::InvalidInput(compact_calibration_graph_error(&err.to_string())))?;
        Some(graph)
    };
    let graph_output_ports = graph.as_ref().and_then(|g| g.host_output_ports()).unwrap_or_default();
    if graph_needed && !graph_output_ports.iter().any(|port| port.eq_ignore_ascii_case(&detections_port)) {
        warnings.push(format!("calibration graph missing requested detections port '{detections_port}' (available: [{}])", graph_output_ports.join(", ")));
    }
    let layout_even_row = BoardLayout::new(&request.board, BoardParity::EvenSquares, BoardOrdering::RowMajor);
    let layout_odd_row = BoardLayout::new(&request.board, BoardParity::OddSquares, BoardOrdering::RowMajor);
    let layout_even_col = BoardLayout::new(&request.board, BoardParity::EvenSquares, BoardOrdering::ColMajor);
    let layout_odd_col = BoardLayout::new(&request.board, BoardParity::OddSquares, BoardOrdering::ColMajor);
    let expected_ids_even: HashSet<u32> = layout_even_row.tag_corners_by_id.keys().copied().collect();
    let expected_ids_odd: HashSet<u32> = layout_odd_row.tag_corners_by_id.keys().copied().collect();

    let total_images = request.images.len();
    let mut debug_entries: Vec<DebugEntry> = Vec::with_capacity(total_images);
    let mut image_size_hint: Option<(u32, u32)> = None;
    // Used to force a correct board parity when the observed ID range makes one layout impossible.
    // This avoids "best score" ties selecting the wrong parity when most tags are near the center.
    let mut observed_max_id: Option<u32> = None;
    let mut overlay_seq = 0usize;
    let overlay_ext = "jpg";
    let min_points = request.config.min_points_per_view as usize;
    // Marker-corner views contribute 4 points per tag; center views contribute 1. If we require
    // the same raw point-count for both, the center solve drops otherwise-good far views.
    let min_center_points = min_points.div_ceil(2);

    for (index, image) in request.images.into_iter().enumerate() {
        let image_start = Instant::now();
        let path = image.path.trim();
        if path.is_empty() {
            warnings.push("skip image: empty path".into());
            debug_entries.push(DebugEntry::Skipped(empty_debug_view("<unknown>".into(), None)));
            continue;
        }
        let display_name = image.name.clone().unwrap_or_else(|| std::path::Path::new(path).file_name().and_then(|name| name.to_str()).map(str::to_string).unwrap_or_else(|| path.to_string()));

        // Never treat generated overlays as calibration inputs. If overlays are written into the
        // same directory as snapshots, they can get picked up by "select all" and recursively
        // re-encoded, which looks like random white dashes and destroys calibration.
        let lower = display_name.to_ascii_lowercase();
        if lower.contains("_overlay_") {
            warnings.push(format!("skip {display_name}: overlay image is not a calibration snapshot"));
            debug_entries.push(DebugEntry::Skipped(empty_debug_view(display_name, None)));
            continue;
        }
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                warnings.push(format!("skip {display_name}: read failed ({err})"));
                debug_entries.push(DebugEntry::Skipped(empty_debug_view(display_name, None)));
                continue;
            }
        };
        let dyn_img = match decode_snapshot_image(path, &bytes) {
            Ok(img) => img,
            Err(err) => {
                warnings.push(format!("skip {display_name}: decode failed ({err})"));
                debug_entries.push(DebugEntry::Skipped(empty_debug_view(display_name, None)));
                continue;
            }
        };
        // Keep a copy for overlay passthrough mode. This is intentionally after decode so we
        // compare apples-to-apples with graph outputs (same decoded pixels).
        //
        // NOTE: We write passthrough overlays *before* running the graph so we can bisect any
        // potential memory corruption in the CV pipeline.
        let overlay_input_img = if include_overlays && overlay_mode == OverlaySaveMode::Input { Some(dyn_img.clone()) } else { None };
        // Keep a grayscale copy for ChArUco corner refinement. This lets us match OpenCV-style
        // interpolation (cornerSubPix) much more closely than pure geometric projection.
        let gray = dyn_img.to_luma8();
        let (width, height) = (dyn_img.width(), dyn_img.height());
        if image_size_hint.is_none() {
            image_size_hint = Some((width, height));
        }
        let mut overlay_path: Option<String> = None;
        if include_overlays && overlay_mode == OverlaySaveMode::Copy {
            overlay_path = match overlay_dir.as_ref() {
                Some(dir) => {
                    if let Err(err) = std::fs::create_dir_all(dir) {
                        warnings.push(format!("overlay output dir unavailable ({err}); skipping overlays"));
                        include_overlays = false;
                        None
                    } else {
                        overlay_seq = overlay_seq.saturating_add(1);
                        let filename = build_overlay_filename(&display_name, overlay_seq, overlay_ext);
                        let path = dir.join(&filename);
                        match std::fs::write(&path, &bytes) {
                            Ok(()) => Some(filename),
                            Err(err) => {
                                warnings.push(format!("failed to write overlay {filename} ({err})"));
                                None
                            }
                        }
                    }
                }
                None => {
                    include_overlays = false;
                    None
                }
            };
        }

        if include_overlays && overlay_mode == OverlaySaveMode::Input {
            overlay_path = match overlay_dir.as_ref() {
                Some(dir) => {
                    if let Err(err) = std::fs::create_dir_all(dir) {
                        warnings.push(format!("overlay output dir unavailable ({err}); skipping overlays"));
                        include_overlays = false;
                        None
                    } else {
                        overlay_seq = overlay_seq.saturating_add(1);
                        let filename = build_overlay_filename(&display_name, overlay_seq, overlay_ext);
                        let path = dir.join(&filename);
                        match overlay_input_img.and_then(encode_overlay_jpeg) {
                            Some(bytes) => match std::fs::write(&path, bytes) {
                                Ok(()) => Some(filename),
                                Err(err) => {
                                    warnings.push(format!("failed to write overlay {filename} ({err})"));
                                    None
                                }
                            },
                            None => None,
                        }
                    }
                }
                None => {
                    include_overlays = false;
                    None
                }
            };
        }

        // Keep a clean base frame around for overlay generation. We intentionally do *not*
        // persist the graph's overlay output; it has regressed into corrupted frames under
        // some runtime configurations (white dash artifacts).
        let overlay_base = if include_overlays && overlay_mode == OverlaySaveMode::Graph { Some(dyn_img.clone()) } else { None };

        let graph = graph.as_ref().expect("calibration graph missing");
        // Some runtime plans materialize host-output samples one tick later. Run one extra pass
        // before reading detections so per-image calibration solves don't miss outputs.
        let _frame = graph.process(dyn_img.clone());
        let mut raw = graph.sample_value_output(&detections_port);
        if raw.is_none() {
            let _ = graph.process(dyn_img.clone());
            raw = graph.sample_value_output(&detections_port);
        }
        let parsed = if let Some(raw) = raw {
            match parse_typed_detections(raw) {
                Ok(parsed) => Ok(parsed),
                Err(value_err) => {
                    if let Some(json) = graph.sample_json_output(&detections_port) {
                        parse_json_detections(&json).map_err(|json_err| format!("{value_err}; json fallback failed ({json_err})"))
                    } else {
                        Err(value_err)
                    }
                }
            }
        } else if let Some(json) = graph.sample_json_output(&detections_port) {
            parse_json_detections(&json)
        } else {
            Err(format!("missing detections output '{detections_port}' (available: [{}])", graph_output_ports.join(", ")))
        };
        let parsed = match parsed {
            Ok(parsed) => parsed,
            Err(err) => {
                warnings.push(format!("skip {display_name}: detections parse failed ({err})"));
                debug_entries.push(DebugEntry::Skipped(empty_debug_view(display_name, overlay_path.clone())));
                continue;
            }
        };
        if parsed.dropped > 0 {
            warnings.push(format!("{display_name}: dropped {} invalid detections", parsed.dropped));
        }
        let detections = parsed.detections;

        let detections_len = detections.len();
        for det in &detections {
            observed_max_id = Some(observed_max_id.unwrap_or(det.id).max(det.id));
        }

        if include_overlays && overlay_mode == OverlaySaveMode::Graph {
            overlay_path = match (overlay_dir.as_ref(), overlay_base) {
                (Some(dir), Some(mut base)) => {
                    if let Err(err) = std::fs::create_dir_all(dir) {
                        warnings.push(format!("overlay output dir unavailable ({err}); skipping overlays"));
                        include_overlays = false;
                        None
                    } else {
                        overlay_seq = overlay_seq.saturating_add(1);
                        let filename = build_overlay_filename(&display_name, overlay_seq, overlay_ext);
                        let path = dir.join(&filename);
                        draw_calibration_overlay(&mut base, &detections);
                        match encode_overlay_jpeg(base) {
                            Some(bytes) => match std::fs::write(&path, bytes) {
                                Ok(()) => Some(filename),
                                Err(err) => {
                                    warnings.push(format!("failed to write overlay {filename} ({err})"));
                                    None
                                }
                            },
                            None => None,
                        }
                    }
                }
                _ => None,
            };
        }

        debug_entries.push(DebugEntry::PendingImage(ImageDetections { image: display_name, overlay_path, detections, gray, width, height }));
        tracing::info!(image_index = index + 1, image_total = total_images, detections = detections_len, elapsed_ms = image_start.elapsed().as_millis(), "calibration: image processed");
    }

    let mut score_row_even = LayoutScore::default();
    let mut score_row_odd = LayoutScore::default();
    let mut score_col_even = LayoutScore::default();
    let mut score_col_odd = LayoutScore::default();
    let (min_tag_area_even, min_tag_area_odd) = if let Some((width, height)) = image_size_hint {
        let abs_min = calibration_min_tag_area_px(width, height);
        let min_views = request.config.min_views as usize;
        let stats_even = tag_area_stats(&debug_entries, &layout_even_row, abs_min, min_points, min_views);
        let stats_odd = tag_area_stats(&debug_entries, &layout_odd_row, abs_min, min_points, min_views);
        (stats_even, stats_odd)
    } else {
        (TagAreaStats::default(), TagAreaStats::default())
    };

    // Use the ChArUco chessboard corner refinement pass for all lens models.
    // For fisheye lenses, the global homography is not exact, but the code below
    // also falls back to local homographies around each corner, which is still
    // a useful signal and matches competitor behavior (OpenCV supports fisheye + ChArUco).
    //
    // Can be disabled for debugging via `HELIOS_CALIBRATION_DISABLE_CHARUCO=1`.
    let allow_charuco = !std::env::var("HELIOS_CALIBRATION_DISABLE_CHARUCO").ok().map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes")).unwrap_or(false);
    let mut processed_entries: Vec<DebugEntry> = Vec::with_capacity(debug_entries.len());
    for entry in debug_entries {
        match entry {
            DebugEntry::Skipped(view) => processed_entries.push(DebugEntry::Skipped(view)),
            DebugEntry::PendingImage(image) => {
                let row_even = build_view(
                    &layout_even_row,
                    &image.detections,
                    BuildViewParams {
                        width: image.width,
                        height: image.height,
                        expected_ids: &expected_ids_even,
                        gray: Some(&image.gray),
                        min_points,
                        min_tag_area: min_tag_area_even.min_area,
                        allow_charuco,
                        lens_model: request.config.lens_model,
                    },
                );
                let row_odd = build_view(
                    &layout_odd_row,
                    &image.detections,
                    BuildViewParams {
                        width: image.width,
                        height: image.height,
                        expected_ids: &expected_ids_odd,
                        gray: Some(&image.gray),
                        min_points,
                        min_tag_area: min_tag_area_odd.min_area,
                        allow_charuco,
                        lens_model: request.config.lens_model,
                    },
                );
                let col_even = build_view(
                    &layout_even_col,
                    &image.detections,
                    BuildViewParams {
                        width: image.width,
                        height: image.height,
                        expected_ids: &expected_ids_even,
                        gray: Some(&image.gray),
                        min_points,
                        min_tag_area: min_tag_area_even.min_area,
                        allow_charuco,
                        lens_model: request.config.lens_model,
                    },
                );
                let col_odd = build_view(
                    &layout_odd_col,
                    &image.detections,
                    BuildViewParams {
                        width: image.width,
                        height: image.height,
                        expected_ids: &expected_ids_odd,
                        gray: Some(&image.gray),
                        min_points,
                        min_tag_area: min_tag_area_odd.min_area,
                        allow_charuco,
                        lens_model: request.config.lens_model,
                    },
                );
                score_row_even.update(&row_even, min_points);
                score_row_odd.update(&row_odd, min_points);
                score_col_even.update(&col_even, min_points);
                score_col_odd.update(&col_odd, min_points);
                processed_entries.push(DebugEntry::Pending(Box::new(ImageCandidate { image: image.image, overlay_path: image.overlay_path, row_even, row_odd, col_even, col_odd })));
            }
            DebugEntry::Pending(candidate) => {
                processed_entries.push(DebugEntry::Pending(candidate));
            }
        }
    }

    let debug_entries = processed_entries;

    let _min_views = request.config.min_views as usize;

    // Pick the board layout (ordering + parity) that best fits the detections.
    //
    // NOTE: A planar homography is a stronger discriminator for pinhole lenses than fisheye,
    // but in practice the wrong layout still yields much worse alignment and far fewer usable
    // views. We therefore pick a single best layout for all lens models and let the downstream
    // reprojection error decide between Charuco/marker/center solve modes.
    #[derive(Clone, Copy)]
    struct Candidate {
        ordering: BoardOrdering,
        parity: BoardParity,
        usable_views: usize,
        mean_align_err: f64,
    }

    fn mean_alignment_error(score: &LayoutScore) -> f64 {
        if score.alignment_error_count == 0 {
            return f64::INFINITY;
        }
        score.alignment_error_sum / (score.alignment_error_count as f64)
    }

    let mut candidates = [
        Candidate { ordering: BoardOrdering::RowMajor, parity: BoardParity::EvenSquares, usable_views: score_row_even.usable_views, mean_align_err: mean_alignment_error(&score_row_even) },
        Candidate { ordering: BoardOrdering::RowMajor, parity: BoardParity::OddSquares, usable_views: score_row_odd.usable_views, mean_align_err: mean_alignment_error(&score_row_odd) },
        Candidate { ordering: BoardOrdering::ColMajor, parity: BoardParity::EvenSquares, usable_views: score_col_even.usable_views, mean_align_err: mean_alignment_error(&score_col_even) },
        Candidate { ordering: BoardOrdering::ColMajor, parity: BoardParity::OddSquares, usable_views: score_col_odd.usable_views, mean_align_err: mean_alignment_error(&score_col_odd) },
    ];

    candidates.sort_by(|a, b| b.usable_views.cmp(&a.usable_views).then_with(|| a.mean_align_err.total_cmp(&b.mean_align_err)));

    let best = candidates[0];
    warnings.push(format!("calibration: selected layout {:?}/{:?} (usable views: {}; mean align err: {:.2} px^2)", best.ordering, best.parity, best.usable_views, best.mean_align_err));
    let (mut chosen_ordering, mut chosen_parity) = (best.ordering, best.parity);

    // If the dataset contains marker IDs that exceed the odd-parity board capacity, the board must
    // be the even-parity layout. This matches our generator and prevents parity mis-selection.
    let max_odd_id = layout_odd_row.tag_corners_by_id.len().saturating_sub(1) as u32;
    if let Some(max_id) = observed_max_id {
        if max_id > max_odd_id {
            chosen_parity = BoardParity::EvenSquares;
            warnings.push(format!("calibration: forced parity {:?} (observed max id {} > odd max id {})", chosen_parity, max_id, max_odd_id));
        }
    }

    if let Ok(raw) = std::env::var("HELIOS_CALIBRATION_FORCE_PARITY") {
        match raw.to_ascii_lowercase().as_str() {
            "odd" => chosen_parity = BoardParity::OddSquares,
            "even" => chosen_parity = BoardParity::EvenSquares,
            _ => {}
        }
    }
    if let Ok(raw) = std::env::var("HELIOS_CALIBRATION_FORCE_ORDERING") {
        match raw.to_ascii_lowercase().as_str() {
            "row" | "rowmajor" => chosen_ordering = BoardOrdering::RowMajor,
            "col" | "colmajor" => chosen_ordering = BoardOrdering::ColMajor,
            _ => {}
        }
    }

    let chosen_area_stats = match chosen_parity {
        BoardParity::EvenSquares => min_tag_area_even,
        BoardParity::OddSquares => min_tag_area_odd,
    };
    if let Some(median) = chosen_area_stats.median {
        if chosen_area_stats.min_area > 0.0 {
            warnings.push(format!("calibration: tag area median {:.1}px^2 (n={}); min tag area {:.1}px^2", median, chosen_area_stats.count, chosen_area_stats.min_area));
        }
    }

    let debug_entries_len = debug_entries.len();
    let mut debug_slots: Vec<Option<CalibrationSolveDebugView>> = vec![None; debug_entries_len];
    let mut candidates: Vec<(usize, String, Option<String>, BuildViewResult)> = Vec::new();
    let mut views_charuco: Vec<CalibrationView> = Vec::new();
    let mut views_marker: Vec<CalibrationView> = Vec::new();
    let mut views_center: Vec<CalibrationView> = Vec::new();
    for (index, entry) in debug_entries.into_iter().enumerate() {
        match entry {
            DebugEntry::Skipped(view) => {
                debug_slots[index] = Some(view);
            }
            DebugEntry::Pending(candidate) => {
                let chosen = match (chosen_ordering, chosen_parity) {
                    (BoardOrdering::RowMajor, BoardParity::EvenSquares) => candidate.row_even,
                    (BoardOrdering::RowMajor, BoardParity::OddSquares) => candidate.row_odd,
                    (BoardOrdering::ColMajor, BoardParity::EvenSquares) => candidate.col_even,
                    (BoardOrdering::ColMajor, BoardParity::OddSquares) => candidate.col_odd,
                };
                if chosen.charuco_used && chosen.view.len() >= min_points {
                    views_charuco.push(chosen.view.clone());
                }
                if chosen.marker_view.len() >= min_points {
                    views_marker.push(chosen.marker_view.clone());
                }
                if chosen.center_view.len() >= min_center_points {
                    views_center.push(chosen.center_view.clone());
                }
                candidates.push((index, candidate.image, candidate.overlay_path, chosen));
            }
            DebugEntry::PendingImage(image) => {
                warnings.push(format!("skip {}: pending calibration image missing views", image.image));
                debug_slots[index] = Some(empty_debug_view(image.image, image.overlay_path));
            }
        }
    }

    let cv_config = map_config(&request.config);
    let map_cv_error = |err| match err {
        lib_cv::calibration::CalibrationSolveError::InsufficientViews { required, provided } => CalibrationSolveFailure::InvalidInput(build_view_error(provided, required, &warnings)),
        lib_cv::calibration::CalibrationSolveError::InsufficientPoints { required, provided } => {
            let suffix = summarize_warnings(&warnings);
            let reason = format!("not enough points in a view ({provided} < {required})");
            CalibrationSolveFailure::InvalidInput(append_suffix(reason, suffix))
        }
        lib_cv::calibration::CalibrationSolveError::DegenerateHomography => CalibrationSolveFailure::InvalidInput("degenerate homography; capture more varied angles".into()),
        lib_cv::calibration::CalibrationSolveError::IntrinsicsSolveFailed => CalibrationSolveFailure::InvalidInput("intrinsics solve failed; capture more coverage".into()),
        lib_cv::calibration::CalibrationSolveError::ExtrinsicsSolveFailed => CalibrationSolveFailure::InvalidInput("extrinsics solve failed; capture more views".into()),
    };
    // Provide a weak, resolution-derived intrinsics hint so fisheye calibration has a sane
    // principal point and focal seed, without relying on any user-provided FOV priors.
    let hint = image_size_hint.map(|(w, h)| {
        let cx = (w as f64) * 0.5;
        let cy = (h as f64) * 0.5;
        // OpenCV's fisheye calibrator often seeds fx/fy to ~0.5*width; this is just a starting point.
        let f = (w as f64) * 0.5;
        CameraIntrinsics::new(f, f, cx, cy)
    });
    let solve_with_views = |views: &[CalibrationView], cfg: CvSolveConfig| solve_camera_intrinsics_with_hint(views, cfg, hint).map_err(map_cv_error);
    let solve_with_views_hint_intrinsics = |views: &[CalibrationView], cfg: CvSolveConfig, intr: CameraIntrinsics| solve_camera_intrinsics_with_hint(views, cfg, Some(intr)).map_err(map_cv_error);

    let charuco_result = if views_charuco.len() >= cv_config.min_views {
        solve_with_views(&views_charuco, cv_config)
    } else {
        Err(CalibrationSolveFailure::InvalidInput(build_view_error(views_charuco.len(), cv_config.min_views, &warnings)))
    };

    let mut cv_config_center = cv_config;
    cv_config_center.min_points_per_view = min_center_points;
    let center_result = if views_center.len() >= cv_config_center.min_views {
        solve_with_views(&views_center, cv_config_center)
    } else {
        Err(CalibrationSolveFailure::InvalidInput(build_view_error(views_center.len(), cv_config_center.min_views, &warnings)))
    };

    // Marker-corner solve is sensitive to corner noise. If we have a reasonable center solve,
    // rerun marker solve with the center intrinsics as a hint to improve convergence.
    let marker_result = if views_marker.len() >= cv_config.min_views {
        let base = solve_with_views(&views_marker, cv_config);
        match (&base, &center_result) {
            (Ok(_), Ok(center)) => {
                let hinted = solve_with_views_hint_intrinsics(&views_marker, cv_config, center.calibration.intrinsics);
                if let Ok(hinted_ok) = &hinted {
                    warnings.push(format!(
                        "calibration: marker solve rerun with center intrinsics hint (reproj {:.2} px -> {:.2} px)",
                        base.as_ref().map(|r| r.reprojection_error_px).unwrap_or(f64::NAN),
                        hinted_ok.reprojection_error_px
                    ));
                }
                match (base, hinted) {
                    (Ok(b), Ok(h)) => {
                        if h.reprojection_error_px.is_finite() && (!b.reprojection_error_px.is_finite() || h.reprojection_error_px < b.reprojection_error_px) {
                            Ok(h)
                        } else {
                            Ok(b)
                        }
                    }
                    (Err(_), Ok(h)) => Ok(h),
                    (other, _) => other,
                }
            }
            _ => base,
        }
    } else {
        Err(CalibrationSolveFailure::InvalidInput(build_view_error(views_marker.len(), cv_config.min_views, &warnings)))
    };

    #[derive(Clone, Copy)]
    enum SolveMode {
        Charuco,
        Marker,
        Center,
    }

    let (result, solve_mode) = match (charuco_result, marker_result, center_result) {
        (Ok(charuco), Ok(marker), Ok(center)) => {
            let ce = charuco.reprojection_error_px;
            let me = marker.reprojection_error_px;
            let cne = center.reprojection_error_px;

            // Pick the numerically best result among available solve modes.
            let mut best = (ce, SolveMode::Charuco);
            if me.is_finite() && (!best.0.is_finite() || me < best.0) {
                best = (me, SolveMode::Marker);
            }
            if cne.is_finite() && (!best.0.is_finite() || cne < best.0) {
                best = (cne, SolveMode::Center);
            }

            match best.1 {
                SolveMode::Charuco => {
                    warnings.push(format!("calibration: selected charuco solve (reproj {:.2} px vs marker {:.2} px, center {:.2} px)", ce, me, cne));
                    (charuco, SolveMode::Charuco)
                }
                SolveMode::Marker => {
                    warnings.push(format!("calibration: selected marker solve (reproj {:.2} px vs charuco {:.2} px, center {:.2} px)", me, ce, cne));
                    (marker, SolveMode::Marker)
                }
                SolveMode::Center => {
                    warnings.push(format!("calibration: selected center solve (reproj {:.2} px vs charuco {:.2} px, marker {:.2} px)", cne, ce, me));
                    (center, SolveMode::Center)
                }
            }
        }
        (Ok(charuco), Ok(marker), Err(err_center)) => {
            warnings.push(format!("calibration: center solve unavailable ({})", err_center.reason()));
            let ce = charuco.reprojection_error_px;
            let me = marker.reprojection_error_px;
            if ce.is_finite() && (!me.is_finite() || ce <= me) {
                warnings.push(format!("calibration: selected charuco solve (reproj {:.2} px vs marker {:.2} px)", ce, me));
                (charuco, SolveMode::Charuco)
            } else {
                warnings.push(format!("calibration: selected marker solve (reproj {:.2} px vs charuco {:.2} px)", me, ce));
                (marker, SolveMode::Marker)
            }
        }
        (Ok(charuco), Err(err_marker), Ok(center)) => {
            warnings.push(format!("calibration: marker solve unavailable ({})", err_marker.reason()));
            let ce = charuco.reprojection_error_px;
            let cne = center.reprojection_error_px;
            if ce.is_finite() && (!cne.is_finite() || ce <= cne) {
                warnings.push(format!("calibration: selected charuco solve (reproj {:.2} px vs center {:.2} px)", ce, cne));
                (charuco, SolveMode::Charuco)
            } else {
                warnings.push(format!("calibration: selected center solve (reproj {:.2} px vs charuco {:.2} px)", cne, ce));
                (center, SolveMode::Center)
            }
        }
        (Err(err_charuco), Ok(marker), Ok(center)) => {
            warnings.push(format!("calibration: charuco solve unavailable ({})", err_charuco.reason()));
            let me = marker.reprojection_error_px;
            let cne = center.reprojection_error_px;
            if me.is_finite() && (!cne.is_finite() || me <= cne) {
                warnings.push(format!("calibration: selected marker solve (reproj {:.2} px vs center {:.2} px)", me, cne));
                (marker, SolveMode::Marker)
            } else {
                warnings.push(format!("calibration: selected center solve (reproj {:.2} px vs marker {:.2} px)", cne, me));
                (center, SolveMode::Center)
            }
        }
        (Ok(charuco), Err(err_marker), Err(err_center)) => {
            warnings.push(format!("calibration: marker solve unavailable ({})", err_marker.reason()));
            warnings.push(format!("calibration: center solve unavailable ({})", err_center.reason()));
            warnings.push(format!("calibration: selected charuco solve (reproj {:.2} px)", charuco.reprojection_error_px));
            (charuco, SolveMode::Charuco)
        }
        (Err(err_charuco), Ok(marker), Err(err_center)) => {
            warnings.push(format!("calibration: charuco solve unavailable ({})", err_charuco.reason()));
            warnings.push(format!("calibration: center solve unavailable ({})", err_center.reason()));
            warnings.push(format!("calibration: selected marker solve (reproj {:.2} px)", marker.reprojection_error_px));
            (marker, SolveMode::Marker)
        }
        (Err(err_charuco), Err(err_marker), Ok(center)) => {
            warnings.push(format!("calibration: marker solve unavailable ({})", err_marker.reason()));
            warnings.push(format!("calibration: charuco solve unavailable ({})", err_charuco.reason()));
            warnings.push(format!("calibration: selected center solve (reproj {:.2} px)", center.reprojection_error_px));
            (center, SolveMode::Center)
        }
        (Err(err_charuco), Err(err_marker), Err(err_center)) => {
            warnings.push(format!("calibration: marker solve unavailable ({})", err_marker.reason()));
            warnings.push(format!("calibration: center solve unavailable ({})", err_center.reason()));
            warnings.push(format!("calibration: charuco solve unavailable ({})", err_charuco.reason()));
            return Err(err_charuco);
        }
    };

    for (index, image, overlay_path, chosen) in candidates {
        let (used, reason, points_detected) = match solve_mode {
            SolveMode::Charuco => {
                let used = chosen.charuco_used && chosen.view.len() >= min_points;
                let reason = if !used {
                    if let Some(reason) = &chosen.charuco_reason {
                        Some(reason.clone())
                    } else {
                        Some(format!("insufficient points ({} < {})", chosen.view.len(), min_points))
                    }
                } else {
                    None
                };
                (used, reason, chosen.stats.points_detected)
            }
            SolveMode::Marker => {
                let count = chosen.marker_view.len();
                let used = count >= min_points;
                let reason = if !used { Some(format!("marker: insufficient points ({count} < {min_points})")) } else { None };
                (used, reason, (count.min(u32::MAX as usize) as u32))
            }
            SolveMode::Center => {
                let count = chosen.center_view.len();
                let used = count >= min_center_points;
                let reason = if !used { Some(format!("center: insufficient points ({count} < {min_center_points})")) } else { None };
                (used, reason, (count.min(u32::MAX as usize) as u32))
            }
        };
        if !used {
            if let Some(reason) = reason {
                warnings.push(format!("skip {image}: {reason}"));
            }
        }
        debug_slots[index] = Some(CalibrationSolveDebugView {
            image,
            tags_detected: chosen.stats.tags_detected,
            points_detected,
            used,
            coverage_ratio: chosen.stats.coverage_ratio,
            raw_tags_detected: chosen.stats.raw_tags_detected,
            raw_ids: chosen.stats.raw_ids,
            raw_duplicate_ids: chosen.stats.raw_duplicate_ids,
            raw_out_of_range_ids: chosen.stats.raw_out_of_range_ids,
            overlay_path,
        });
    }

    let debug_views: Vec<CalibrationSolveDebugView> = debug_slots.into_iter().flatten().collect();

    warnings.extend(result.warnings.iter().cloned());

    let calib = result.calibration;
    let reprojection_error_px = if result.reprojection_error_px.is_finite() { result.reprojection_error_px } else { 0.0 };
    let response = CalibrationSolveResponse {
        calibration: StreamCalibration {
            fx: calib.intrinsics.fx,
            fy: calib.intrinsics.fy,
            cx: calib.intrinsics.cx,
            cy: calib.intrinsics.cy,
            k1: calib.distortion.k1,
            k2: calib.distortion.k2,
            p1: calib.distortion.p1,
            p2: calib.distortion.p2,
            k3: calib.distortion.k3,
            undistort_iters: calib.undistort_iters as i64,
            lens_model: calib.lens_model,
        },
        reprojection_error_px,
        views_used: result.views_used.min(u32::MAX as usize) as u32,
        points_used: result.points_used.min(u32::MAX as usize) as u32,
        warnings,
        debug_views,
    };

    tracing::info!(
        elapsed_ms = solve_start.elapsed().as_millis(),
        views_used = response.views_used,
        points_used = response.points_used,
        reprojection_error_px = response.reprojection_error_px,
        "calibration: solve complete"
    );

    Ok(response)
}

fn map_config(cfg: &CalibrationSolveConfig) -> CvSolveConfig {
    CvSolveConfig {
        min_views: cfg.min_views.max(1) as usize,
        min_points_per_view: cfg.min_points_per_view.max(1) as usize,
        refine_distortion: cfg.refine_distortion,
        undistort_iters: cfg.undistort_iters,
        refine_undistort_iters: cfg.refine_undistort_iters,
        lens_model: cfg.lens_model,
    }
}

fn load_graph_json(request: &CalibrationSolveRequest) -> Result<JsonValue, CalibrationSolveFailure> {
    let dictionary_override = request
        .board
        .dictionary
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|value| normalize_calibration_dictionary_name(value).ok_or_else(|| CalibrationSolveFailure::InvalidInput(format!("unknown board dictionary '{value}'"))))
        .transpose()?;
    let max_board_id = board_marker_capacity(&request.board).saturating_sub(1);

    if let Some(graph) = request.graph.as_ref() {
        let mut graph = graph.0.clone();
        if let Some(dictionary) = dictionary_override.as_deref() {
            patch_dictionary_const(&mut graph, dictionary);
        }
        patch_id_range_consts(&mut graph, 0, max_board_id as i64);
        return Ok(graph);
    }
    if let Some(graph_id) = request.graph_id {
        let mut graph = crate::pipelines::load_pipeline_graph_json(graph_id).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                CalibrationSolveFailure::NotFound(format!("graph {graph_id} not found"))
            } else {
                CalibrationSolveFailure::Internal(format!("failed to load graph {graph_id}: {err}"))
            }
        })?;
        if let Some(dictionary) = dictionary_override.as_deref() {
            patch_dictionary_const(&mut graph, dictionary);
        }
        patch_id_range_consts(&mut graph, 0, max_board_id as i64);
        return Ok(graph);
    }
    if let Some(template_id) = request.graph_template_id.as_deref() {
        let mut graph = crate::pipelines::load_template_graph_json(template_id).map_err(|err| CalibrationSolveFailure::Internal(format!("failed to load template {template_id}: {err}")))?;
        if let Some(dictionary) = dictionary_override.as_deref() {
            patch_dictionary_const(&mut graph, dictionary);
        }
        patch_id_range_consts(&mut graph, 0, max_board_id as i64);
        return Ok(graph);
    }
    Err(CalibrationSolveFailure::InvalidInput("calibration graph missing (graph, graph_id, or graph_template_id required)".into()))
}

fn compact_calibration_graph_error(raw: &str) -> String {
    if !raw.contains("planner diagnostics") || !raw.contains("graph is missing input port `") {
        return format!("invalid calibration graph: {raw}");
    }

    let mut by_node: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let needle = "graph is missing input port `";
    let mut rest = raw;
    while let Some(idx) = rest.find(needle) {
        rest = &rest[idx + needle.len()..];
        let Some(port_end) = rest.find('`') else {
            break;
        };
        let port = rest[..port_end].trim();
        let after_port = &rest[port_end + 1..];

        let node = after_port
            .find(" on node ")
            .map(|node_idx| {
                let after_node = &after_port[node_idx + " on node ".len()..];
                let end = after_node.find([' ', '(', ',', '"']).unwrap_or(after_node.len());
                after_node[..end].trim()
            })
            .unwrap_or_default();

        if !port.is_empty() && !node.is_empty() {
            let entry = by_node.entry(node.to_string()).or_default();
            if !entry.iter().any(|existing| existing == port) {
                entry.push(port.to_string());
            }
        }

        rest = after_port;
    }

    if by_node.is_empty() {
        return "invalid calibration graph: stale node port declarations; regenerate graph ports from registry".to_string();
    }

    let summary = by_node
        .into_iter()
        .map(|(node, mut ports)| {
            ports.sort();
            let missing_total = ports.len();
            let mut sample = ports;
            sample.truncate(3);
            if missing_total > sample.len() {
                format!("{node} missing {missing_total} (e.g. {})", sample.join(", "))
            } else {
                format!("{node} missing {missing_total} ({})", sample.join(", "))
            }
        })
        .collect::<Vec<_>>()
        .join("; ");

    format!("invalid calibration graph: stale node port declarations: {summary}. Regenerate graph ports from registry.")
}

fn patch_dictionary_const(graph: &mut JsonValue, dictionary: &str) {
    let Some(nodes) = graph.get_mut("nodes").and_then(|v| v.as_array_mut()) else {
        return;
    };
    for node in nodes {
        let Some(id) = node.get("id").and_then(|v| v.as_str()) else { continue };
        if !id.starts_with("cv:aruco:") {
            continue;
        }
        let accepts_dictionary =
            node.get("inputs").and_then(|v| v.as_array()).is_some_and(|inputs| inputs.iter().any(|name| name.as_str().is_some_and(|value| value.eq_ignore_ascii_case("dictionary"))));
        if !accepts_dictionary {
            continue;
        }
        let consts = match node.get_mut("const_inputs").and_then(|v| v.as_array_mut()) {
            Some(arr) => arr,
            None => {
                let Some(obj) = node.as_object_mut() else { continue };
                obj.insert("const_inputs".into(), JsonValue::Array(Vec::new()));
                match obj.get_mut("const_inputs").and_then(|v| v.as_array_mut()) {
                    Some(arr) => arr,
                    None => continue,
                }
            }
        };
        let mut found = false;
        for entry in consts.iter_mut() {
            let Some(pair) = entry.as_array_mut() else { continue };
            if pair.len() != 2 {
                continue;
            }
            if pair[0].as_str() == Some("dictionary") {
                pair[1] = serde_json::json!({ "type": "String", "value": dictionary });
                found = true;
                break;
            }
        }
        if !found {
            consts.push(serde_json::json!(["dictionary", { "type": "String", "value": dictionary }]));
        }
    }
}

fn patch_id_range_consts(graph: &mut JsonValue, min_id: i64, max_id: i64) {
    let Some(nodes) = graph.get_mut("nodes").and_then(|v| v.as_array_mut()) else {
        return;
    };
    for node in nodes {
        let Some(id) = node.get("id").and_then(|v| v.as_str()) else { continue };
        if !id.starts_with("cv:aruco:") {
            continue;
        }
        let has_min = node.get("inputs").and_then(|v| v.as_array()).is_some_and(|inputs| inputs.iter().any(|name| name.as_str().is_some_and(|value| value.eq_ignore_ascii_case("min_id"))));
        let has_max = node.get("inputs").and_then(|v| v.as_array()).is_some_and(|inputs| inputs.iter().any(|name| name.as_str().is_some_and(|value| value.eq_ignore_ascii_case("max_id"))));
        if !has_min && !has_max {
            continue;
        }

        let consts = match node.get_mut("const_inputs").and_then(|v| v.as_array_mut()) {
            Some(arr) => arr,
            None => {
                let Some(obj) = node.as_object_mut() else { continue };
                obj.insert("const_inputs".into(), JsonValue::Array(Vec::new()));
                match obj.get_mut("const_inputs").and_then(|v| v.as_array_mut()) {
                    Some(arr) => arr,
                    None => continue,
                }
            }
        };

        if has_min {
            upsert_i64_const(consts, "min_id", min_id);
        }
        if has_max {
            upsert_i64_const(consts, "max_id", max_id);
        }
    }
}

fn upsert_i64_const(consts: &mut Vec<JsonValue>, key: &str, value: i64) {
    for entry in consts.iter_mut() {
        let Some(pair) = entry.as_array_mut() else { continue };
        if pair.len() != 2 {
            continue;
        }
        if pair[0].as_str() == Some(key) {
            pair[1] = serde_json::json!({ "type": "Int", "value": value });
            return;
        }
    }
    consts.push(serde_json::json!([key, { "type": "Int", "value": value }]));
}

fn validate_board(board: &CalibrationBoard) -> Result<(), CalibrationSolveFailure> {
    if board.squares_x < 2 || board.squares_y < 2 {
        return Err(CalibrationSolveFailure::InvalidInput("invalid board dimensions".into()));
    }
    if !board.square_size.is_finite() || board.square_size <= 0.0 {
        return Err(CalibrationSolveFailure::InvalidInput("invalid square_size".into()));
    }
    if !board.marker_size.is_finite() || board.marker_size <= 0.0 || board.marker_size >= board.square_size {
        return Err(CalibrationSolveFailure::InvalidInput("invalid marker_size".into()));
    }
    let marker_slots = board_marker_capacity(board);
    match resolve_board_dictionary(board)? {
        BoardTagFamily::Dictionary(dict) => {
            if marker_slots > dict.marker_count() {
                return Err(CalibrationSolveFailure::InvalidInput(format!("board requires {marker_slots} markers, but dictionary '{}' provides only {}", dict.name(), dict.marker_count())));
            }
        }
        BoardTagFamily::TagFamily(family) => {
            let available = family.codes().len();
            if marker_slots > available {
                return Err(CalibrationSolveFailure::InvalidInput(format!("board requires {marker_slots} markers, but tag family provides only {available}")));
            }
        }
    }
    Ok(())
}

enum BoardTagFamily {
    Dictionary(ArucoDictionary),
    TagFamily(ArucoTagFamily),
}

fn resolve_board_dictionary(board: &CalibrationBoard) -> Result<BoardTagFamily, CalibrationSolveFailure> {
    let raw = board.dictionary.as_deref().unwrap_or("4x4_1000");
    let trimmed = raw.trim();
    let effective = if trimmed.is_empty() { "4x4_1000" } else { trimmed };
    let normalized = normalize_calibration_dictionary_name(effective).unwrap_or_else(|| effective.to_ascii_lowercase());
    if let Some(dict) = aruco_dictionary_from_name(&normalized) {
        return Ok(BoardTagFamily::Dictionary(dict));
    }
    if let Some(family) = ArucoTagFamily::from_label(&normalized).or_else(|| ArucoTagFamily::from_label(effective)) {
        return Ok(BoardTagFamily::TagFamily(family));
    }
    Err(CalibrationSolveFailure::InvalidInput(format!("unknown board dictionary '{effective}'")))
}

fn normalize_calibration_dictionary_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = trimmed.to_ascii_lowercase();
    let mut candidates = vec![normalized.clone()];
    if let Some(stripped) = normalized.strip_prefix("dict_") {
        candidates.push(stripped.to_string());
    }
    if let Some(stripped) = normalized.strip_prefix("dict") {
        if stripped.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
            candidates.push(stripped.to_string());
        }
    }

    candidates.into_iter().find(|candidate| lib_cv::modules::aruco::ArucoDictionaryKind::from_str(candidate).is_ok())
}

fn board_marker_capacity(board: &CalibrationBoard) -> usize {
    // Our board generator places markers on "white" squares.
    // In `lib-cv`'s ChArUco generator, "white" is defined as (i+j)%2==1, so the marker count
    // is floor(total/2) when the board has an odd number of squares.
    let total = (board.squares_x as usize).saturating_mul(board.squares_y as usize);
    total / 2
}

struct ParsedDetections {
    detections: Vec<ArucoDetection2D>,
    dropped: usize,
}

fn parse_typed_detections(raw: DaedalusValue) -> Result<ParsedDetections, String> {
    let list = match raw {
        DaedalusValue::List(items) => items,
        DaedalusValue::Struct(fields) => match struct_field(&fields, "detections") {
            Some(DaedalusValue::List(items)) => items.clone(),
            Some(_) => return Err("detections field is not a list".into()),
            None => return Err("detections list missing".into()),
        },
        other => return Err(format!("unexpected detections output type: {other:?}")),
    };

    let mut detections = Vec::with_capacity(list.len());
    let mut dropped = 0usize;
    for item in list {
        match parse_detection(&item) {
            Ok(det) => detections.push(det),
            Err(_) => dropped += 1,
        }
    }

    Ok(ParsedDetections { detections, dropped })
}

fn parse_json_detections(raw: &serde_json::Value) -> Result<ParsedDetections, String> {
    let items = match raw {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Object(map) => map.get("detections").and_then(|v| v.as_array()).ok_or_else(|| "json detections list missing".to_string())?,
        other => return Err(format!("unexpected detections json type: {other:?}")),
    };

    let mut detections = Vec::with_capacity(items.len());
    let mut dropped = 0usize;
    for item in items {
        let Some(obj) = item.as_object() else {
            dropped = dropped.saturating_add(1);
            continue;
        };
        let Some(id) = obj.get("id").and_then(|v| v.as_u64()).and_then(|v| u32::try_from(v).ok()) else {
            dropped = dropped.saturating_add(1);
            continue;
        };
        let rotation = obj.get("rotation").and_then(|v| v.as_u64()).and_then(|v| u8::try_from(v).ok()).unwrap_or(0);
        let Some(corners_json) = obj.get("corners").and_then(|v| v.as_array()) else {
            dropped = dropped.saturating_add(1);
            continue;
        };
        if corners_json.len() != 4 {
            dropped = dropped.saturating_add(1);
            continue;
        }
        let mut corners = [Point { x: 0.0, y: 0.0 }; 4];
        let mut ok = true;
        for (idx, point_json) in corners_json.iter().enumerate() {
            let Some(point_obj) = point_json.as_object() else {
                ok = false;
                break;
            };
            let Some(x) = point_obj.get("x").and_then(|v| v.as_f64()) else {
                ok = false;
                break;
            };
            let Some(y) = point_obj.get("y").and_then(|v| v.as_f64()) else {
                ok = false;
                break;
            };
            corners[idx] = Point { x, y };
        }
        if !ok {
            dropped = dropped.saturating_add(1);
            continue;
        }
        detections.push(ArucoDetection2D {
            id,
            rotation,
            corners,
            score: None,
            best_distance: None,
            second_distance: None,
            border_mismatches: None,
            contrast_range: None,
            border_width: None,
            data_width: None,
            bits: None,
        });
    }

    Ok(ParsedDetections { detections, dropped })
}

fn parse_detection(value: &DaedalusValue) -> Result<ArucoDetection2D, String> {
    let DaedalusValue::Struct(fields) = value else {
        return Err("detection is not a struct".into());
    };
    let id = read_u32(struct_field(fields, "id").ok_or_else(|| "missing id".to_string())?).ok_or_else(|| "invalid id".to_string())?;
    let rotation = read_u8(struct_field(fields, "rotation").ok_or_else(|| "missing rotation".to_string())?).ok_or_else(|| "invalid rotation".to_string())?;
    let corners_value = struct_field(fields, "corners").ok_or_else(|| "missing corners".to_string())?;
    let corners = parse_corners(corners_value)?;

    Ok(ArucoDetection2D {
        id,
        rotation,
        corners,
        score: None,
        best_distance: None,
        second_distance: None,
        border_mismatches: None,
        contrast_range: None,
        border_width: None,
        data_width: None,
        bits: None,
    })
}

fn parse_corners(value: &DaedalusValue) -> Result<[Point; 4], String> {
    let DaedalusValue::List(items) = value else {
        return Err("corners is not a list".into());
    };
    if items.len() != 4 {
        return Err("corners must contain 4 points".into());
    }
    let mut out = [Point { x: 0.0, y: 0.0 }; 4];
    for (idx, item) in items.iter().enumerate() {
        out[idx] = parse_point(item)?;
    }
    Ok(out)
}

fn parse_point(value: &DaedalusValue) -> Result<Point, String> {
    let DaedalusValue::Struct(fields) = value else {
        return Err("corner is not a struct".into());
    };
    let x = read_f64(struct_field(fields, "x").ok_or_else(|| "missing x".to_string())?).ok_or_else(|| "invalid x".to_string())?;
    let y = read_f64(struct_field(fields, "y").ok_or_else(|| "missing y".to_string())?).ok_or_else(|| "invalid y".to_string())?;
    Ok(Point { x, y })
}

fn struct_field<'a>(fields: &'a [StructFieldValue], name: &str) -> Option<&'a DaedalusValue> {
    fields.iter().find(|field| field.name.eq_ignore_ascii_case(name)).map(|field| &field.value)
}

fn read_f64(value: &DaedalusValue) -> Option<f64> {
    match value {
        DaedalusValue::Float(v) => Some(*v),
        DaedalusValue::Int(v) => Some(*v as f64),
        _ => None,
    }
}

fn read_u32(value: &DaedalusValue) -> Option<u32> {
    match value {
        DaedalusValue::Int(v) if *v >= 0 => Some(*v as u32),
        DaedalusValue::Float(v) if v.is_finite() && *v >= 0.0 && v.fract() == 0.0 => Some(*v as u32),
        _ => None,
    }
}

fn read_u8(value: &DaedalusValue) -> Option<u8> {
    read_u32(value).and_then(|v| u8::try_from(v).ok())
}

fn overlay_jpeg_quality() -> u8 {
    std::env::var("HELIOS_CALIBRATION_OVERLAY_JPEG_QUALITY").ok().and_then(|raw| raw.parse::<u8>().ok()).unwrap_or(85).clamp(1, 100)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverlaySaveMode {
    /// Save the graph's selected preview output (default).
    Graph,
    /// Save the raw JPEG bytes as-is (no decode). Useful to bisect JPEG decoder issues.
    Copy,
    /// Save the decoded input image (decode + re-encode). Useful to bisect decode artifacts.
    Input,
}

fn overlay_save_mode_from_request(request: &CalibrationSolveRequest) -> OverlaySaveMode {
    let raw = request.overlay_save_mode.as_deref().unwrap_or_default();
    if !raw.trim().is_empty() {
        return match raw.trim().to_ascii_lowercase().as_str() {
            "copy" | "bytes" => OverlaySaveMode::Copy,
            "input" | "raw" | "decoded" | "passthrough" => OverlaySaveMode::Input,
            _ => OverlaySaveMode::Graph,
        };
    }
    overlay_save_mode()
}

fn overlay_save_mode() -> OverlaySaveMode {
    let raw = std::env::var("HELIOS_CALIBRATION_OVERLAY_SAVE_MODE").ok().unwrap_or_default();
    match raw.trim().to_ascii_lowercase().as_str() {
        "copy" | "bytes" => OverlaySaveMode::Copy,
        "input" | "raw" | "decoded" | "passthrough" => OverlaySaveMode::Input,
        _ => OverlaySaveMode::Graph,
    }
}

fn is_jpeg_path(path: &str) -> bool {
    let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    ext == "jpg" || ext == "jpeg"
}

fn decode_snapshot_image(path: &str, bytes: &[u8]) -> Result<DynamicImage, String> {
    // Prefer libjpeg via `djpeg` on the device. Some Rust JPEG decoders have produced artifacts
    // (random white dashes) on ARM in the past, which then makes overlays and detection diverge
    // from the guided view.
    if is_jpeg_path(path) {
        if let Ok(img) = decode_jpeg_djpeg(path) {
            return Ok(img);
        }
    }
    image::load_from_memory(bytes).map_err(|err| err.to_string())
}

fn decode_jpeg_djpeg(path: &str) -> Result<DynamicImage, String> {
    use std::process::Command;

    // Avoid `-nosmooth`: it makes JPEG blocking/noise show up as "random white dashes" in saved
    // overlays and can shift detection relative to typical decoders (browser/libjpeg defaults).
    let out = Command::new("djpeg").arg("-rgb").arg("-dct").arg("int").arg(path).output().map_err(|e| format!("spawn djpeg failed ({e})"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("djpeg failed ({}) {}", out.status, stderr.trim()));
    }
    parse_pnm_output(&out.stdout).ok_or_else(|| "djpeg: failed to parse PNM output".into())
}

fn parse_pnm_output(bytes: &[u8]) -> Option<DynamicImage> {
    // Minimal PNM (P6/P5) parser: supports comments and arbitrary whitespace.
    fn is_ws(b: u8) -> bool {
        matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0c)
    }

    fn next_token<'a>(buf: &'a [u8], i: &mut usize) -> Option<&'a [u8]> {
        while *i < buf.len() {
            let b = buf[*i];
            if is_ws(b) {
                *i += 1;
                continue;
            }
            if b == b'#' {
                while *i < buf.len() && buf[*i] != b'\n' {
                    *i += 1;
                }
                continue;
            }
            break;
        }
        if *i >= buf.len() {
            return None;
        }
        let start = *i;
        while *i < buf.len() && !is_ws(buf[*i]) {
            *i += 1;
        }
        Some(&buf[start..*i])
    }

    let mut i = 0usize;
    let magic = next_token(bytes, &mut i)?;
    if magic.len() != 2 || magic[0] != b'P' {
        return None;
    }
    let kind = magic[1];
    let w = std::str::from_utf8(next_token(bytes, &mut i)?).ok()?.parse::<u32>().ok()?;
    let h = std::str::from_utf8(next_token(bytes, &mut i)?).ok()?.parse::<u32>().ok()?;
    let maxv = std::str::from_utf8(next_token(bytes, &mut i)?).ok()?.parse::<u32>().ok()?;
    if w == 0 || h == 0 || maxv == 0 || maxv > 255 {
        return None;
    }
    // PNM binary raster begins after at least one whitespace delimiter after maxval.
    //
    // In theory the raster could start with bytes that look like whitespace, but in practice
    // `djpeg` can emit multiple whitespace bytes here (e.g. "\n\n"), and skipping only one
    // will misalign the entire raster and produce "random dash" corruption.
    if i >= bytes.len() || !is_ws(bytes[i]) {
        return None;
    }
    // Consume a small bounded run of whitespace delimiters.
    let mut consumed = 0usize;
    while i < bytes.len() && is_ws(bytes[i]) && consumed < 32 {
        i += 1;
        consumed += 1;
    }
    if consumed == 0 {
        return None;
    }
    let pixel_count = (w as usize).checked_mul(h as usize)?;
    match kind {
        b'6' => {
            let len = pixel_count.checked_mul(3)?;
            if i + len > bytes.len() {
                return None;
            }
            let data = bytes[i..i + len].to_vec();
            let rgb = image::RgbImage::from_raw(w, h, data)?;
            Some(DynamicImage::ImageRgb8(rgb))
        }
        b'5' => {
            let len = pixel_count;
            if i + len > bytes.len() {
                return None;
            }
            let data = bytes[i..i + len].to_vec();
            let gray = image::GrayImage::from_raw(w, h, data)?;
            Some(DynamicImage::ImageLuma8(gray))
        }
        _ => None,
    }
}

fn calibration_min_tag_area_ratio() -> f64 {
    std::env::var("HELIOS_CALIBRATION_MIN_TAG_AREA_RATIO").ok().and_then(|raw| raw.parse::<f64>().ok()).unwrap_or(0.0).clamp(0.0, 0.1)
}

fn calibration_min_tag_area_median_ratio() -> f64 {
    std::env::var("HELIOS_CALIBRATION_MIN_TAG_AREA_MEDIAN_RATIO").ok().and_then(|raw| raw.parse::<f64>().ok()).unwrap_or(0.0).clamp(0.0, 3.0)
}

fn calibration_min_tag_area_px(width: u32, height: u32) -> f64 {
    if let Ok(raw) = std::env::var("HELIOS_CALIBRATION_MIN_TAG_AREA_PX") {
        if let Ok(px) = raw.parse::<f64>() {
            return px.max(0.0);
        }
    }
    let denom = (width as f64) * (height as f64);
    if denom <= 0.0 {
        return 0.0;
    }
    calibration_min_tag_area_ratio() * denom
}

fn encode_overlay_jpeg(image: DynamicImage) -> Option<Vec<u8>> {
    encode_overlay_jpeg_rust(image)
}

fn draw_calibration_overlay(image: &mut DynamicImage, detections: &[ArucoDetection2D]) {
    // Render overlays in-process from the decoded input + detections. This avoids relying on
    // the graph's overlay image output, which has regressed into corrupted frames in the past.
    let mut rgba = image.to_rgba8();

    let cyan = Rgba([0u8, 255u8, 255u8, 255u8]);
    let orange = Rgba([255u8, 64u8, 0u8, 255u8]);

    for det in detections {
        let corners = det.corners;
        let pts: [(i32, i32); 4] = corners.map(|p| (p.x.round() as i32, p.y.round() as i32));
        for i in 0..4 {
            let a = pts[i];
            let b = pts[(i + 1) % 4];
            draw_line_thick(&mut rgba, a.0, a.1, b.0, b.1, 2, cyan);
        }
        for (x, y) in pts {
            draw_filled_circle(&mut rgba, x, y, 4, orange);
        }
    }

    *image = DynamicImage::ImageRgba8(rgba);
}

fn draw_line_thick(img: &mut RgbaImage, x0: i32, y0: i32, x1: i32, y1: i32, thickness: i32, color: Rgba<u8>) {
    // Simple Bresenham with a square brush. Enough for debug overlays without extra deps.
    let mut x0 = x0;
    let mut y0 = y0;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        draw_square(img, x0, y0, thickness, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn draw_square(img: &mut RgbaImage, x: i32, y: i32, half: i32, color: Rgba<u8>) {
    let w = img.width() as i32;
    let h = img.height() as i32;
    let r = half.max(1);
    for yy in (y - r)..=(y + r) {
        if yy < 0 || yy >= h {
            continue;
        }
        for xx in (x - r)..=(x + r) {
            if xx < 0 || xx >= w {
                continue;
            }
            img.put_pixel(xx as u32, yy as u32, color);
        }
    }
}

fn draw_filled_circle(img: &mut RgbaImage, cx: i32, cy: i32, radius: i32, color: Rgba<u8>) {
    let w = img.width() as i32;
    let h = img.height() as i32;
    let r = radius.max(1);
    let r2 = r * r;
    for y in (cy - r)..=(cy + r) {
        if y < 0 || y >= h {
            continue;
        }
        let dy = y - cy;
        for x in (cx - r)..=(cx + r) {
            if x < 0 || x >= w {
                continue;
            }
            let dx = x - cx;
            if dx * dx + dy * dy <= r2 {
                img.put_pixel(x as u32, y as u32, color);
            }
        }
    }
}

fn encode_overlay_jpeg_rust(image: DynamicImage) -> Option<Vec<u8>> {
    use image::codecs::jpeg::JpegEncoder;
    use image::ColorType;

    let width = image.width().max(1);
    let height = image.height().max(1);
    let rgb = image.to_rgb8();
    let mut out = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut out, overlay_jpeg_quality());
    enc.encode(rgb.as_raw(), width, height, ColorType::Rgb8.into()).ok()?;
    Some(out)
}

fn normalize_port_name(port: Option<&str>) -> Option<String> {
    let trimmed = port.map(str::trim).filter(|value| !value.is_empty())?;
    Some(trimmed.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoardParity {
    EvenSquares,
    OddSquares,
}

impl BoardParity {
    fn is_marker_square(self, x: u32, y: u32) -> bool {
        let even = (x + y).is_multiple_of(2);
        match self {
            BoardParity::EvenSquares => even,
            BoardParity::OddSquares => !even,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoardOrdering {
    RowMajor,
    ColMajor,
}

#[derive(Debug)]
struct BoardLayout {
    tag_corners_by_id: HashMap<u32, [Point; 4]>,
    square_ij_by_id: HashMap<u32, (u32, u32)>,
    squares_x: u32,
    squares_y: u32,
    square_size: f64,
}

impl BoardLayout {
    fn new(board: &CalibrationBoard, parity: BoardParity, ordering: BoardOrdering) -> Self {
        let mut tag_corners_by_id = HashMap::new();
        let mut square_ij_by_id = HashMap::new();
        let mut id = 0u32;
        let off = ((board.square_size - board.marker_size) * 0.5).max(0.0);
        match ordering {
            BoardOrdering::RowMajor => {
                for j in 0..board.squares_y {
                    for i in 0..board.squares_x {
                        if !parity.is_marker_square(i, j) {
                            continue;
                        }
                        // Marker corners (not square corners): the decoder returns the marker border quad.
                        let sx0 = (i as f64) * board.square_size;
                        let sy0 = (j as f64) * board.square_size;
                        let mx0 = sx0 + off;
                        let my0 = sy0 + off;
                        let mx1 = mx0 + board.marker_size;
                        let my1 = my0 + board.marker_size;
                        tag_corners_by_id.insert(id, [Point { x: mx0, y: my0 }, Point { x: mx1, y: my0 }, Point { x: mx1, y: my1 }, Point { x: mx0, y: my1 }]);
                        square_ij_by_id.insert(id, (i, j));
                        id = id.wrapping_add(1);
                    }
                }
            }
            BoardOrdering::ColMajor => {
                for i in 0..board.squares_x {
                    for j in 0..board.squares_y {
                        if !parity.is_marker_square(i, j) {
                            continue;
                        }
                        let sx0 = (i as f64) * board.square_size;
                        let sy0 = (j as f64) * board.square_size;
                        let mx0 = sx0 + off;
                        let my0 = sy0 + off;
                        let mx1 = mx0 + board.marker_size;
                        let my1 = my0 + board.marker_size;
                        tag_corners_by_id.insert(id, [Point { x: mx0, y: my0 }, Point { x: mx1, y: my0 }, Point { x: mx1, y: my1 }, Point { x: mx0, y: my1 }]);
                        square_ij_by_id.insert(id, (i, j));
                        id = id.wrapping_add(1);
                    }
                }
            }
        }
        Self { tag_corners_by_id, square_ij_by_id, squares_x: board.squares_x, squares_y: board.squares_y, square_size: board.square_size }
    }
}

#[derive(Debug)]
struct BuildViewResult {
    view: CalibrationView,
    marker_view: CalibrationView,
    center_view: CalibrationView,
    stats: DetectionStats,
    alignment_error_sum: f64,
    alignment_error_count: usize,
    charuco_used: bool,
    charuco_reason: Option<String>,
}

#[derive(Debug)]
struct ImageCandidate {
    image: String,
    overlay_path: Option<String>,
    row_even: BuildViewResult,
    row_odd: BuildViewResult,
    col_even: BuildViewResult,
    col_odd: BuildViewResult,
}

#[derive(Debug)]
struct ImageDetections {
    image: String,
    overlay_path: Option<String>,
    detections: Vec<ArucoDetection2D>,
    gray: GrayImage,
    width: u32,
    height: u32,
}

#[derive(Debug)]
enum DebugEntry {
    Skipped(CalibrationSolveDebugView),
    PendingImage(ImageDetections),
    Pending(Box<ImageCandidate>),
}

#[derive(Debug, Default)]
struct LayoutScore {
    usable_views: usize,
    alignment_error_sum: f64,
    alignment_error_count: usize,
}

impl LayoutScore {
    fn update(&mut self, result: &BuildViewResult, min_points: usize) {
        if result.marker_view.len() >= min_points {
            self.usable_views = self.usable_views.saturating_add(1);
        }
        if result.alignment_error_count > 0 {
            self.alignment_error_sum += result.alignment_error_sum;
            self.alignment_error_count = self.alignment_error_count.saturating_add(result.alignment_error_count);
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct TagAreaStats {
    median: Option<f64>,
    min_area: f64,
    count: usize,
}

#[derive(Debug, Default)]
struct DetectionStats {
    tags_detected: u32,
    points_detected: u32,
    coverage_ratio: f64,
    raw_tags_detected: u32,
    raw_ids: Vec<u32>,
    raw_duplicate_ids: Vec<u32>,
    raw_out_of_range_ids: Vec<u32>,
}

type Point2 = (f64, f64);

fn quad_center(quad: &[Point; 4]) -> Point {
    let mut x = 0.0;
    let mut y = 0.0;
    for p in quad {
        x += p.x;
        y += p.y;
    }
    Point { x: x / 4.0, y: y / 4.0 }
}

fn project_homography(h: &Matrix3<f64>, p: Point2) -> Option<Point2> {
    let x = h[(0, 0)] * p.0 + h[(0, 1)] * p.1 + h[(0, 2)];
    let y = h[(1, 0)] * p.0 + h[(1, 1)] * p.1 + h[(1, 2)];
    let z = h[(2, 0)] * p.0 + h[(2, 1)] * p.1 + h[(2, 2)];
    if !z.is_finite() || z.abs() <= 1e-12 {
        return None;
    }
    Some((x / z, y / z))
}

fn normalize_points(points: &[Point2]) -> Option<(Matrix3<f64>, Vec<Point2>)> {
    if points.is_empty() {
        return None;
    }
    let mut cx = 0.0;
    let mut cy = 0.0;
    for (x, y) in points {
        cx += *x;
        cy += *y;
    }
    cx /= points.len() as f64;
    cy /= points.len() as f64;

    let mut mean_dist = 0.0;
    for (x, y) in points {
        mean_dist += ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    }
    mean_dist /= points.len() as f64;
    if !mean_dist.is_finite() || mean_dist <= 1e-12 {
        return None;
    }
    let s = (2.0f64).sqrt() / mean_dist;
    let t = Matrix3::new(s, 0.0, -s * cx, 0.0, s, -s * cy, 0.0, 0.0, 1.0);
    let out = points.iter().map(|(x, y)| (s * (x - cx), s * (y - cy))).collect();
    Some((t, out))
}

fn solve_homography(obj: &[Point2], img: &[Point2]) -> Option<Matrix3<f64>> {
    if obj.len() != img.len() || obj.len() < 4 {
        return None;
    }
    let (t_obj, norm_obj) = normalize_points(obj)?;
    let (t_img, norm_img) = normalize_points(img)?;

    let n = obj.len();
    let mut a = DMatrix::<f64>::zeros(2 * n, 9);
    for (k, ((x, y), (u, v))) in norm_obj.iter().zip(norm_img.iter()).enumerate() {
        let row1 = 2 * k;
        let row2 = row1 + 1;
        a[(row1, 0)] = -*x;
        a[(row1, 1)] = -*y;
        a[(row1, 2)] = -1.0;
        a[(row1, 6)] = u * x;
        a[(row1, 7)] = u * y;
        a[(row1, 8)] = *u;

        a[(row2, 3)] = -*x;
        a[(row2, 4)] = -*y;
        a[(row2, 5)] = -1.0;
        a[(row2, 6)] = v * x;
        a[(row2, 7)] = v * y;
        a[(row2, 8)] = *v;
    }

    let svd = a.svd(true, true);
    let vt = svd.v_t?;
    let h = vt.row(vt.nrows() - 1).transpose();
    if h.len() != 9 {
        return None;
    }
    // DLT solves for `h` in row-major order: [h11, h12, h13, h21, ... , h33].
    // `Matrix3::from_column_slice` would interpret this as column-major and effectively transpose/scramble H.
    let hn = Matrix3::from_row_slice(h.as_slice());
    let h = t_img.try_inverse()? * hn * t_obj;
    let scale = if h[(2, 2)].abs() > 1e-12 { 1.0 / h[(2, 2)] } else { 1.0 };
    Some(h * scale)
}

fn robust_homography_pairs(pairs: &[(Point2, Point2)], min_threshold_px: f64) -> Option<(Matrix3<f64>, Vec<bool>, f64)> {
    if pairs.len() < 4 {
        return None;
    }
    let mut nn_dists = Vec::with_capacity(pairs.len());
    for (i, (_, img_a)) in pairs.iter().enumerate() {
        let mut best = f64::INFINITY;
        for (j, (_, img_b)) in pairs.iter().enumerate() {
            if i == j {
                continue;
            }
            let dx = img_a.0 - img_b.0;
            let dy = img_a.1 - img_b.1;
            let d = (dx * dx + dy * dy).sqrt();
            if d < best {
                best = d;
            }
        }
        if best.is_finite() {
            nn_dists.push(best);
        }
    }
    let mut nn_sorted = nn_dists.clone();
    let median_nn = median_in_place(&mut nn_sorted).unwrap_or(0.0);
    let min_threshold_px = if min_threshold_px.is_finite() { min_threshold_px.max(0.0) } else { 0.0 };
    let threshold = if median_nn.is_finite() && median_nn > 0.0 { (median_nn * 0.2).max(min_threshold_px).max(6.0) } else { 8.0f64.max(min_threshold_px) };

    let mut seed = (pairs.len() as u64).wrapping_mul(1_664_525_295).wrapping_add(1_013_904_223);
    let mut best_inliers: Vec<bool> = vec![false; pairs.len()];
    let mut best_count = 0usize;
    let mut best_error = f64::INFINITY;
    let mut best_h: Option<Matrix3<f64>> = None;

    let iterations = 64usize.max(pairs.len().saturating_mul(2));
    for _ in 0..iterations {
        let mut idx = [0usize; 4];
        let mut used = [false; 4];
        let mut count = 0usize;
        while count < 4 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let cand = (seed % (pairs.len() as u64)) as usize;
            if idx[..count].iter().all(|v| *v != cand) {
                idx[count] = cand;
                used[count] = true;
                count += 1;
            }
        }
        if !used.iter().all(|v| *v) {
            continue;
        }
        let mut obj = Vec::with_capacity(4);
        let mut img = Vec::with_capacity(4);
        for &i in &idx {
            obj.push(pairs[i].0);
            img.push(pairs[i].1);
        }
        let Some(h) = solve_homography(&obj, &img) else {
            continue;
        };
        let mut inliers = vec![false; pairs.len()];
        let mut inlier_count = 0usize;
        let mut total_error = 0.0;
        for (i, (o, img)) in pairs.iter().enumerate() {
            let err = match project_homography(&h, *o) {
                Some((u, v)) => {
                    let dx = u - img.0;
                    let dy = v - img.1;
                    (dx * dx + dy * dy).sqrt()
                }
                None => f64::INFINITY,
            };
            if err <= threshold {
                inliers[i] = true;
                inlier_count += 1;
                total_error += err;
            }
        }
        if inlier_count > best_count || (inlier_count == best_count && total_error < best_error) {
            best_count = inlier_count;
            best_error = total_error;
            best_inliers = inliers;
            best_h = Some(h);
        }
    }

    if best_count < 4 {
        return None;
    }

    let mut obj = Vec::with_capacity(best_count);
    let mut img = Vec::with_capacity(best_count);
    for (idx, ok) in best_inliers.iter().enumerate() {
        if *ok {
            obj.push(pairs[idx].0);
            img.push(pairs[idx].1);
        }
    }
    let h = solve_homography(&obj, &img).or(best_h)?;
    Some((h, best_inliers, threshold))
}

fn order_marker_corners(rotation: u8, quad: [Point; 4]) -> [Point; 4] {
    let r = (rotation & 3) as usize;
    // `ArucoDetection2D` corners are canonicalized to an image-based ordering (TL/TR/BR/BL).
    //
    // The decoder's `rotation` is the "code rotation": how many 90-degree rotations are
    // required to align the sampled grid with the dictionary's canonical orientation.
    //
    // OpenCV's `detectMarkers()` returns corners in tag-canonical order (tag-local TL/TR/BR/BL),
    // so we must rotate image-ordered corners *back* by the code rotation.
    let shift = (4 - r) & 3;
    [quad[shift], quad[(shift + 1) & 3], quad[(shift + 2) & 3], quad[(shift + 3) & 3]]
}

fn median_in_place(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    Some(values[values.len() / 2])
}

fn refine_charuco_corner_harris(gray: &GrayImage, u: f64, v: f64) -> Option<(f64, f64)> {
    if !u.is_finite() || !v.is_finite() {
        return None;
    }
    let w = gray.width() as i32;
    let h = gray.height() as i32;
    if w < 16 || h < 16 {
        return None;
    }

    // Search a small window around the predicted corner for a strong 2D corner response.
    // This is a lightweight substitute for OpenCV's cornerSubPix and dramatically reduces
    // ChArUco reprojection error versus pure homography projection, especially on fisheye lenses.
    // The homography-projected intersections can be off by ~10-30px on strong fisheye lenses.
    // Use a larger window to reliably snap to the true black/white intersection.
    let search = 24i32;
    let tensor_r = 2i32; // 5x5 tensor window
    let k = 0.04f64;

    let cx = u.round() as i32;
    let cy = v.round() as i32;

    let mut best: Option<(i32, i32, f64)> = None;
    for yy in (cy - search)..=(cy + search) {
        for xx in (cx - search)..=(cx + search) {
            // Need a 1px border for gradients, plus tensor radius.
            if xx <= tensor_r + 1 || yy <= tensor_r + 1 || xx >= w - (tensor_r + 2) || yy >= h - (tensor_r + 2) {
                continue;
            }
            let r = harris_response(gray, xx, yy, tensor_r, k)?;
            if !r.is_finite() {
                continue;
            }
            match best {
                Some((_, _, best_r)) if best_r >= r => {}
                _ => best = Some((xx, yy, r)),
            }
        }
    }

    let (bx, by, _br) = best?;

    // Simple subpixel tweak: compute a response-weighted centroid in a 3x3 neighborhood.
    let mut sum_w = 0.0f64;
    let mut sum_x = 0.0f64;
    let mut sum_y = 0.0f64;
    for yy in (by - 1)..=(by + 1) {
        for xx in (bx - 1)..=(bx + 1) {
            if xx <= tensor_r + 1 || yy <= tensor_r + 1 || xx >= w - (tensor_r + 2) || yy >= h - (tensor_r + 2) {
                continue;
            }
            let r = harris_response(gray, xx, yy, tensor_r, k).unwrap_or(0.0);
            let wgt = r.max(0.0);
            sum_w += wgt;
            sum_x += (xx as f64) * wgt;
            sum_y += (yy as f64) * wgt;
        }
    }
    if sum_w > 0.0 {
        Some((sum_x / sum_w, sum_y / sum_w))
    } else {
        Some((bx as f64, by as f64))
    }
}

fn harris_response(gray: &GrayImage, x: i32, y: i32, r: i32, k: f64) -> Option<f64> {
    let w = gray.width() as i32;
    let h = gray.height() as i32;
    if x - r - 1 < 0 || y - r - 1 < 0 || x + r + 1 >= w || y + r + 1 >= h {
        return None;
    }

    let mut sxx = 0.0f64;
    let mut syy = 0.0f64;
    let mut sxy = 0.0f64;
    for yy in (y - r)..=(y + r) {
        for xx in (x - r)..=(x + r) {
            let xm1 = (xx - 1) as u32;
            let xp1 = (xx + 1) as u32;
            let ym1 = (yy - 1) as u32;
            let yp1 = (yy + 1) as u32;

            let i_xp1 = gray.get_pixel(xp1, yy as u32).0[0] as i32;
            let i_xm1 = gray.get_pixel(xm1, yy as u32).0[0] as i32;
            let i_yp1 = gray.get_pixel(xx as u32, yp1).0[0] as i32;
            let i_ym1 = gray.get_pixel(xx as u32, ym1).0[0] as i32;
            let ix = (i_xp1 - i_xm1) as f64;
            let iy = (i_yp1 - i_ym1) as f64;

            sxx += ix * ix;
            syy += iy * iy;
            sxy += ix * iy;
        }
    }

    let det = sxx * syy - sxy * sxy;
    let trace = sxx + syy;
    Some(det - k * trace * trace)
}

fn tag_area_stats(entries: &[DebugEntry], layout: &BoardLayout, base_min_area: f64, min_points: usize, min_views: usize) -> TagAreaStats {
    let mut areas: Vec<f64> = Vec::new();
    let mut total_views = 0usize;
    for entry in entries {
        let DebugEntry::PendingImage(image) = entry else { continue };
        total_views = total_views.saturating_add(1);
        for det in &image.detections {
            if !layout.tag_corners_by_id.contains_key(&det.id) {
                continue;
            }
            let area = quad_area_px2(&det.corners);
            if !area.is_finite() || area <= 0.0 {
                continue;
            }
            areas.push(area);
        }
    }

    let count = areas.len();
    let mut stats = TagAreaStats { median: None, min_area: base_min_area, count };
    if areas.is_empty() {
        return stats;
    }

    areas.sort_by(|a, b| a.total_cmp(b));
    stats.median = Some(areas[areas.len() / 2]);

    let min_views_required = min_views.max((total_views / 2).max(4));
    let percentiles = [0.95, 0.9, 0.85, 0.8, 0.7, 0.6, 0.5, 0.4, 0.0];
    let mut chosen = base_min_area;
    let ratio = calibration_min_tag_area_median_ratio();
    for p in percentiles {
        let idx = ((areas.len().saturating_sub(1)) as f64 * p).round() as usize;
        let q = areas[idx.min(areas.len() - 1)];
        let threshold = base_min_area.max(q * ratio);
        let usable = count_views_above_area(entries, layout, threshold, min_points);
        if usable >= min_views_required {
            chosen = threshold;
            break;
        }
    }
    stats.min_area = chosen;
    stats
}

fn count_views_above_area(entries: &[DebugEntry], layout: &BoardLayout, min_area: f64, min_points: usize) -> usize {
    let mut usable = 0usize;
    for entry in entries {
        let DebugEntry::PendingImage(image) = entry else { continue };
        let mut ids: HashSet<u32> = HashSet::new();
        for det in &image.detections {
            if !layout.tag_corners_by_id.contains_key(&det.id) {
                continue;
            }
            let area = quad_area_px2(&det.corners);
            if !area.is_finite() || area <= 0.0 || area < min_area {
                continue;
            }
            ids.insert(det.id);
        }
        if ids.len().saturating_mul(4) >= min_points {
            usable = usable.saturating_add(1);
        }
    }
    usable
}

#[derive(Clone, Copy)]
struct BuildViewParams<'a> {
    width: u32,
    height: u32,
    expected_ids: &'a HashSet<u32>,
    gray: Option<&'a GrayImage>,
    min_points: usize,
    min_tag_area: f64,
    allow_charuco: bool,
    lens_model: lib_cv::modules::calibration::LensModel,
}

fn build_view(layout: &BoardLayout, detections: &[ArucoDetection2D], params: BuildViewParams<'_>) -> BuildViewResult {
    let BuildViewParams { width, height, expected_ids, gray, min_points, min_tag_area, allow_charuco, lens_model } = params;
    let is_fisheye = lens_model == lib_cv::modules::calibration::LensModel::Fisheye;
    let mut stats = DetectionStats { raw_tags_detected: detections.len().min(u32::MAX as usize) as u32, raw_ids: detections.iter().map(|d| d.id).collect(), ..Default::default() };
    let mut counts: HashMap<u32, u32> = HashMap::new();
    for id in &stats.raw_ids {
        *counts.entry(*id).or_insert(0) += 1;
    }
    stats.raw_duplicate_ids = counts.iter().filter_map(|(id, count)| if *count > 1 { Some(*id) } else { None }).collect();
    stats.raw_duplicate_ids.sort_unstable();
    stats.raw_out_of_range_ids = stats.raw_ids.iter().copied().filter(|id| !expected_ids.contains(id)).collect();
    stats.raw_out_of_range_ids.sort_unstable();
    stats.raw_out_of_range_ids.dedup();

    let mut best_by_id: BTreeMap<u32, (f64, u8, [Point; 4])> = BTreeMap::new();
    for det in detections {
        let Some(obj) = layout.tag_corners_by_id.get(&det.id) else {
            continue;
        };
        let area = quad_area_px2(&det.corners);
        if !area.is_finite() || area <= 0.0 {
            continue;
        }
        if min_tag_area > 0.0 && area < min_tag_area {
            continue;
        }
        match best_by_id.get(&det.id) {
            Some((best_area, _, _)) if *best_area >= area => {}
            _ => {
                let _ = obj;
                best_by_id.insert(det.id, (area, det.rotation, det.corners));
            }
        }
    }

    // Build center pairs from the best detection per ID.
    let mut center_pairs: Vec<(u32, Point2, Point2)> = Vec::new();
    for (id, (_, _, quad)) in &best_by_id {
        let Some(obj) = layout.tag_corners_by_id.get(id) else { continue };
        let obj_center = quad_center(obj);
        let img_center = quad_center(quad);
        center_pairs.push((*id, (obj_center.x, obj_center.y), (img_center.x, img_center.y)));
    }

    // Estimate a robust homography from marker centers and use it to reject outlier detections
    // (common failure mode: black/white squares are detected as tags and decode to a valid ID).
    let mut inlier_by_index: Vec<bool> = Vec::new();
    let mut center_obj: Vec<Point2> = Vec::new();
    let mut center_img: Vec<Point2> = Vec::new();
    let mut center_homography: Option<Matrix3<f64>> = None;
    if center_pairs.len() >= 4 {
        let pair_list: Vec<(Point2, Point2)> = center_pairs.iter().map(|(_, o, i)| (*o, *i)).collect();
        let min_thr = if is_fisheye { 24.0 } else { 6.0 };
        if let Some((h, mask, threshold)) = robust_homography_pairs(&pair_list, min_thr) {
            center_homography = Some(h);
            inlier_by_index = mask;

            // Tighten inlier set a bit: robust_homography_pairs uses an adaptive threshold; we also
            // reject any pairs that exceed 2x the computed threshold.
            if let Some(h) = &center_homography {
                let thr = if is_fisheye {
                    // Fisheye distortion can make a planar homography a poor fit at the edges;
                    // use this only to reject obvious outliers (false-positive tags).
                    (threshold.max(min_thr) * 3.0).min(160.0)
                } else {
                    (threshold.max(6.0) * 2.0).min(48.0)
                };
                for (idx, (_, obj, img)) in center_pairs.iter().enumerate() {
                    if !inlier_by_index.get(idx).copied().unwrap_or(false) {
                        continue;
                    }
                    if let Some((u, v)) = project_homography(h, *obj) {
                        let dx = u - img.0;
                        let dy = v - img.1;
                        let err = (dx * dx + dy * dy).sqrt();
                        if !err.is_finite() || err > thr {
                            inlier_by_index[idx] = false;
                        }
                    } else {
                        inlier_by_index[idx] = false;
                    }
                }
            }
        } else {
            // Fallback: attempt a plain homography. No inlier mask in this case.
            let obj_pts: Vec<Point2> = center_pairs.iter().map(|(_, o, _)| *o).collect();
            let img_pts: Vec<Point2> = center_pairs.iter().map(|(_, _, i)| *i).collect();
            center_homography = solve_homography(&obj_pts, &img_pts);
        }
    }
    if inlier_by_index.is_empty() {
        inlier_by_index = vec![true; center_pairs.len()];
    }
    for (idx, (_, obj, img)) in center_pairs.iter().enumerate() {
        if inlier_by_index.get(idx).copied().unwrap_or(false) {
            center_obj.push(*obj);
            center_img.push(*img);
        }
    }

    // Drop outlier IDs from best_by_id based on the center-homography inlier mask.
    if center_pairs.len() == inlier_by_index.len() && !center_pairs.is_empty() {
        let mut keep: HashSet<u32> = HashSet::new();
        for (idx, (id, _o, _i)) in center_pairs.iter().enumerate() {
            if inlier_by_index[idx] {
                keep.insert(*id);
            }
        }
        // A planar homography is a decent outlier rejector for pinhole lenses, but it is not a
        // reliable discriminator under strong fisheye distortion. For fisheye, keep all decoded
        // detections and rely on strict decode knobs + downstream calibration residuals.
        if !is_fisheye {
            best_by_id.retain(|id, _| keep.contains(id));
        }
    }

    let mut tag_count = 0usize;
    let mut points = Vec::new();
    let mut center_pairs: Vec<CalibrationPointPair> = Vec::new();
    let mut marker_obj_points: Vec<Point2> = Vec::new();
    let mut marker_img_points: Vec<Point2> = Vec::new();
    let mut covered_area = 0.0;
    let mut alignment_error_sum = 0.0;
    let mut alignment_error_count = 0usize;

    #[derive(Default, Clone, Copy)]
    struct CornerAccum {
        sum_u: f64,
        sum_v: f64,
        sum_w: f64,
        count: u32,
    }
    let square = layout.square_size;
    let mut charuco_accum: HashMap<(i32, i32), CornerAccum> = HashMap::new();

    #[derive(Clone, Copy)]
    struct CharucoMarkerSample {
        obj_marker: [Point; 4],
        img_marker: [Point; 4],
        square_ij: (u32, u32),
        area_px2: f64,
    }

    let mut charuco_marker_samples: Vec<CharucoMarkerSample> = Vec::new();

    for (id, (area, rot, quad)) in best_by_id {
        let obj = match layout.tag_corners_by_id.get(&id) {
            Some(obj) => obj,
            None => continue,
        };
        let obj = *obj;
        tag_count += 1;
        covered_area += area;
        // Trust the decoder's rotation for corner ordering. Using a global board homography to
        // pick the best cyclic shift is fragile under heavy fisheye distortion and can
        // permute corners inconsistently across detections, which destroys calibration.
        let ordered = order_marker_corners(rot, quad);
        if let Some(h) = &center_homography {
            let obj_center = quad_center(&obj);
            let img_center = quad_center(&ordered);
            if let Some((u, v)) = project_homography(h, (obj_center.x, obj_center.y)) {
                let dx = u - img_center.x;
                let dy = v - img_center.y;
                let err = dx * dx + dy * dy;
                if err.is_finite() {
                    alignment_error_sum += err;
                    alignment_error_count = alignment_error_count.saturating_add(1);
                }
            }
        }
        for k in 0..4 {
            points.push(CalibrationPointPair::new(obj[k], ordered[k]));
            marker_obj_points.push((obj[k].x, obj[k].y));
            marker_img_points.push((ordered[k].x, ordered[k].y));
        }
        if allow_charuco && square > 0.0 {
            if let Some(&(si, sj)) = layout.square_ij_by_id.get(&id) {
                // Capture per-marker correspondences so we can estimate local homographies.
                // This tracks OpenCV's behavior better than a single global homography on fisheye lenses.
                charuco_marker_samples.push(CharucoMarkerSample { obj_marker: obj, img_marker: ordered, square_ij: (si, sj), area_px2: area });
            }
        }
        let obj_center = quad_center(&obj);
        let img_center = quad_center(&quad);
        center_obj.push((obj_center.x, obj_center.y));
        center_img.push((img_center.x, img_center.y));
        center_pairs.push(CalibrationPointPair::new(obj_center, img_center));
    }

    stats.tags_detected = tag_count.min(u32::MAX as usize) as u32;
    let denom = (width as f64) * (height as f64);
    stats.coverage_ratio = if denom > 0.0 { (covered_area / denom).clamp(0.0, 1.0) } else { 0.0 };

    let marker_view = CalibrationView::new(points);
    let center_view = CalibrationView::new(center_pairs);
    let mut charuco_used = false;
    let mut charuco_reason = None;
    let mut charuco_pairs: Vec<CalibrationPointPair> = Vec::new();
    if !allow_charuco {
        charuco_reason = Some("charuco: disabled for small-tag mode".into());
    } else if tag_count < 2 {
        charuco_reason = Some(format!("charuco: insufficient markers ({tag_count} < 2)"));
    } else if square <= 0.0 {
        charuco_reason = Some("charuco: invalid square size".into());
    } else {
        // Approximate ChArUco chessboard corners by projecting the *square* intersections through
        // per-marker homographies and averaging any repeated estimates. This is much more robust
        // than a single global homography on fisheye lenses, and matches OpenCV's interpolation
        // behavior more closely than using marker corners directly.
        let max_ix = layout.squares_x as i32;
        let max_iy = layout.squares_y as i32;
        let w = width as f64;
        let h = height as f64;
        let margin = 64.0;

        if charuco_marker_samples.is_empty() {
            charuco_reason = Some("charuco: no marker samples".into());
        } else {
            for sample in charuco_marker_samples {
                let CharucoMarkerSample { obj_marker, img_marker, square_ij: (si, sj), area_px2: area } = sample;
                let obj_pts: [Point2; 4] = [(obj_marker[0].x, obj_marker[0].y), (obj_marker[1].x, obj_marker[1].y), (obj_marker[2].x, obj_marker[2].y), (obj_marker[3].x, obj_marker[3].y)];
                let img_pts: [Point2; 4] = [(img_marker[0].x, img_marker[0].y), (img_marker[1].x, img_marker[1].y), (img_marker[2].x, img_marker[2].y), (img_marker[3].x, img_marker[3].y)];
                // Solve a local homography from this marker's object corners to image corners.
                let hm = solve_homography(&obj_pts, &img_pts);
                let Some(hm) = hm else { continue };

                // Weight larger markers more heavily; they generally have lower corner noise.
                let weight = area.max(1.0);

                let sx0 = (si as f64) * square;
                let sy0 = (sj as f64) * square;
                let sx1 = sx0 + square;
                let sy1 = sy0 + square;
                let corners = [(si as i32, sj as i32, (sx0, sy0)), ((si + 1) as i32, sj as i32, (sx1, sy0)), ((si + 1) as i32, (sj + 1) as i32, (sx1, sy1)), (si as i32, (sj + 1) as i32, (sx0, sy1))];
                for (ix, iy, obj_corner) in corners {
                    if ix <= 0 || iy <= 0 || ix >= max_ix || iy >= max_iy {
                        continue;
                    }
                    if let Some((u, v)) = project_homography(&hm, obj_corner) {
                        if u < -margin || u > (w + margin) || v < -margin || v > (h + margin) {
                            continue;
                        }
                        let entry = charuco_accum.entry((ix, iy)).or_default();
                        entry.sum_u += u * weight;
                        entry.sum_v += v * weight;
                        entry.sum_w += weight;
                        entry.count = entry.count.saturating_add(1);
                    }
                }
            }

            for ((ix, iy), acc) in charuco_accum {
                if acc.count == 0 || acc.sum_w <= 0.0 {
                    continue;
                }
                let u = acc.sum_u / acc.sum_w;
                let v = acc.sum_v / acc.sum_w;
                let obj = Point { x: (ix as f64) * square, y: (iy as f64) * square };
                charuco_pairs.push(CalibrationPointPair::new(obj, Point { x: u, y: v }));
            }
        }

        if let Some(gray) = gray {
            for pair in &mut charuco_pairs {
                if let Some((ru, rv)) = refine_charuco_corner_harris(gray, pair.image.x, pair.image.y) {
                    pair.image.x = ru;
                    pair.image.y = rv;
                }
            }
        }

        let count = charuco_pairs.len();
        if count >= min_points {
            charuco_used = true;
        } else {
            charuco_reason = Some(format!("charuco: insufficient corners ({count} < {min_points})"));
        }
    }

    // Report the chessboard corner count for ChArUco mode. Marker mode reports marker points.
    stats.points_detected = charuco_pairs.len().min(u32::MAX as usize) as u32;
    let view = if charuco_used { CalibrationView::new(charuco_pairs) } else { CalibrationView::new(Vec::new()) };

    BuildViewResult { view, marker_view, center_view, stats, alignment_error_sum, alignment_error_count, charuco_used, charuco_reason }
}

fn quad_area_px2(quad: &[Point; 4]) -> f64 {
    let mut sum = 0.0f64;
    for i in 0..4 {
        let j = (i + 1) % 4;
        sum += quad[i].x * quad[j].y - quad[j].x * quad[i].y;
    }
    0.5 * sum.abs()
}

fn empty_debug_view(name: String, overlay: Option<String>) -> CalibrationSolveDebugView {
    CalibrationSolveDebugView {
        image: name,
        tags_detected: 0,
        points_detected: 0,
        used: false,
        coverage_ratio: 0.0,
        raw_tags_detected: 0,
        raw_ids: Vec::new(),
        raw_duplicate_ids: Vec::new(),
        raw_out_of_range_ids: Vec::new(),
        overlay_path: overlay,
    }
}

fn build_overlay_filename(display_name: &str, seq: usize, ext: &str) -> String {
    let stem = std::path::Path::new(display_name).file_stem().and_then(|s| s.to_str()).unwrap_or(display_name);
    let sanitized = sanitize_filename(stem);
    let base = if sanitized.is_empty() { "calibration" } else { sanitized.as_str() };
    let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let ext = if ext.is_empty() { "jpg" } else { ext };
    format!("{base}_overlay_{timestamp}_{seq}.{ext}")
}

fn sanitize_filename(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

fn build_view_error(used: usize, required: usize, warnings: &[String]) -> String {
    let reason = format!("not enough usable views ({used} < {required})");
    let suffix = summarize_warnings(warnings);
    append_suffix(reason, suffix)
}

fn summarize_warnings(warnings: &[String]) -> Option<String> {
    if warnings.is_empty() {
        return None;
    }
    let max = 3usize;
    let mut summary = warnings.iter().take(max).cloned().collect::<Vec<_>>().join("; ");
    if warnings.len() > max {
        summary.push_str(&format!(" (+{} more)", warnings.len() - max));
    }
    Some(summary)
}

fn append_suffix(mut base: String, suffix: Option<String>) -> String {
    if let Some(extra) = suffix {
        base.push_str("; ");
        base.push_str(&extra);
    }
    base
}
