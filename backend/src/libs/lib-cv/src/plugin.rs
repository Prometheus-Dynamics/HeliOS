#[cfg(feature = "gpu")]
use daedalus::gpu::{Compute, GpuError};
use daedalus::runtime::RuntimeValue;
use daedalus::runtime::plugins::RegistryPluginExt;
use daedalus::{Plugin, PluginRegistry};
use image::{DynamicImage, GrayAlphaImage, GrayImage, RgbImage, RgbaImage};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};

use crate::{Point, modules};

#[cfg(feature = "gpu")]
pub type ImagePayload = Compute<DynamicImage>;
#[cfg(not(feature = "gpu"))]
pub type ImagePayload = DynamicImage;

#[cfg(feature = "gpu")]
pub trait ImagePayloadExt {
    fn to_rgba_bytes(&self, gpu: Option<&daedalus::gpu::GpuContextHandle>) -> Result<(Vec<u8>, u32, u32), GpuError>;
}

#[cfg(feature = "gpu")]
impl ImagePayloadExt for ImagePayload {
    fn to_rgba_bytes(&self, gpu: Option<&daedalus::gpu::GpuContextHandle>) -> Result<(Vec<u8>, u32, u32), GpuError> {
        match self {
            Compute::Cpu(img) => {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                Ok((rgba.into_raw(), w, h))
            }
            Compute::Gpu(handle) => {
                let ctx = gpu.ok_or(GpuError::Unsupported)?;
                let bytes = ctx.read_texture(handle)?;
                Ok((bytes, handle.width, handle.height))
            }
        }
    }
}

#[repr(i64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecMode {
    #[default]
    Auto = 0,
    Cpu = 1,
    Gpu = 2,
}

impl ExecMode {
    pub fn from_label(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "cpu" => Self::Cpu,
            "gpu" => Self::Gpu,
            _ => Self::Auto,
        }
    }
}

#[derive(Default)]
pub struct CvPlugin;

