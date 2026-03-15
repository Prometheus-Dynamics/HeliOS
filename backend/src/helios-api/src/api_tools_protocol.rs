use helios_peripherals::{AiModelFormat, AiModelTensorMetadata};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ToolColorChart {
    #[default]
    ColorCheckerClassic24,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationBoardParams {
    pub squares_x: Option<u32>,
    pub squares_y: Option<u32>,
    pub square_px: Option<u32>,
    pub marker_px: Option<u32>,
    pub dictionary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationBoardPdfParams {
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
pub struct IpaChartParams {
    pub patch_mm: Option<f64>,
    pub margin_mm: Option<f64>,
    pub dpi: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpaChartPdfParams {
    pub patch_mm: Option<f64>,
    pub margin_mm: Option<f64>,
    pub dpi: Option<f64>,
    pub paper: Option<String>,
    pub orientation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolModelInspection {
    #[serde(default)]
    pub suggested_tags: Vec<String>,
    #[serde(default)]
    pub inputs: Vec<AiModelTensorMetadata>,
    #[serde(default)]
    pub outputs: Vec<AiModelTensorMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApiToolsRequest {
    CalibrationBoardPng { params: CalibrationBoardParams },
    CalibrationBoardPdf { params: CalibrationBoardPdfParams },
    IpaChartPng { params: IpaChartParams },
    IpaChartPdf { params: IpaChartPdfParams },
    IpaSolveCcm { image_base64: String, corners: [[f64; 2]; 4], chart: ToolColorChart },
    ImageDimensions { image_base64: String },
    ImageEdit { image_base64: String, content_type: String, rotate_degrees: Option<i32>, crop: Option<ToolCropRect> },
    ModelInspect { model_base64: String, format: AiModelFormat },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApiToolsResponse {
    Bytes { data_base64: String },
    ImageDimensions { width: Option<u32>, height: Option<u32> },
    ImageEdit { data_base64: String, width: u32, height: u32 },
    ModelInspect { inspection: ToolModelInspection },
    SolveCcm { chart: ToolColorChart, ccm: [[f64; 3]; 3], rms_error: f64 },
    Error { message: String },
}
