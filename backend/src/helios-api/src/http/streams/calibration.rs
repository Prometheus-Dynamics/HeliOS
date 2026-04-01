use axum::{
    Json,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use helios_engine::ipc::{EngineErrorCode, EngineEvent};
use lib_cv::modules::calibration::LensModel;

use crate::api_tools_client;
use crate::api_tools_protocol::{CalibrationBoardParams, CalibrationBoardPdfParams};
use crate::http::AppState;
use crate::http::error::{ApiError, ApiResult};
use crate::http::streams_persist;

use super::util::engine_error_body;

#[derive(Debug, Clone, Deserialize)]
pub struct BoardParams {
    pub squares_x: Option<u32>,
    pub squares_y: Option<u32>,
    pub square_px: Option<u32>,
    pub marker_px: Option<u32>,
    pub dictionary: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BoardPdfParams {
    pub squares_x: Option<u32>,
    pub squares_y: Option<u32>,
    pub square_mm: Option<f64>,
    pub marker_mm: Option<f64>,
    pub margin_mm: Option<f64>,
    pub dpi: Option<f64>,
    pub paper: Option<String>,
    pub orientation: Option<String>,
    pub dictionary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(ToSchema)]
pub struct StreamCalibrationParams {
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
    pub k1: f64,
    pub k2: f64,
    pub p1: f64,
    pub p2: f64,
    pub k3: f64,
    pub undistort_iters: i64,
    #[serde(default)]
    pub lens_model: LensModel,
}

#[utoipa::path(
    get,
    path = "/streams/{id}/calibration/board",
    tag = "EngineStreams",
    params(("id" = String, Path, description = "Stream id")),
    responses((status = 200, description = "Calibration board PNG"))
)]
pub async fn board_png(Path(_id): Path<Uuid>, Query(params): Query<BoardParams>) -> ApiResult<Response> {
    let squares_x = params.squares_x.unwrap_or(8).clamp(2, 64);
    let squares_y = params.squares_y.unwrap_or(6).clamp(2, 64);
    let square_px = params.square_px.unwrap_or(140).clamp(16, 1_000);
    let out = api_tools_client::calibration_board_png(CalibrationBoardParams {
        squares_x: Some(squares_x),
        squares_y: Some(squares_y),
        square_px: Some(square_px),
        marker_px: Some(params.marker_px.unwrap_or(100).clamp(8, square_px.saturating_sub(2))),
        dictionary: params.dictionary,
    })
    .await?;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"charuco_{}_{}_{square_px}.png\"", squares_x, squares_y))
        .body(axum::body::Body::from(out))
        .map_err(|err| ApiError::internal(format!("failed to build response: {err}")))
}

#[utoipa::path(
    get,
    path = "/streams/{id}/calibration/board.pdf",
    tag = "EngineStreams",
    params(("id" = String, Path, description = "Stream id")),
    responses((status = 200, description = "Calibration board PDF"))
)]
pub async fn board_pdf(Path(_id): Path<Uuid>, Query(params): Query<BoardPdfParams>) -> ApiResult<Response> {
    let squares_x = params.squares_x.unwrap_or(8).clamp(2, 64);
    let squares_y = params.squares_y.unwrap_or(6).clamp(2, 64);
    let pdf_bytes = api_tools_client::calibration_board_pdf(CalibrationBoardPdfParams {
        squares_x: Some(squares_x),
        squares_y: Some(squares_y),
        square_mm: params.square_mm,
        marker_mm: params.marker_mm,
        margin_mm: params.margin_mm,
        dpi: params.dpi,
        paper: params.paper,
        orientation: params.orientation,
        dictionary: params.dictionary,
    })
    .await?;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/pdf")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"charuco_{}_{}.pdf\"", squares_x, squares_y))
        .body(axum::body::Body::from(pdf_bytes))
        .map_err(|err| ApiError::internal(format!("failed to build response: {err}")))
}

#[utoipa::path(
    post,
    path = "/streams/{id}/calibration/apply",
    tag = "EngineStreams",
    request_body = StreamCalibrationParams,
    responses((status = 200, description = "Updated stream manifest"))
)]
pub async fn apply_calibration(State(state): State<AppState>, Path(id): Path<Uuid>, Json(payload): Json<StreamCalibrationParams>) -> Response {
    let streams = match state.engine.list_streams().await {
        Ok(streams) => streams,
        Err(err) => return ApiError::internal(err.to_string()).into_response(),
    };
    let summary = match streams.into_iter().find(|s| s.stream_id == id) {
        Some(summary) => summary,
        None => return ApiError::not_found("stream not found").into_response(),
    };
    let mut manifest = summary.manifest.to_requested_manifest();

    manifest.calibration = Some(helios_engine::ipc::StreamCalibration {
        fx: payload.fx,
        fy: payload.fy,
        cx: payload.cx,
        cy: payload.cy,
        k1: payload.k1,
        k2: payload.k2,
        p1: payload.p1,
        p2: payload.p2,
        k3: payload.k3,
        undistort_iters: payload.undistort_iters,
        lens_model: payload.lens_model,
    });

    // Restart the stream in-place (same id) so updated graph consts take effect.
    let _ = state.engine.stop_stream(id).await;
    // Give the capture backend time to fully release before reacquiring, otherwise libcamera can
    // end up in a "Running state trying acquire() requiring state Available" error state.
    for _ in 0..20 {
        match state.engine.list_streams().await {
            Ok(active) if active.iter().any(|s| s.stream_id == id) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
            _ => break,
        }
    }
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    match state.engine.start_stream(manifest.resolve()).await {
        Ok(EngineEvent::Started { .. }) => {}
        Ok(EngineEvent::Nack { code, reason, .. }) => {
            return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(code), reason))).into_response();
        }
        Ok(_) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), "engine did not confirm stream start"))).into_response();
        }
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to restart stream: {err}")))).into_response();
        }
    }

    if let Err(err) = streams_persist::persist_manifest_quick_auto_camera_id_checked(Some(id), manifest.clone()).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("calibration applied live but failed to persist: {err}")))).into_response();
    }
    Json(manifest).into_response()
}