impl Plugin for CvPlugin {
    fn id(&self) -> &'static str {
        "cv"
    }

    fn install(&self, registry: &mut PluginRegistry) -> Result<(), &'static str> {
        register_payload_size_inspectors();
        registry.register_enum::<ExecMode>(["auto", "cpu", "gpu"]);
        registry.register_enum::<crate::modules::aruco::ArucoCandidateQuadsMode>(["robust", "fast"]);
        registry.register_enum::<crate::modules::aruco::ArucoMaskMode>(["adaptive_mean", "otsu"]);
        registry.register_enum::<crate::modules::aruco::ArucoDictionaryKind>(crate::modules::aruco::ArucoDictionaryKind::values());
        registry.register_enum::<crate::modules::aruco::pose::TagPoseMethod>(["auto", "homography_v1", "homography_v2", "pnp_refine"]);
        registry.register_enum::<crate::modules::aruco::ArucoDetectionsFilterMode>([
            "left_most",
            "right_most",
            "top_most",
            "bottom_most",
            "middle_most",
            "top_left",
            "top_right",
            "bottom_left",
            "bottom_right",
            "largest_area",
            "smallest_area",
        ]);
        registry.register_enum::<crate::modules::calibration::LensModel>(["pinhole", "fisheye"]);
        registry.register_enum::<crate::modules::image::nodes::MorphNorm>(["l1", "l2", "linf"]);
        registry.register_enum::<crate::modules::image::nodes::BorderMode>(["zero", "clamp"]);
        registry.register_enum::<crate::modules::image::nodes::UndistortZoomMode>(["manual", "fill"]);

        crate::daedalus_types::register_cv_host_types(registry)?;
        crate::daedalus_types::register_image_runtime_types(registry)?;

        registry.register_output_mover::<DynamicImage, _>(|img| RuntimeValue::Any(Arc::new(img)));
        registry.register_output_mover::<GrayImage, _>(|img| RuntimeValue::Any(Arc::new(img)));
        registry.register_output_mover::<GrayAlphaImage, _>(|img| {
            let dyn_img = DynamicImage::ImageLumaA8(img);
            RuntimeValue::Any(Arc::new(dyn_img))
        });
        registry.register_output_mover::<RgbImage, _>(|img| RuntimeValue::Any(Arc::new(img)));
        registry.register_output_mover::<RgbaImage, _>(|img| RuntimeValue::Any(Arc::new(img)));
        // Typed CV containers are frequently fanned out to multiple nodes and host outputs.
        // Wrap them in a shared Arc carrier so downstream readers can borrow without cloning
        // the full vector payload on every frame.
        registry.register_output_mover::<Vec<crate::modules::aruco::ArucoDetection2D>, _>(|detections| RuntimeValue::Any(Arc::new(Arc::new(detections))));
        registry.register_output_mover::<Vec<[Point; 4]>, _>(|quads| RuntimeValue::Any(Arc::new(Arc::new(quads))));

        // Use bare values for unit plugins; use `new()` for macro-generated plugins with fields.
        let image = modules::image::nodes::CvImagePlugin;
        let blur = modules::image::blur::nodes::CvBlurPlugin::new();
        let color = modules::color::nodes::CvColorPlugin;
        let analysis = modules::analysis::nodes::CvAnalysisPlugin::new();
        let motion = modules::motion::nodes::CvMotionPlugin::new();
        let logic = modules::logic::nodes::CvLogicPlugin::new();

        registry.install_plugin(&image)?;
        registry.install_plugin(&blur)?;
        registry.install_plugin(&color)?;
        registry.install_plugin(&analysis)?;
        registry.install_plugin(&motion)?;
        registry.install_plugin(&logic)?;

        #[cfg(feature = "draw")]
        {
            let draw = modules::draw::nodes::CvDrawPlugin::new();
            registry.install_plugin(&draw)?;
        }
        #[cfg(feature = "contour")]
        {
            let contour = modules::contour::nodes::CvContourPlugin::new();
            registry.install_plugin(&contour)?;
        }
        #[cfg(feature = "aruco")]
        {
            let aruco = modules::aruco::nodes::CvArucoPlugin;
            registry.install_plugin(&aruco)?;
        }

        Ok(())
    }
}

pub fn plugin() -> CvPlugin {
    CvPlugin
}

fn register_payload_size_inspectors() {
    static REGISTERED: OnceLock<()> = OnceLock::new();
    REGISTERED.get_or_init(|| {
        daedalus::runtime::register_runtime_data_size_inspector(cv_payload_size_bytes);
    });
}

fn cv_payload_size_bytes(any: &(dyn std::any::Any + Send + Sync)) -> Option<u64> {
    if let Some(image) = any.downcast_ref::<crate::BinaryImage>() {
        return Some(binary_image_size_bytes(image));
    }
    if let Some(quads) = any.downcast_ref::<Vec<[Point; 4]>>() {
        return Some(vec_inline_bytes(quads) as u64);
    }
    if let Some(quads) = any.downcast_ref::<Arc<Vec<[Point; 4]>>>() {
        return Some(vec_inline_bytes(quads.as_ref()) as u64);
    }
    if let Some(contours) = any.downcast_ref::<Vec<Vec<Point>>>() {
        return Some(contours_size_bytes(contours));
    }
    if let Some(detections) = any.downcast_ref::<Vec<crate::modules::aruco::ArucoDetection2D>>() {
        return Some(aruco_detection_vec_size_bytes(detections));
    }
    if let Some(detections) = any.downcast_ref::<Arc<Vec<crate::modules::aruco::ArucoDetection2D>>>() {
        return Some(aruco_detection_vec_size_bytes(detections.as_ref()));
    }
    if let Some(output) = any.downcast_ref::<crate::modules::aruco::DetectionPoseOutput>() {
        return Some(detection_pose_output_size_bytes(output));
    }
    if let Some(detections) = any.downcast_ref::<Vec<crate::modules::aruco::DetectionPose>>() {
        return Some(detection_pose_vec_size_bytes(detections));
    }
    None
}

