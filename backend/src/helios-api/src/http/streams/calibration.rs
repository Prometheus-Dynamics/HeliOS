use axum::{
    Json,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use flate2::{Compression, write::ZlibEncoder};
use image::ImageFormat;
use lopdf::{Document as PdfDocument, Object as PdfObject, Stream as PdfStream, dictionary};
use serde::{Deserialize, Serialize};
use std::io::Write;
use utoipa::ToSchema;
use uuid::Uuid;

use helios_engine::ipc::{EngineErrorCode, EngineEvent};
use lib_cv::modules::calibration::LensModel;

use crate::http::AppState;
use crate::http::error::{ApiError, ApiResult};
use crate::http::streams_persist;

use super::util::{camera_id_for_manifest, engine_error_body};

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

const LETTER_W_MM: f64 = 215.9;
const LETTER_H_MM: f64 = 279.4;
const A4_W_MM: f64 = 210.0;
const A4_H_MM: f64 = 297.0;

fn max_square_mm_that_fits(squares_x: u32, squares_y: u32, margin_mm: f64, page_w_mm: f64, page_h_mm: f64) -> f64 {
    let usable_w_mm = (page_w_mm - 2.0 * margin_mm).max(0.0);
    let usable_h_mm = (page_h_mm - 2.0 * margin_mm).max(0.0);
    let by_w = usable_w_mm / squares_x as f64;
    let by_h = usable_h_mm / squares_y as f64;
    by_w.min(by_h)
}

fn floor_tenth(mm: f64) -> f64 {
    (mm * 10.0).floor() / 10.0
}

fn resolve_board_square_mm_default(requested_square_mm: Option<f64>, squares_x: u32, squares_y: u32, margin_mm: f64, paper: &str, orientation: &str) -> f64 {
    if let Some(square_mm) = requested_square_mm {
        return square_mm.clamp(2.0, 200.0);
    }

    let base_mm: f64 = 25.0;
    let constrained_max = match (paper, orientation) {
        ("letter", "portrait") => Some(max_square_mm_that_fits(squares_x, squares_y, margin_mm, LETTER_W_MM, LETTER_H_MM)),
        ("letter", "landscape") => Some(max_square_mm_that_fits(squares_x, squares_y, margin_mm, LETTER_H_MM, LETTER_W_MM)),
        ("a4", "portrait") => Some(max_square_mm_that_fits(squares_x, squares_y, margin_mm, A4_W_MM, A4_H_MM)),
        ("a4", "landscape") => Some(max_square_mm_that_fits(squares_x, squares_y, margin_mm, A4_H_MM, A4_W_MM)),
        _ => None,
    };
    let default_mm = constrained_max.map(|max_mm| base_mm.min(floor_tenth(max_mm.max(2.0)))).unwrap_or(base_mm);
    default_mm.clamp(2.0, 200.0)
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
    let marker_px = params.marker_px.unwrap_or(100).clamp(8, square_px.saturating_sub(2));

    let dict_name = params.dictionary.as_deref().unwrap_or("4x4_1000").trim();
    let img =
        lib_cv::modules::aruco::tag::dictionary::charuco_from_dict_name(dict_name, squares_x, squares_y, square_px, marker_px).ok_or_else(|| ApiError::bad_request("unknown ArUco dictionary"))?;
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png).map_err(|err| ApiError::internal(format!("png encode failed: {err}")))?;

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
    let margin_mm = params.margin_mm.unwrap_or(10.0).clamp(0.0, 50.0);
    let dpi = params.dpi.unwrap_or(300.0).clamp(72.0, 1200.0);

    let paper = params.paper.as_deref().unwrap_or("auto").trim().to_ascii_lowercase();
    let orientation = params.orientation.as_deref().unwrap_or("auto").trim().to_ascii_lowercase();
    if orientation != "auto" && orientation != "portrait" && orientation != "landscape" {
        return Err(ApiError::bad_request("invalid orientation; use auto|portrait|landscape"));
    }

    let square_mm = resolve_board_square_mm_default(params.square_mm, squares_x, squares_y, margin_mm, &paper, &orientation);
    let marker_mm_default = (square_mm * 0.7).max(1.0).min(square_mm - 0.1);
    let marker_mm = params.marker_mm.unwrap_or(marker_mm_default).clamp(1.0, square_mm - 0.1);

    let square_px = mm_to_px(square_mm, dpi).clamp(16, 8_000);
    let marker_px = mm_to_px(marker_mm, dpi).clamp(8, square_px.saturating_sub(2));

    let dict_name = params.dictionary.as_deref().unwrap_or("4x4_1000").trim();
    let img =
        lib_cv::modules::aruco::tag::dictionary::charuco_from_dict_name(dict_name, squares_x, squares_y, square_px, marker_px).ok_or_else(|| ApiError::bad_request("unknown ArUco dictionary"))?;

    let board_w_mm = squares_x as f64 * square_mm;
    let board_h_mm = squares_y as f64 * square_mm;
    let required_w_mm = board_w_mm + 2.0 * margin_mm;
    let required_h_mm = board_h_mm + 2.0 * margin_mm;

    let fits = |w: f64, h: f64| required_w_mm <= w + 1e-6 && required_h_mm <= h + 1e-6;

    let pick_orientation = |portrait_w: f64, portrait_h: f64| -> Result<(f64, f64), &'static str> {
        match orientation.as_str() {
            "portrait" => {
                if fits(portrait_w, portrait_h) {
                    Ok((portrait_w, portrait_h))
                } else {
                    Err("board does not fit on requested paper (portrait)")
                }
            }
            "landscape" => {
                if fits(portrait_h, portrait_w) {
                    Ok((portrait_h, portrait_w))
                } else {
                    Err("board does not fit on requested paper (landscape)")
                }
            }
            "auto" => {
                if fits(portrait_w, portrait_h) {
                    Ok((portrait_w, portrait_h))
                } else if fits(portrait_h, portrait_w) {
                    Ok((portrait_h, portrait_w))
                } else {
                    Err("board does not fit on requested paper")
                }
            }
            _ => unreachable!("orientation validated above"),
        }
    };

    let (page_w_mm, page_h_mm) = match paper.as_str() {
        "letter" => pick_orientation(LETTER_W_MM, LETTER_H_MM).map_err(ApiError::bad_request)?,
        "a4" => pick_orientation(A4_W_MM, A4_H_MM).map_err(ApiError::bad_request)?,
        "custom" => (required_w_mm, required_h_mm),
        "auto" => {
            if let Ok(dims) = pick_orientation(LETTER_W_MM, LETTER_H_MM) {
                dims
            } else if let Ok(dims) = pick_orientation(A4_W_MM, A4_H_MM) {
                dims
            } else {
                (required_w_mm, required_h_mm)
            }
        }
        _ => return Err(ApiError::bad_request("invalid paper; use auto|letter|a4|custom")),
    };

    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() <= 0.5
    }

    let (resolved_paper, resolved_orientation) = if approx_eq(page_w_mm, A4_W_MM) && approx_eq(page_h_mm, A4_H_MM) {
        ("A4", "portrait")
    } else if approx_eq(page_w_mm, A4_H_MM) && approx_eq(page_h_mm, A4_W_MM) {
        ("A4", "landscape")
    } else if approx_eq(page_w_mm, LETTER_W_MM) && approx_eq(page_h_mm, LETTER_H_MM) {
        ("Letter", "portrait")
    } else if approx_eq(page_w_mm, LETTER_H_MM) && approx_eq(page_h_mm, LETTER_W_MM) {
        ("Letter", "landscape")
    } else {
        ("Custom", "custom")
    };

    let page_label = format!("{resolved_paper} {resolved_orientation}");
    let board_label = format!(
        "{sx}x{sy}, square {square_mm:.1}mm, marker {marker_mm:.1}mm, margin {margin_mm:.1}mm, {dpi:.0}dpi",
        sx = squares_x,
        sy = squares_y,
        square_mm = square_mm,
        marker_mm = marker_mm,
        margin_mm = margin_mm,
        dpi = dpi
    );

    let extra_w_mm = (page_w_mm - required_w_mm).max(0.0);
    let extra_h_mm = (page_h_mm - required_h_mm).max(0.0);
    let image_x_mm = margin_mm + (extra_w_mm * 0.5);
    let image_y_mm = margin_mm + (extra_h_mm * 0.5);

    let pdf_bytes = render_board_pdf_gray(
        &img,
        RenderBoardPdfParams { page_w_mm, page_h_mm, image_x_mm, image_y_mm, image_w_mm: board_w_mm, image_h_mm: board_h_mm, page_label: &page_label, board_label: &board_label },
    )
    .map_err(|err| ApiError::internal(format!("pdf encode failed: {err}")))?;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/pdf")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"charuco_{}_{}_{}mm.pdf\"", squares_x, squares_y, square_mm.round() as i64))
        .body(axum::body::Body::from(pdf_bytes))
        .map_err(|err| ApiError::internal(format!("failed to build response: {err}")))
}

