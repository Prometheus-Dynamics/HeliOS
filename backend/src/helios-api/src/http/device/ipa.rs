use axum::{
    Json,
    extract::Query,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::{IntoParams, ToSchema};

use crate::api_tools_client;
use crate::api_tools_protocol::{IpaChartParams, IpaChartPdfParams, ToolColorChart};
use crate::http::error::{ApiError, ApiResult};
use crate::http::storage;

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IpaFileInfo {
    pub target: String,
    pub path: String,
    pub exists: bool,
    pub size_bytes: Option<u64>,
    pub modified_at_ms: Option<i64>,
    pub sha256: Option<String>,
    pub ccm_ct: Option<i64>,
    pub ccm: Option<[f64; 9]>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IpaStatus {
    pub files: Vec<IpaFileInfo>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplyCcmRequest {
    pub ccm: [[f64; 3]; 3],
    pub ct: Option<i64>,
    pub target: Option<IpaTarget>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SolveCcmRequest {
    pub image: String,
    pub corners: [[f64; 2]; 4],
    pub chart: Option<ColorChart>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema, Default)]
#[serde(rename_all = "camelCase")]
pub enum ColorChart {
    #[default]
    ColorCheckerClassic24,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SolveCcmResponse {
    pub chart: ColorChart,
    pub ccm: [[f64; 3]; 3],
    pub rms_error: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChartParams {
    pub patch_mm: Option<f64>,
    pub margin_mm: Option<f64>,
    pub dpi: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChartPdfParams {
    pub patch_mm: Option<f64>,
    pub margin_mm: Option<f64>,
    pub dpi: Option<f64>,
    pub paper: Option<String>,
    pub orientation: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum IpaTarget {
    Pisp,
    Vc4,
    Both,
}

impl IpaTarget {
    fn includes(self, candidate: &str) -> bool {
        match self {
            Self::Both => true,
            Self::Pisp => candidate == "pisp",
            Self::Vc4 => candidate == "vc4",
        }
    }
}

#[derive(Debug, Clone, Deserialize, IntoParams)]
pub struct DownloadParams {
    pub target: IpaTarget,
}

const PISP_OV9782: &str = "/usr/share/libcamera/ipa/rpi/pisp/ov9782.json";
const VC4_OV9782: &str = "/usr/share/libcamera/ipa/rpi/vc4/ov9782.json";
#[utoipa::path(
    get,
    path = "/device/ipa",
    tag = "Device",
    responses((status = 200, description = "IPA tuning files and current CCM", body = IpaStatus))
)]
pub async fn ipa_status() -> ApiResult<impl IntoResponse> {
    Ok(Json(IpaStatus { files: vec![describe_ipa("pisp", PISP_OV9782).await?, describe_ipa("vc4", VC4_OV9782).await?] }))
}

#[utoipa::path(
    get,
    path = "/device/ipa/download",
    operation_id = "device_ipa_download",
    tag = "Device",
    params(DownloadParams),
    responses((status = 200, description = "Download IPA JSON", content_type = "application/json"))
)]
pub async fn download(Query(params): Query<DownloadParams>) -> ApiResult<Response> {
    let (target, path) = match params.target {
        IpaTarget::Pisp => ("pisp", PISP_OV9782),
        IpaTarget::Vc4 => ("vc4", VC4_OV9782),
        IpaTarget::Both => return Err(ApiError::bad_request("target must be pisp or vc4")),
    };
    let bytes = tokio::fs::read(path).await.map_err(|err| ApiError::internal(format!("failed to read {path}: {err}")))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{target}_ov9782.json\""))
        .body(axum::body::Body::from(bytes))
        .map_err(|err| ApiError::internal(format!("failed to build response: {err}")))
}

#[utoipa::path(
    post,
    path = "/device/ipa/ccm",
    tag = "Device",
    request_body = ApplyCcmRequest,
    responses((status = 204, description = "CCM written; restart engine to reload"))
)]
pub async fn apply_ccm(Json(req): Json<ApplyCcmRequest>) -> ApiResult<impl IntoResponse> {
    let target = req.target.unwrap_or(IpaTarget::Both);
    let ct = req.ct.unwrap_or(4000);
    let ccm_flat = flatten_ccm(req.ccm);

    let mut patched_any = false;
    for (tgt, path) in [("pisp", PISP_OV9782), ("vc4", VC4_OV9782)] {
        if !target.includes(tgt) {
            continue;
        }
        if !tokio::fs::try_exists(path).await.unwrap_or(false) {
            continue;
        }
        backup_file(path).await?;
        patch_ccm_file(path, ct, ccm_flat).await?;
        patched_any = true;
    }

    if !patched_any {
        return Err(ApiError::not_found("no matching IPA tuning files found"));
    }

    // Best-effort restart so the new IPA tuning is loaded.
    let _ = tokio::process::Command::new("systemctl").args(["restart", "helios-engine.service"]).output().await;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/device/ipa/ccm/solve",
    tag = "Device",
    request_body = SolveCcmRequest,
    responses((status = 200, description = "Solve CCM from a chart image", body = SolveCcmResponse))
)]
pub async fn solve_ccm(Json(req): Json<SolveCcmRequest>) -> ApiResult<impl IntoResponse> {
    let image_name = storage::sanitize_name(&req.image).ok_or_else(|| ApiError::bad_request("invalid image name"))?;
    let media_dir = storage::ensure_subdir_async("media").await.map_err(|err| ApiError::internal(format!("failed to access media dir: {err}")))?;
    let path = media_dir.join(&image_name);
    let bytes = tokio::fs::read(&path).await.map_err(|err| ApiError::internal(format!("failed to read {image_name}: {err}")))?;

    let chart = req.chart.unwrap_or_default();
    let (ccm, rms_error) = api_tools_client::ipa_solve_ccm(&bytes, req.corners, tool_color_chart(chart)).await?;

    Ok(Json(SolveCcmResponse { chart, ccm, rms_error }))
}

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

