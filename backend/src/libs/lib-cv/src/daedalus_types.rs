use std::sync::Arc;

use daedalus::PluginRegistry;
use daedalus::data::{
    model::{StructField, TypeExpr, ValueType},
    typing,
};
use daedalus::registry::convert::ConverterBuilder;
use image::{DynamicImage, GrayAlphaImage, GrayImage, RgbImage, RgbaImage};

pub fn point() -> TypeExpr {
    TypeExpr::opaque("cv:point2d")
}

pub fn contour() -> TypeExpr {
    TypeExpr::list(point())
}

pub fn contours() -> TypeExpr {
    TypeExpr::list(contour())
}

pub fn quad() -> TypeExpr {
    // A quad is 4 points; we express it as a point list.
    TypeExpr::list(point())
}

pub fn quads() -> TypeExpr {
    TypeExpr::list(quad())
}

pub fn binary_image() -> TypeExpr {
    TypeExpr::opaque("cv:binary_image")
}

pub fn aruco_detection_2d() -> TypeExpr {
    TypeExpr::opaque("cv:aruco_detection_2d")
}

pub fn aruco_detections_2d() -> TypeExpr {
    TypeExpr::list(aruco_detection_2d())
}

pub fn camera_calibration() -> TypeExpr {
    TypeExpr::opaque("cv:camera_calibration")
}

pub fn translation3() -> TypeExpr {
    TypeExpr::opaque("cv:translation3")
}

pub fn quaternion() -> TypeExpr {
    TypeExpr::opaque("cv:aruco_detection_quat")
}

pub fn pose_rotation() -> TypeExpr {
    TypeExpr::opaque("cv:aruco_detection_pose_rotation")
}

pub fn aruco_bit_grid() -> TypeExpr {
    TypeExpr::opaque("cv:aruco_bit_grid")
}

pub fn detection_pose() -> TypeExpr {
    TypeExpr::opaque("cv:aruco_detection_pose")
}

pub fn detection_pose_failure_counts() -> TypeExpr {
    TypeExpr::opaque("cv:aruco_detection_pose_failure_counts")
}

pub fn detection_pose_failure_sample() -> TypeExpr {
    TypeExpr::opaque("cv:aruco_detection_pose_failure_sample")
}

pub fn detection_pose_calibration_summary() -> TypeExpr {
    TypeExpr::opaque("cv:aruco_detection_pose_calibration_summary")
}

pub fn detection_pose_stats() -> TypeExpr {
    TypeExpr::opaque("cv:aruco_detection_pose_stats")
}

pub fn detection_pose_output() -> TypeExpr {
    TypeExpr::opaque("cv:aruco_detection_pose_output")
}

pub fn json_value() -> TypeExpr {
    TypeExpr::opaque("json")
}

pub fn image_gray8() -> TypeExpr {
    TypeExpr::opaque("image:gray8")
}

pub fn image_graya8() -> TypeExpr {
    TypeExpr::opaque("image:graya8")
}

pub fn image_rgb8() -> TypeExpr {
    TypeExpr::opaque("image:rgb8")
}

pub fn image_rgba8() -> TypeExpr {
    TypeExpr::opaque("image:rgba8")
}

pub fn image_dynamic() -> TypeExpr {
    TypeExpr::opaque("image:dynamic")
}

pub fn optional_image_gray8() -> TypeExpr {
    TypeExpr::Optional(Box::new(image_gray8()))
}

pub fn optional_image_dynamic() -> TypeExpr {
    TypeExpr::Optional(Box::new(image_dynamic()))
}

pub fn pixel_rgba() -> TypeExpr {
    TypeExpr::Struct(vec![
        StructField { name: "r".into(), ty: TypeExpr::Scalar(ValueType::Int) },
        StructField { name: "g".into(), ty: TypeExpr::Scalar(ValueType::Int) },
        StructField { name: "b".into(), ty: TypeExpr::Scalar(ValueType::Int) },
        StructField { name: "a".into(), ty: TypeExpr::Scalar(ValueType::Int) },
    ])
}