fn mm_to_px(mm: f64, dpi: f64) -> u32 {
    let px = (mm / 25.4) * dpi;
    px.round().max(1.0) as u32
}

fn mm_to_pt(mm: f64) -> f32 {
    ((mm / 25.4) * 72.0) as f32
}

struct RenderBoardPdfParams<'a> {
    page_w_mm: f64,
    page_h_mm: f64,
    image_x_mm: f64,
    image_y_mm: f64,
    image_w_mm: f64,
    image_h_mm: f64,
    page_label: &'a str,
    board_label: &'a str,
}

fn render_board_pdf_gray(board: &image::GrayImage, params: RenderBoardPdfParams<'_>) -> anyhow::Result<Vec<u8>> {
    let RenderBoardPdfParams { page_w_mm, page_h_mm, image_x_mm, image_y_mm, image_w_mm, image_h_mm, page_label, board_label } = params;
    let page_w_pt = mm_to_pt(page_w_mm);
    let page_h_pt = mm_to_pt(page_h_mm);
    let x_pt = mm_to_pt(image_x_mm);
    let y_pt = mm_to_pt(image_y_mm);
    let w_pt = mm_to_pt(image_w_mm);
    let h_pt = mm_to_pt(image_h_mm);

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(board.as_raw())?;
    let compressed = encoder.finish()?;

    let mut doc = PdfDocument::with_version("1.5");
    let catalog_id = doc.new_object_id();
    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let image_id = doc.new_object_id();
    let contents_id = doc.new_object_id();
    let font_id = doc.new_object_id();

    doc.objects.insert(
        image_id,
        PdfObject::Stream(PdfStream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => board.width() as i64,
                "Height" => board.height() as i64,
                "ColorSpace" => "DeviceGray",
                "BitsPerComponent" => 8,
                "Filter" => "FlateDecode",
            },
            compressed,
        )),
    );

    doc.objects.insert(
        font_id,
        PdfObject::Dictionary(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        }),
    );

    // Scale bar: keep it inside the board margins (closer to the board so printers with large
    // unprintable areas still include it), and place labels above the bar.
    let max_scale_len_mm = (image_w_mm - 4.0).max(20.0);
    let scale_len_mm = max_scale_len_mm.min(100.0);
    let scale_len_pt = mm_to_pt(scale_len_mm);
    let scale_x_mm = (image_x_mm + image_w_mm - scale_len_mm).max(image_x_mm);
    let scale_x_pt = mm_to_pt(scale_x_mm);
    let max_scale_y_mm = (image_y_mm - 2.0).max(8.0);
    let scale_y_mm = (image_y_mm - 4.0).clamp(8.0, max_scale_y_mm);
    let scale_y_pt = mm_to_pt(scale_y_mm);
    let mid_x_pt = scale_x_pt + (scale_len_pt * 0.5);
    let label_above_pt = scale_y_pt + mm_to_pt(6.0);
    let label_below_pt = (scale_y_pt - mm_to_pt(6.0)).max(mm_to_pt(4.0));
    let max_label_pt = mm_to_pt((image_y_mm - 2.0).max(0.0));
    let label_y_pt = if label_above_pt <= max_label_pt { label_above_pt } else { label_below_pt };
    let label_x_pt = scale_x_pt;
    let page_label = escape_pdf_text(page_label);
    let board_label = escape_pdf_text(board_label);

    let contents = format!(
        "q\n\
        {w_pt} 0 0 {h_pt} {x_pt} {y_pt} cm\n\
        /Im0 Do\n\
        Q\n\
        0 G 1 w\n\
        {sx} {sy} m {sx2} {sy} l S\n\
        {mx} {sy} m {mx} {sy_t1} l S\n\
        {sx} {sy_t1} m {sx} {sy_t2} l S\n\
        {sx2} {sy_t1} m {sx2} {sy_t2} l S\n\
        BT\n\
        /F1 9 Tf\n\
        {lx} {ly} Td\n\
        (Scale bar: {scale_mm:.0} mm) Tj\n\
        0 -8 Td\n\
        (Page: {page_label}) Tj\n\
        0 -8 Td\n\
        (Board: {board_label}) Tj\n\
        ET\n",
        w_pt = w_pt,
        h_pt = h_pt,
        x_pt = x_pt,
        y_pt = y_pt,
        sx = scale_x_pt,
        sx2 = scale_x_pt + scale_len_pt,
        mx = mid_x_pt,
        sy = scale_y_pt,
        sy_t1 = scale_y_pt - 3.0,
        sy_t2 = scale_y_pt + 3.0,
        ly = label_y_pt,
        lx = label_x_pt,
        scale_mm = scale_len_mm,
        page_label = page_label,
        board_label = board_label,
    );
    doc.objects.insert(contents_id, PdfObject::Stream(PdfStream::new(dictionary! {}, contents.into_bytes())));

    doc.objects.insert(
        page_id,
        PdfObject::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => PdfObject::Array(vec![
                PdfObject::Integer(0),
                PdfObject::Integer(0),
                PdfObject::Real(page_w_pt),
                PdfObject::Real(page_h_pt),
            ]),
            "Resources" => dictionary! {
                "XObject" => dictionary! {
                    "Im0" => image_id,
                },
                "Font" => dictionary! {
                    "F1" => font_id,
                },
            },
            "Contents" => contents_id,
        }),
    );

    doc.objects.insert(
        pages_id,
        PdfObject::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => PdfObject::Array(vec![page_id.into()]),
            "Count" => 1,
        }),
    );

    doc.objects.insert(
        catalog_id,
        PdfObject::Dictionary(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
            "ViewerPreferences" => dictionary! {
                "PrintScaling" => "None",
            },
        }),
    );

    doc.trailer.set("Root", catalog_id);

    let mut out = Vec::new();
    doc.save_to(&mut out)?;
    Ok(out)
}

fn escape_pdf_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch == '(' || ch == ')' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out
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
    let mut manifest = summary.manifest;

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

    match state.engine.start_stream(manifest.clone()).await {
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

    if let Err(err) = streams_persist::persist_manifest_quick_checked(&camera_id_for_manifest(&manifest), Some(id), manifest.clone()).await {
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
    let mut manifest = summary.manifest;

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
    if let Err(err) = streams_persist::persist_manifest_checked(&camera_id_for_manifest(&manifest), Some(id), manifest.clone()).await {
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
    use super::*;

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