#[utoipa::path(
    post,
    path = "/streams/{id}/calibration/save",
    tag = "EngineStreams",
    request_body = StreamCalibrationParams,
    responses((status = 200, description = "Updated stream manifest"))
)]
pub async fn save_calibration(State(state): State<AppState>, Path(id): Path<Uuid>, Json(payload): Json<StreamCalibrationParams>) -> Response {
    let streams = match state.engine.list_streams().await {
        Ok(streams) => streams,
        Err(err) => return ApiError::internal(err.to_string()).into_response(),
    };
    let summary = match streams.into_iter().find(|s| s.stream_id == id) {
        Some(summary) => summary,
        None => return ApiError::not_found("stream not found").into_response(),
    };
    let mut manifest = summary.manifest.to_requested_manifest();

    let calibration = helios_engine::ipc::StreamCalibration {
        fx: payload.fx,
        fy: payload.fy,
        cx: payload.cx,
        cy: payload.cy,
        k1: payload.k1,
        k2: payload.k2,
        p1: payload.p1,
        p2: payload.p2,
        k3: payload.k3,
        undistort_iters: payload.undistort_iters,
        lens_model: payload.lens_model,
    };

    match state.engine.set_calibration(id, Some(calibration.clone())).await {
        Ok(EngineEvent::Ack { .. }) => {}
        Ok(EngineEvent::Nack { code, reason, .. }) => {
            return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(code), reason))).into_response();
        }
        Ok(_) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), "unexpected engine response"))).into_response();
        }
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to update calibration: {err}")))).into_response();
        }
    }

    manifest.calibration = Some(calibration);
    if let Err(err) = streams_persist::persist_manifest_auto_camera_id_checked(Some(id), manifest.clone()).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("calibration updated live but failed to persist: {err}")))).into_response();
    }
    Json(manifest).into_response()
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationModeRequest {
    pub enabled: bool,
    #[serde(default)]
    pub dictionary: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
}

#[utoipa::path(
    post,
    path = "/streams/{id}/calibration/mode",
    tag = "EngineStreams",
    request_body = CalibrationModeRequest,
    responses((status = 204, description = "Calibration mode updated"))
)]
pub async fn set_calibration_mode(State(state): State<AppState>, Path(id): Path<Uuid>, Json(payload): Json<CalibrationModeRequest>) -> Response {
    match state.engine.set_calibration_mode(id, payload.enabled, payload.dictionary.clone(), payload.mode.clone()).await {
        Ok(EngineEvent::Ack { .. }) => {}
        Ok(EngineEvent::Nack { code, reason, .. }) => {
            return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(code), reason))).into_response();
        }
        Ok(_) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), "unexpected engine response"))).into_response();
        }
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to toggle calibration mode: {err}")))).into_response();
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use crate::api_tools_impl::{A4_H_MM, A4_W_MM, LETTER_H_MM, LETTER_W_MM, resolve_board_square_mm_default};

    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn board_square_default_fits_letter_portrait() {
        let square_mm = resolve_board_square_mm_default(None, 8, 6, 10.0, "letter", "portrait");
        let required_w_mm = (8.0 * square_mm) + 20.0;
        let required_h_mm = (6.0 * square_mm) + 20.0;
        assert!(required_w_mm <= LETTER_W_MM + 1e-6);
        assert!(required_h_mm <= LETTER_H_MM + 1e-6);
        assert!(approx_eq(square_mm, 24.4));
    }

    #[test]
    fn board_square_default_fits_a4_portrait() {
        let square_mm = resolve_board_square_mm_default(None, 8, 6, 10.0, "a4", "portrait");
        let required_w_mm = (8.0 * square_mm) + 20.0;
        let required_h_mm = (6.0 * square_mm) + 20.0;
        assert!(required_w_mm <= A4_W_MM + 1e-6);
        assert!(required_h_mm <= A4_H_MM + 1e-6);
        assert!(approx_eq(square_mm, 23.7));
    }

    #[test]
    fn board_square_default_keeps_legacy_for_auto_layout() {
        let square_mm = resolve_board_square_mm_default(None, 8, 6, 10.0, "auto", "auto");
        assert!(approx_eq(square_mm, 25.0));
    }

    #[test]
    fn board_square_respects_explicit_value() {
        let square_mm = resolve_board_square_mm_default(Some(30.0), 8, 6, 10.0, "letter", "portrait");
        assert!(approx_eq(square_mm, 30.0));
    }
}
