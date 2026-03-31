use axum::{
    extract::Query,
    http::{StatusCode, header},
    response::Response,
};

use crate::api_tools_client;
use crate::api_tools_protocol::{IpaChartParams, IpaChartPdfParams};
use crate::http::error::{ApiError, ApiResult};

use super::types::{ChartParams, ChartPdfParams};

#[utoipa::path(
    get,
    path = "/device/ipa/ccm/chart",
    tag = "Device",
    responses((status = 200, description = "ColorChecker Classic 24 PNG"))
)]
pub async fn chart_png(Query(params): Query<ChartParams>) -> ApiResult<Response> {
    let patch_mm = params.patch_mm.unwrap_or(25.0).clamp(5.0, 80.0);
    let out = api_tools_client::ipa_chart_png(IpaChartParams {
        patch_mm: Some(patch_mm),
        margin_mm: Some(params.margin_mm.unwrap_or(10.0).clamp(0.0, 50.0)),
        dpi: Some(params.dpi.unwrap_or(300.0).clamp(72.0, 1200.0)),
    })
    .await?;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"colorchecker24_{}mm.png\"", patch_mm.round() as i64))
        .body(axum::body::Body::from(out))
        .map_err(|err| ApiError::internal(format!("failed to build response: {err}")))
}

#[utoipa::path(
    get,
    path = "/device/ipa/ccm/chart.pdf",
    tag = "Device",
    responses((status = 200, description = "ColorChecker Classic 24 PDF"))
)]
pub async fn chart_pdf(Query(params): Query<ChartPdfParams>) -> ApiResult<Response> {
    let patch_mm = params.patch_mm.unwrap_or(25.0).clamp(5.0, 80.0);
    let pdf_bytes = api_tools_client::ipa_chart_pdf(IpaChartPdfParams {
        patch_mm: Some(patch_mm),
        margin_mm: Some(params.margin_mm.unwrap_or(10.0).clamp(0.0, 50.0)),
        dpi: Some(params.dpi.unwrap_or(300.0).clamp(72.0, 1200.0)),
        paper: params.paper,
        orientation: params.orientation,
    })
    .await?;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/pdf")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"colorchecker24_{}mm.pdf\"", patch_mm.round() as i64))
        .body(axum::body::Body::from(pdf_bytes))
        .map_err(|err| ApiError::internal(format!("failed to build response: {err}")))
}
