use axum::{
    Json,
    extract::Query,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use flate2::{Compression, write::ZlibEncoder};
use image::{ImageFormat, Rgb, RgbImage};
use lopdf::{Document as PdfDocument, Object as PdfObject, Stream as PdfStream, dictionary};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use utoipa::{IntoParams, ToSchema};

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
const COLORCHECKER_COLS: u32 = 6;
const COLORCHECKER_ROWS: u32 = 4;

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
    let mut corners = req.corners;
    order_corners_tl_tr_br_bl(&mut corners);

    let (ccm, rms_error) = solve_colorchecker24_ccm(&bytes, corners).map_err(|err| ApiError::bad_request(format!("solve failed: {err}")))?;

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
    let margin_mm = params.margin_mm.unwrap_or(10.0).clamp(0.0, 50.0);
    let dpi = params.dpi.unwrap_or(300.0).clamp(72.0, 1200.0);
    let patch_px = mm_to_px(patch_mm, dpi).clamp(8, 8_000);
    let margin_px = mm_to_px(margin_mm, dpi).clamp(0, 8_000);

    let chart = render_colorchecker_chart(patch_px, margin_px);
    let mut out = Vec::new();
    chart.write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png).map_err(|err| ApiError::internal(format!("png encode failed: {err}")))?;

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
    let margin_mm = params.margin_mm.unwrap_or(10.0).clamp(0.0, 50.0);
    let dpi = params.dpi.unwrap_or(300.0).clamp(72.0, 1200.0);

    let patch_px = mm_to_px(patch_mm, dpi).clamp(8, 8_000);
    let margin_px = mm_to_px(margin_mm, dpi).clamp(0, 8_000);
    let chart = render_colorchecker_chart(patch_px, margin_px);

    let chart_w_mm = COLORCHECKER_COLS as f64 * patch_mm;
    let chart_h_mm = COLORCHECKER_ROWS as f64 * patch_mm;
    let required_w_mm = chart_w_mm + 2.0 * margin_mm;
    let required_h_mm = chart_h_mm + 2.0 * margin_mm;

    let paper = params.paper.as_deref().unwrap_or("auto").trim().to_ascii_lowercase();
    let orientation = params.orientation.as_deref().unwrap_or("auto").trim().to_ascii_lowercase();
    if orientation != "auto" && orientation != "portrait" && orientation != "landscape" {
        return Err(ApiError::bad_request("invalid orientation; use auto|portrait|landscape"));
    }

    const LETTER_W_MM: f64 = 215.9;
    const LETTER_H_MM: f64 = 279.4;
    const A4_W_MM: f64 = 210.0;
    const A4_H_MM: f64 = 297.0;

    let fits = |w: f64, h: f64| required_w_mm <= w + 1e-6 && required_h_mm <= h + 1e-6;

    let pick_orientation = |portrait_w: f64, portrait_h: f64| -> ApiResult<(f64, f64)> {
        match orientation.as_str() {
            "portrait" => {
                if fits(portrait_w, portrait_h) {
                    Ok((portrait_w, portrait_h))
                } else {
                    Err(ApiError::bad_request("chart does not fit on requested paper (portrait)"))
                }
            }
            "landscape" => {
                if fits(portrait_h, portrait_w) {
                    Ok((portrait_h, portrait_w))
                } else {
                    Err(ApiError::bad_request("chart does not fit on requested paper (landscape)"))
                }
            }
            "auto" => {
                if fits(portrait_w, portrait_h) {
                    Ok((portrait_w, portrait_h))
                } else if fits(portrait_h, portrait_w) {
                    Ok((portrait_h, portrait_w))
                } else {
                    Err(ApiError::bad_request("chart does not fit on requested paper"))
                }
            }
            _ => Err(ApiError::bad_request("invalid orientation; use auto|portrait|landscape")),
        }
    };

    let (page_w_mm, page_h_mm) = match paper.as_str() {
        "letter" => pick_orientation(LETTER_W_MM, LETTER_H_MM)?,
        "a4" => pick_orientation(A4_W_MM, A4_H_MM)?,
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

    let extra_w_mm = (page_w_mm - required_w_mm).max(0.0);
    let extra_h_mm = (page_h_mm - required_h_mm).max(0.0);
    let image_x_mm = margin_mm + (extra_w_mm * 0.5);
    let image_y_mm = margin_mm + (extra_h_mm * 0.5);

    let pdf_bytes = render_chart_pdf_rgb(&chart, page_w_mm, page_h_mm, image_x_mm, image_y_mm, chart_w_mm, chart_h_mm).map_err(|err| ApiError::internal(format!("pdf encode failed: {err}")))?;

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

fn order_corners_tl_tr_br_bl(corners: &mut [[f64; 2]; 4]) {
    let mut pts = corners.to_vec();
    pts.sort_by(|a, b| a[1].partial_cmp(&b[1]).unwrap_or(std::cmp::Ordering::Equal));
    let mut top = [pts[0], pts[1]];
    let mut bottom = [pts[2], pts[3]];
    top.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
    bottom.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
    *corners = [top[0], top[1], bottom[1], bottom[0]];
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn mm_to_px(mm: f64, dpi: f64) -> u32 {
    let px = (mm / 25.4) * dpi;
    px.round().max(1.0) as u32
}

fn mm_to_pt(mm: f64) -> f32 {
    ((mm / 25.4) * 72.0) as f32
}

fn render_colorchecker_chart(patch_px: u32, margin_px: u32) -> RgbImage {
    let width = COLORCHECKER_COLS * patch_px + margin_px.saturating_mul(2);
    let height = COLORCHECKER_ROWS * patch_px + margin_px.saturating_mul(2);
    let mut img = RgbImage::from_pixel(width, height, Rgb([255, 255, 255]));

    for row in 0..COLORCHECKER_ROWS {
        for col in 0..COLORCHECKER_COLS {
            let idx = (row * COLORCHECKER_COLS + col) as usize;
            let [r, g, b] = COLORCHECKER_CLASSIC_24_SRGB[idx];
            let color = Rgb([r as u8, g as u8, b as u8]);
            let x0 = margin_px + col * patch_px;
            let y0 = margin_px + row * patch_px;
            for y in y0..(y0 + patch_px) {
                for x in x0..(x0 + patch_px) {
                    img.put_pixel(x, y, color);
                }
            }
        }
    }

    img
}

fn render_chart_pdf_rgb(chart: &RgbImage, page_w_mm: f64, page_h_mm: f64, image_x_mm: f64, image_y_mm: f64, image_w_mm: f64, image_h_mm: f64) -> anyhow::Result<Vec<u8>> {
    let page_w_pt = mm_to_pt(page_w_mm);
    let page_h_pt = mm_to_pt(page_h_mm);
    let x_pt = mm_to_pt(image_x_mm);
    let y_pt = mm_to_pt(image_y_mm);
    let w_pt = mm_to_pt(image_w_mm);
    let h_pt = mm_to_pt(image_h_mm);

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(chart.as_raw())?;
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
                "Width" => chart.width() as i64,
                "Height" => chart.height() as i64,
                "ColorSpace" => "DeviceRGB",
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

    let scale_len_mm = (page_w_mm - (image_x_mm * 2.0)).clamp(20.0, 100.0);
    let scale_len_pt = mm_to_pt(scale_len_mm);
    let scale_x_pt = ((page_w_pt - scale_len_pt) * 0.5).max(0.0);
    let scale_y_mm = (image_y_mm * 0.4).clamp(4.0, 8.0);
    let scale_y_pt = mm_to_pt(scale_y_mm);
    let mid_x_pt = scale_x_pt + (scale_len_pt * 0.5);
    let label_y_pt = (scale_y_pt - 12.0).max(2.0);
    let label_x_pt = (mid_x_pt - 40.0).max(0.0);

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
        /F1 10 Tf\n\
        {lx} {ly} Td\n\
        (Scale bar: {scale_mm:.0} mm) Tj\n\
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

fn solve_colorchecker24_ccm(image_bytes: &[u8], corners: [[f64; 2]; 4]) -> anyhow::Result<([[f64; 3]; 3], f64)> {
    use nalgebra::{DMatrix, Matrix3, Vector3};

    let img = image::load_from_memory(image_bytes)?.to_rgb8();
    let (w, h) = img.dimensions();
    if w < 8 || h < 8 {
        anyhow::bail!("image too small");
    }

    let h_mat = homography_from_unit_square(corners)?;
    let refs_srgb: [[f64; 3]; 24] = COLORCHECKER_CLASSIC_24_SRGB;

    let mut cam_samples = Vec::with_capacity(24);
    let mut ref_samples = Vec::with_capacity(24);
    let ctx = SamplePatchContext { img: &img, w: w as f64, h: h as f64, h_mat: &h_mat, grid: 5 };
    for (idx, srgb) in refs_srgb.iter().enumerate() {
        let row = idx / 6;
        let col = idx % 6;
        let (u0, u1) = (col as f64 / 6.0, (col as f64 + 1.0) / 6.0);
        let (v0, v1) = (row as f64 / 4.0, (row as f64 + 1.0) / 4.0);
        let uc = (u0 + u1) * 0.5;
        let vc = (v0 + v1) * 0.5;
        let du = (u1 - u0) * 0.18;
        let dv = (v1 - v0) * 0.18;
        let cam = sample_patch_rgb_linear(&ctx, uc, vc, du, dv)?;
        cam_samples.push(cam);

        let r = srgb_to_linear(srgb[0] / 255.0);
        let g = srgb_to_linear(srgb[1] / 255.0);
        let b = srgb_to_linear(srgb[2] / 255.0);
        ref_samples.push([r, g, b]);
    }

    let a = DMatrix::from_row_slice(24, 3, &cam_samples.iter().flat_map(|v| v.iter()).copied().collect::<Vec<_>>());
    let b = DMatrix::from_row_slice(24, 3, &ref_samples.iter().flat_map(|v| v.iter()).copied().collect::<Vec<_>>());
    let ata = &a.transpose() * &a;
    let atb = &a.transpose() * &b;
    let solved = ata.lu().solve(&atb).ok_or_else(|| anyhow::anyhow!("singular solve"))?;

    let m = Matrix3::new(solved[(0, 0)], solved[(0, 1)], solved[(0, 2)], solved[(1, 0)], solved[(1, 1)], solved[(1, 2)], solved[(2, 0)], solved[(2, 1)], solved[(2, 2)]);

    let mut mse = 0.0;
    for i in 0..24 {
        let cam_v = Vector3::new(cam_samples[i][0], cam_samples[i][1], cam_samples[i][2]);
        let ref_v = Vector3::new(ref_samples[i][0], ref_samples[i][1], ref_samples[i][2]);
        let out = m.transpose() * cam_v;
        let diff = out - ref_v;
        mse += diff.dot(&diff);
    }
    let rms = (mse / 24.0).sqrt();

    Ok(([[m[(0, 0)], m[(0, 1)], m[(0, 2)]], [m[(1, 0)], m[(1, 1)], m[(1, 2)]], [m[(2, 0)], m[(2, 1)], m[(2, 2)]]], rms))
}

fn homography_from_unit_square(corners: [[f64; 2]; 4]) -> anyhow::Result<nalgebra::Matrix3<f64>> {
    use nalgebra::{DMatrix, DVector, Matrix3};
    let src = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let mut a = DMatrix::<f64>::zeros(8, 8);
    let mut b = DVector::<f64>::zeros(8);
    for i in 0..4 {
        let u = src[i][0];
        let v = src[i][1];
        let x = corners[i][0];
        let y = corners[i][1];
        let r0 = i * 2;
        let r1 = r0 + 1;
        a[(r0, 0)] = u;
        a[(r0, 1)] = v;
        a[(r0, 2)] = 1.0;
        a[(r0, 6)] = -u * x;
        a[(r0, 7)] = -v * x;
        b[r0] = x;

        a[(r1, 3)] = u;
        a[(r1, 4)] = v;
        a[(r1, 5)] = 1.0;
        a[(r1, 6)] = -u * y;
        a[(r1, 7)] = -v * y;
        b[r1] = y;
    }
    let h = a.lu().solve(&b).ok_or_else(|| anyhow::anyhow!("homography solve failed"))?;
    Ok(Matrix3::new(h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], 1.0))
}

struct SamplePatchContext<'a> {
    img: &'a image::RgbImage,
    w: f64,
    h: f64,
    h_mat: &'a nalgebra::Matrix3<f64>,
    grid: i32,
}

fn sample_patch_rgb_linear(ctx: &SamplePatchContext<'_>, u: f64, v: f64, du: f64, dv: f64) -> anyhow::Result<[f64; 3]> {
    use nalgebra::Vector3;
    if ctx.grid <= 1 {
        anyhow::bail!("invalid grid");
    }
    let mut sum = [0.0f64; 3];
    let mut count = 0.0f64;
    for yi in 0..ctx.grid {
        for xi in 0..ctx.grid {
            let fu = u + (xi as f64 / (ctx.grid as f64 - 1.0) - 0.5) * 2.0 * du;
            let fv = v + (yi as f64 / (ctx.grid as f64 - 1.0) - 0.5) * 2.0 * dv;
            let p = ctx.h_mat * Vector3::new(fu, fv, 1.0);
            if p[2].abs() < 1e-9 {
                continue;
            }
            let x = p[0] / p[2];
            let y = p[1] / p[2];
            if !(0.0..(ctx.w - 1.0)).contains(&x) || !(0.0..(ctx.h - 1.0)).contains(&y) {
                continue;
            }
            let px = ctx.img.get_pixel(x.round() as u32, y.round() as u32).0;
            sum[0] += srgb_to_linear(px[0] as f64 / 255.0);
            sum[1] += srgb_to_linear(px[1] as f64 / 255.0);
            sum[2] += srgb_to_linear(px[2] as f64 / 255.0);
            count += 1.0;
        }
    }
    if count < 1.0 {
        anyhow::bail!("no samples in patch region (check corners)");
    }
    Ok([sum[0] / count, sum[1] / count, sum[2] / count])
}

const COLORCHECKER_CLASSIC_24_SRGB: [[f64; 3]; 24] = [
    [115.0, 82.0, 68.0],
    [194.0, 150.0, 130.0],
    [98.0, 122.0, 157.0],
    [87.0, 108.0, 67.0],
    [133.0, 128.0, 177.0],
    [103.0, 189.0, 170.0],
    [214.0, 126.0, 44.0],
    [80.0, 91.0, 166.0],
    [193.0, 90.0, 99.0],
    [94.0, 60.0, 108.0],
    [157.0, 188.0, 64.0],
    [224.0, 163.0, 46.0],
    [56.0, 61.0, 150.0],
    [70.0, 148.0, 73.0],
    [175.0, 54.0, 60.0],
    [231.0, 199.0, 31.0],
    [187.0, 86.0, 149.0],
    [8.0, 133.0, 161.0],
    [243.0, 243.0, 242.0],
    [200.0, 200.0, 200.0],
    [160.0, 160.0, 160.0],
    [122.0, 122.0, 121.0],
    [85.0, 85.0, 85.0],
    [52.0, 52.0, 52.0],
];

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