fn flatten_ccm(ccm: [[f64; 3]; 3]) -> [f64; 9] {
    [
        ccm[0][0], ccm[0][1], ccm[0][2], //
        ccm[1][0], ccm[1][1], ccm[1][2], //
        ccm[2][0], ccm[2][1], ccm[2][2], //
    ]
}

fn tool_color_chart(chart: ColorChart) -> ToolColorChart {
    match chart {
        ColorChart::ColorCheckerClassic24 => ToolColorChart::ColorCheckerClassic24,
    }
}

async fn backup_file(path: &str) -> ApiResult<()> {
    let ts = Utc::now().timestamp();
    let backup_path = format!("{path}.bak.{ts}");
    if tokio::fs::try_exists(&backup_path).await.unwrap_or(false) {
        return Ok(());
    }
    tokio::fs::copy(path, &backup_path).await.map_err(|err| ApiError::internal(format!("failed to create backup {backup_path}: {err}")))?;
    Ok(())
}

async fn patch_ccm_file(path: &str, ct: i64, ccm: [f64; 9]) -> ApiResult<()> {
    let raw = tokio::fs::read_to_string(path).await.map_err(|err| ApiError::internal(format!("failed to read {path}: {err}")))?;
    let mut value: serde_json::Value = serde_json::from_str(&raw).map_err(|err| ApiError::internal(format!("invalid JSON in {path}: {err}")))?;
    patch_ccm_json(&mut value, ct, ccm)?;
    let out = serde_json::to_string_pretty(&value).map_err(|err| ApiError::internal(format!("failed to serialize {path}: {err}")))?;
    tokio::fs::write(path, out.as_bytes()).await.map_err(|err| ApiError::internal(format!("failed to write {path}: {err}")))?;
    Ok(())
}

fn patch_ccm_json(root: &mut serde_json::Value, ct: i64, ccm: [f64; 9]) -> Result<(), Box<ApiError>> {
    let Some(algs) = root.get_mut("algorithms").and_then(|v| v.as_array_mut()) else {
        return Err(Box::new(ApiError::bad_request("IPA JSON missing algorithms array")));
    };
    let mut found = false;
    for entry in algs.iter_mut() {
        let Some(obj) = entry.as_object_mut() else { continue };
        let Some(ccm_root) = obj.get_mut("rpi.ccm") else { continue };
        let Some(ccms) = ccm_root.get_mut("ccms").and_then(|v| v.as_array_mut()) else { continue };
        let idx = ccms.iter().position(|v| v.get("ct").and_then(|n| n.as_i64()) == Some(ct));
        let chosen = match idx {
            Some(idx) => ccms.get_mut(idx),
            None => ccms.first_mut(),
        };
        let Some(chosen) = chosen else { continue };
        if let Some(obj) = chosen.as_object_mut() {
            obj.insert("ct".to_string(), serde_json::json!(ct));
            obj.insert("ccm".to_string(), serde_json::json!(ccm));
            found = true;
        }
    }
    if !found {
        return Err(Box::new(ApiError::not_found("rpi.ccm entry not found in IPA JSON")));
    }
    Ok(())
}

async fn describe_ipa(target: &str, path: &str) -> ApiResult<IpaFileInfo> {
    let exists = tokio::fs::try_exists(path).await.unwrap_or(false);
    if !exists {
        return Ok(IpaFileInfo { target: target.to_string(), path: path.to_string(), exists: false, size_bytes: None, modified_at_ms: None, sha256: None, ccm_ct: None, ccm: None });
    }

    let metadata = tokio::fs::metadata(path).await.map_err(|err| ApiError::internal(format!("failed to stat {path}: {err}")))?;
    let modified_at_ms = metadata.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64);

    let bytes = tokio::fs::read(path).await.map_err(|err| ApiError::internal(format!("failed to read {path}: {err}")))?;
    let sha256 = {
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        hex::encode(hasher.finalize())
    };

    let (ccm_ct, ccm) = extract_ccm(&bytes).unwrap_or((None, None));
    Ok(IpaFileInfo { target: target.to_string(), path: path.to_string(), exists: true, size_bytes: Some(metadata.len()), modified_at_ms, sha256: Some(sha256), ccm_ct, ccm })
}

fn extract_ccm(bytes: &[u8]) -> Option<(Option<i64>, Option<[f64; 9]>)> {
    let raw = std::str::from_utf8(bytes).ok()?;
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let algs = value.get("algorithms")?.as_array()?;
    for entry in algs {
        let Some(obj) = entry.as_object() else { continue };
        let Some(ccm_root) = obj.get("rpi.ccm") else { continue };
        let Some(ccms) = ccm_root.get("ccms").and_then(|v| v.as_array()) else { continue };
        let Some(first) = ccms.first().and_then(|v| v.as_object()) else { continue };
        let ct = first.get("ct").and_then(|v| v.as_i64());
        let Some(ccm_val) = first.get("ccm").and_then(|v| v.as_array()) else { continue };
        if ccm_val.len() != 9 {
            continue;
        }
        let mut out = [0.0f64; 9];
        for (i, v) in ccm_val.iter().enumerate() {
            out[i] = v.as_f64()?;
        }
        return Some((ct, Some(out)));
    }
    None
}