fn vec_inline_bytes<T>(values: &Vec<T>) -> usize {
    std::mem::size_of::<Vec<T>>() + values.capacity() * std::mem::size_of::<T>()
}

fn binary_image_size_bytes(image: &crate::BinaryImage) -> u64 {
    let raw = serde_json::to_vec(image).map(|bytes| bytes.len() as u64).unwrap_or(0);
    raw.max(std::mem::size_of::<crate::BinaryImage>() as u64)
}

fn contours_size_bytes(contours: &Vec<Vec<Point>>) -> u64 {
    let mut total = vec_inline_bytes(contours) as u64;
    total = total.saturating_add(contours.iter().map(|contour| vec_inline_bytes(contour) as u64).sum::<u64>());
    total
}

fn aruco_bit_grid_size_bytes(grid: &crate::modules::aruco::ArucoBitGrid) -> u64 {
    let mut total = std::mem::size_of::<crate::modules::aruco::ArucoBitGrid>() as u64;
    total = total.saturating_add(vec_inline_bytes(&grid.rows) as u64);
    total.saturating_add(grid.rows.iter().map(|row| row.capacity() as u64).sum::<u64>())
}

fn aruco_detection_size_bytes(detection: &crate::modules::aruco::ArucoDetection2D) -> u64 {
    let mut total = std::mem::size_of::<crate::modules::aruco::ArucoDetection2D>() as u64;
    if let Some(bits) = detection.bits.as_ref() {
        total = total.saturating_add(aruco_bit_grid_size_bytes(bits));
    }
    total
}

fn aruco_detection_vec_size_bytes(detections: &Vec<crate::modules::aruco::ArucoDetection2D>) -> u64 {
    let mut total = vec_inline_bytes(detections) as u64;
    total = total.saturating_add(detections.iter().map(aruco_detection_size_bytes).sum::<u64>());
    total
}

fn detection_pose_size_bytes(detection: &crate::modules::aruco::DetectionPose) -> u64 {
    let mut total = std::mem::size_of::<crate::modules::aruco::DetectionPose>() as u64;
    total = total.saturating_add(detection.pose_method.capacity() as u64);
    if let Some(bits) = detection.bits.as_ref() {
        total = total.saturating_add(aruco_bit_grid_size_bytes(bits));
    }
    total
}

fn detection_pose_failure_sample_size_bytes(sample: &crate::modules::aruco::DetectionPoseFailureSample) -> u64 {
    std::mem::size_of::<crate::modules::aruco::DetectionPoseFailureSample>() as u64 + sample.reason.capacity() as u64
}

fn detection_pose_stats_size_bytes(stats: &crate::modules::aruco::DetectionPoseStats) -> u64 {
    let mut total = std::mem::size_of::<crate::modules::aruco::DetectionPoseStats>() as u64;
    total = total.saturating_add(stats.pose_method.capacity() as u64);
    if let Some(first_failure) = stats.first_failure.as_ref() {
        total = total.saturating_add(detection_pose_failure_sample_size_bytes(first_failure));
    }
    total
}

fn detection_pose_vec_size_bytes(detections: &Vec<crate::modules::aruco::DetectionPose>) -> u64 {
    let mut total = vec_inline_bytes(detections) as u64;
    total = total.saturating_add(detections.iter().map(detection_pose_size_bytes).sum::<u64>());
    total
}

fn detection_pose_output_size_bytes(output: &crate::modules::aruco::DetectionPoseOutput) -> u64 {
    std::mem::size_of::<crate::modules::aruco::DetectionPoseOutput>() as u64 + detection_pose_vec_size_bytes(&output.detections) + detection_pose_stats_size_bytes(&output.stats)
}
