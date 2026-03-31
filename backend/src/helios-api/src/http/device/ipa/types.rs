use axum::extract::Query;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

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

#[derive(Debug, Clone, Deserialize, IntoParams)]
pub struct ChartParams {
    pub patch_mm: Option<f64>,
    pub margin_mm: Option<f64>,
    pub dpi: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, IntoParams)]
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
    pub(super) fn includes(self, candidate: &str) -> bool {
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

pub(super) fn require_download_target(Query(params): Query<DownloadParams>) -> Result<(&'static str, &'static str), crate::http::error::ApiError> {
    match params.target {
        IpaTarget::Pisp => Ok(("pisp", super::files::PISP_OV9782)),
        IpaTarget::Vc4 => Ok(("vc4", super::files::VC4_OV9782)),
        IpaTarget::Both => Err(crate::http::error::ApiError::bad_request("target must be pisp or vc4")),
    }
}