pub fn register_cv_host_types(registry: &mut PluginRegistry) -> Result<(), &'static str> {
    typing::register_type::<crate::Pixel>(pixel_rgba());
    typing::register_type::<crate::BinaryImage>(binary_image());

    // Stable, UI-facing schemas + host serializers for common CV data types.
    daedalus::register_daedalus_values!(
        registry,
        crate::Point,
        crate::Translation3,
        crate::modules::aruco::ArucoBitGrid,
        crate::modules::aruco::ArucoDetection2D,
        crate::modules::aruco::DetectionQuat,
        crate::modules::aruco::DetectionPoseRotation,
        crate::modules::aruco::DetectionPose,
        crate::modules::aruco::DetectionPoseFailureCounts,
        crate::modules::aruco::DetectionPoseFailureSample,
        crate::modules::aruco::DetectionPoseCalibrationSummary,
        crate::modules::aruco::DetectionPoseStats,
        crate::modules::aruco::DetectionPoseOutput,
    )?;

    // Graph pipelines pass calibration via the host bridge; this must be stable + serializable.
    daedalus::register_daedalus_values!(registry, crate::modules::aruco::detect::CameraCalibration,)?;

    // Container outputs (no separate type key needed).
    daedalus::register_to_value_serializers!(
        registry,
        Vec<crate::modules::aruco::ArucoDetection2D>,
        Arc<Vec<crate::modules::aruco::ArucoDetection2D>>,
        Vec<Vec<crate::Point>>,
        Vec<[crate::Point; 4]>,
        Arc<Vec<[crate::Point; 4]>>,
    );

    Ok(())
}

pub fn register_image_runtime_types(registry: &mut PluginRegistry) -> Result<(), &'static str> {
    let img_gray = image_gray8();
    let img_graya = image_graya8();
    let img_rgb = image_rgb8();
    let img_rgba = image_rgba8();
    let img_dynamic = image_dynamic();
    let img_gray_opt = optional_image_gray8();
    let img_dynamic_opt = optional_image_dynamic();

    typing::register_type::<GrayImage>(img_gray.clone());
    typing::register_type::<GrayAlphaImage>(img_graya.clone());
    typing::register_type::<RgbImage>(img_rgb.clone());
    typing::register_type::<RgbaImage>(img_rgba.clone());
    typing::register_type::<DynamicImage>(img_dynamic.clone());
    typing::register_type::<Option<GrayImage>>(img_gray_opt.clone());
    typing::register_type::<Option<DynamicImage>>(img_dynamic_opt.clone());
    #[cfg(feature = "gpu")]
    {
        typing::register_type::<daedalus::gpu::Compute<DynamicImage>>(img_dynamic.clone());
        typing::register_type::<daedalus::gpu::Compute<GrayImage>>(img_gray.clone());
    }

    // Treat image flavors as mutually compatible with `image:dynamic`. Runtime coercion is
    // handled by Daedalus' conversion registry (CPU) and GpuSendable transfers (GPU).
    registry.register_type_compatibility(img_gray.clone(), img_dynamic.clone());
    registry.register_type_compatibility(img_dynamic.clone(), img_gray.clone());
    registry.register_type_compatibility(img_graya.clone(), img_dynamic.clone());
    registry.register_type_compatibility(img_dynamic.clone(), img_graya.clone());
    registry.register_type_compatibility(img_rgb.clone(), img_dynamic.clone());
    registry.register_type_compatibility(img_dynamic.clone(), img_rgb.clone());
    registry.register_type_compatibility(img_rgba.clone(), img_dynamic.clone());
    registry.register_type_compatibility(img_dynamic.clone(), img_rgba.clone());
    registry.register_type_compatibility(img_dynamic.clone(), img_dynamic_opt);
    registry.register_type_compatibility(img_gray.clone(), img_gray_opt);

    // Runtime CPU conversions between image flavors. These are intentionally registered in the
    // conversion registry (not exposed as nodes) so graphs can focus on the types they want.
    registry.register_conversion::<DynamicImage, GrayImage>(|img| Some(img.to_luma8()));
    registry.register_conversion::<DynamicImage, Option<DynamicImage>>(|img| Some(Some(img.clone())));
    registry.register_conversion::<GrayImage, DynamicImage>(|img| Some(DynamicImage::ImageLuma8(img.clone())));
    registry.register_conversion::<GrayAlphaImage, DynamicImage>(|img| Some(DynamicImage::ImageLumaA8(img.clone())));
    registry.register_conversion::<RgbImage, DynamicImage>(|img| Some(DynamicImage::ImageRgb8(img.clone())));
    registry.register_conversion::<RgbaImage, DynamicImage>(|img| Some(DynamicImage::ImageRgba8(img.clone())));
    registry.register_conversion::<DynamicImage, GrayAlphaImage>(|img| Some(img.to_luma_alpha8()));
    registry.register_conversion::<DynamicImage, RgbImage>(|img| Some(img.to_rgb8()));
    registry.register_conversion::<DynamicImage, RgbaImage>(|img| Some(img.to_rgba8()));
    registry
        .registry
        .register_converter(ConverterBuilder::new("image_gray8_to_dynamic", img_gray, image_dynamic(), Ok).build_boxed())
        .map_err(|_| "failed to register image:gray8 -> image:dynamic converter")?;
    registry
        .registry
        .register_converter(ConverterBuilder::new("image_dynamic_to_gray8", image_dynamic(), image_gray8(), Ok).build_boxed())
        .map_err(|_| "failed to register image:dynamic -> image:gray8 converter")?;

    Ok(())
}
