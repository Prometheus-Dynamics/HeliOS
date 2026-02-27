#![allow(clippy::ptr_arg)]

use super::{ArucoDetection2D, ArucoDetectionsFilterMode, ArucoDetectionsOrderMode};
use crate::{BinaryImage, Point};
use daedalus::FanIn;
use daedalus::data::model::{StructFieldValue, TypeExpr, Value};
use daedalus::graph_builder::GraphCtx;
use daedalus::macros::{NodeConfig, node};
use daedalus::runtime::NodeError;
use daedalus::runtime::plugins::{Plugin, PluginRegistry};
use daedalus::runtime::state::ExecutionContext;
use geo::{LineString, Polygon, algorithm::minimum_rotated_rect::MinimumRotatedRect};
use image::{DynamicImage, GenericImageView, GrayImage, Luma, RgbImage, Rgba, RgbaImage};
use imageproc::geometric_transformations::{Interpolation, Projection, warp_into};
use rayon::prelude::*;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use daedalus::gpu::Payload;

use crate::draw;
use crate::modules::aruco::DetectionPoseOutput;
use crate::modules::aruco::detect::{
    ArucoDecodeConfig, ArucoTagDecodeConfig, ArucoTagDetectorConfig, CameraCalibration, candidate_quad_from_contour, candidate_quad_from_contour_fast, decode_quads_aruco_calibrated_with_config,
    decode_quads_aruco_with_config, decode_quads_aruco_with_config_no_bits, decode_quads_calibrated_with_config, decode_quads_warp_with_config, decode_quads_with_config,
    decode_quads_with_config_no_bits, filter_candidates, quad_satisfies_config, sort_corners_clockwise,
};
use crate::modules::aruco::pose::{TagPoseCalibration, TagPoseMethod, detections_to_tag_pose_output};
use crate::modules::aruco::tag::{ArucoTagDecoding, ArucoTagFamilyKind, decode_marker_grid};
use crate::modules::aruco::{ArucoDictionaryKind, aruco_dictionary_from_name};
use crate::modules::calibration::LensModel;
use crate::modules::image::luma::with_luma8_frame;
use imageproc::point::Point as CvPoint;
use std::collections::HashMap;
use std::time::{Duration, Instant};

mod shared;
use shared::*;

mod adaptive;
mod candidate_quads;
mod consensus;
mod decode;
mod decode_grid;
mod decode_hamming;
mod dedup;
mod json;
mod mask;
mod merge;
mod overlay;
mod poses;
mod scale;
mod temporal;

use adaptive::{
    cv_aruco_adaptive_merge_quads, cv_aruco_adaptive_quads_from_mask, cv_aruco_adaptive_quads_gate, cv_aruco_adaptive_quads_pass, cv_aruco_adaptive_quads_select_best,
    cv_aruco_adaptive_threshold_mask, cv_aruco_adaptive_window_select, cv_aruco_quads_concat,
};
use candidate_quads::{
    cv_aruco_candidate_quads, cv_aruco_candidate_quads_extract, cv_aruco_candidate_quads_filter_area, cv_aruco_candidate_quads_filter_corner_spacing, cv_aruco_candidate_quads_filter_geometry,
    cv_aruco_candidate_quads_group,
};
use consensus::cv_aruco_consensus_detections;
use decode::{cv_aruco_decode_quads, cv_aruco_decode_quads_calibrated, cv_aruco_decode_quads_warp};
use decode_grid::cv_decode_grid;
use decode_hamming::{
    cv_aruco_decode_quads_hamming, cv_aruco_decode_quads_hamming_decode_detections, cv_aruco_decode_quads_hamming_finalize_detections, cv_aruco_decode_quads_hamming_refine_detections,
};
use dedup::{cv_aruco_dedup_detections, cv_aruco_dedup_detections_spatial};
use json::{
    cv_aruco_detections_crosshair_target, cv_aruco_detections_filter, cv_aruco_detections_filter_area, cv_aruco_detections_json, cv_aruco_detections_order, cv_aruco_merge_decode_stats_json,
    cv_aruco_poses_json,
};
use mask::{cv_mask_blur_gray, cv_mask_component_filter, cv_mask_downscale_gray, cv_mask_gradient_gate, cv_mask_majority, cv_mask_open, cv_mask_open_stage, cv_mask_prune_sparse};
use merge::{cv_aruco_merge_detections, cv_aruco_merge_detections_pair, cv_aruco_merge_detections_spatial, cv_aruco_merge_quads, cv_aruco_merge_quads_pair};
use overlay::{
    cv_aruco_overlay, cv_aruco_overlay_quads, cv_aruco_overlay_quads_count, cv_detect_aruco_detections, cv_detect_aruco_detections_from_contours, cv_detect_aruco_overlay, cv_overlay_tags_count,
};
use poses::cv_aruco_tag_poses;
use scale::{cv_aruco_offset_detections, cv_aruco_scale_detections, cv_aruco_scale_quads};
use temporal::{cv_aruco_temporal_smooth_detections, cv_aruco_temporal_stabilize_detections};

#[derive(Clone, Debug, Default)]
pub struct CvArucoPlugin;

impl CvArucoPlugin {
    pub fn install(&self, registry: &mut PluginRegistry) -> Result<(), &'static str> {
        // Value serializers need to be registered in the same crate instance that produces the
        // payload types (important across dynamic plugin boundaries).
        fn value_point(p: &Point) -> Value {
            Value::Struct(vec![StructFieldValue { name: "x".into(), value: Value::Float(p.x) }, StructFieldValue { name: "y".into(), value: Value::Float(p.y) }])
        }

        fn value_quads(quads: &[[Point; 4]]) -> Value {
            Value::List(quads.iter().map(|q| Value::List(vec![value_point(&q[0]), value_point(&q[1]), value_point(&q[2]), value_point(&q[3])])).collect())
        }

        fn value_detections(dets: &[ArucoDetection2D]) -> Value {
            let mut out = Vec::with_capacity(dets.len());
            for det in dets {
                let corners = det.corners.iter().map(value_point).collect::<Vec<_>>();
                out.push(Value::Struct(vec![
                    StructFieldValue { name: "id".into(), value: Value::Int(det.id as i64) },
                    StructFieldValue { name: "rotation".into(), value: Value::Int(det.rotation as i64) },
                    StructFieldValue { name: "corners".into(), value: Value::List(corners) },
                ]));
            }
            Value::List(out)
        }

        registry.merge::<cv_decode_grid>()?;

        registry.merge::<cv_mask_downscale_gray>()?;
        registry.merge::<cv_mask_blur_gray>()?;
        registry.merge::<cv_mask_gradient_gate>()?;
        registry.merge::<cv_mask_prune_sparse>()?;
        registry.merge::<cv_mask_component_filter>()?;
        registry.merge::<cv_mask_open>()?;
        registry.merge::<cv_mask_open_stage>()?;
        registry.merge::<cv_mask_majority>()?;

        registry.merge::<cv_aruco_candidate_quads_extract>()?;
        registry.merge::<cv_aruco_candidate_quads_filter_area>()?;
        registry.merge::<cv_aruco_candidate_quads_filter_geometry>()?;
        registry.merge::<cv_aruco_candidate_quads_filter_corner_spacing>()?;
        registry.merge::<cv_aruco_candidate_quads_group>()?;
        registry.merge::<cv_aruco_candidate_quads>()?;

        // Adaptive quads internals are exposed as normal nodes so profiling can pinpoint hotspots.
        registry.merge::<cv_aruco_adaptive_merge_quads>()?;
        registry.merge::<cv_aruco_quads_concat>()?;
        registry.merge::<cv_aruco_adaptive_window_select>()?;
        registry.merge::<cv_aruco_adaptive_threshold_mask>()?;
        registry.merge::<cv_aruco_adaptive_quads_from_mask>()?;
        registry.merge::<cv_aruco_adaptive_quads_gate>()?;
        registry.merge::<cv_aruco_adaptive_quads_select_best>()?;
        registry.merge::<cv_aruco_adaptive_quads_pass>()?;

        // Decode internals are exposed as normal nodes so profiling can pinpoint hotspots.
        registry.merge::<cv_aruco_decode_quads_hamming_decode_detections>()?;
        registry.merge::<cv_aruco_decode_quads_hamming_refine_detections>()?;
        registry.merge::<cv_aruco_decode_quads_hamming_finalize_detections>()?;
        registry.merge::<cv_aruco_decode_quads_hamming>()?;
        registry.merge::<cv_aruco_decode_quads>()?;
        registry.merge::<cv_aruco_decode_quads_warp>()?;
        registry.merge::<cv_aruco_decode_quads_calibrated>()?;

        registry.merge::<cv_aruco_tag_poses>()?;
        registry.merge::<cv_aruco_poses_json>()?;
        registry.merge::<cv_aruco_detections_json>()?;
        registry.merge::<cv_aruco_detections_filter>()?;
        registry.merge::<cv_aruco_detections_filter_area>()?;
        registry.merge::<cv_aruco_detections_order>()?;
        registry.merge::<cv_aruco_detections_crosshair_target>()?;
        registry.merge::<cv_aruco_merge_decode_stats_json>()?;
        registry.merge::<cv_aruco_consensus_detections>()?;

        registry.merge::<cv_aruco_overlay>()?;
        registry.merge::<cv_aruco_overlay_quads>()?;
        registry.merge::<cv_aruco_overlay_quads_count>()?;
        registry.merge::<cv_overlay_tags_count>()?;

        registry.merge::<cv_aruco_scale_quads>()?;
        registry.merge::<cv_aruco_scale_detections>()?;
        registry.merge::<cv_aruco_offset_detections>()?;

        registry.merge::<cv_aruco_merge_quads>()?;
        registry.merge::<cv_aruco_merge_quads_pair>()?;
        registry.merge::<cv_aruco_merge_detections>()?;
        registry.merge::<cv_aruco_merge_detections_pair>()?;
        registry.merge::<cv_aruco_merge_detections_spatial>()?;

        registry.merge::<cv_aruco_dedup_detections>()?;
        registry.merge::<cv_aruco_dedup_detections_spatial>()?;
        registry.merge::<cv_aruco_temporal_smooth_detections>()?;
        registry.merge::<cv_aruco_temporal_stabilize_detections>()?;

        registry.merge::<cv_detect_aruco_overlay>()?;
        registry.merge::<cv_detect_aruco_detections>()?;
        registry.merge::<cv_detect_aruco_detections_from_contours>()?;

        // Host output sampling: enable `Value` serialization for common ArUco structs.
        registry.register_value_serializer::<Vec<ArucoDetection2D>, _>(|dets| value_detections(dets.as_slice()));
        registry.register_value_serializer::<Arc<Vec<ArucoDetection2D>>, _>(|dets| value_detections(dets.as_slice()));
        registry.register_value_serializer::<Vec<[Point; 4]>, _>(|quads| value_quads(quads.as_slice()));
        registry.register_value_serializer::<Arc<Vec<[Point; 4]>>, _>(|quads| value_quads(quads.as_slice()));

        Ok(())
    }
}

impl Plugin for CvArucoPlugin {
    fn id(&self) -> &'static str {
        "aruco"
    }

    fn install(&self, registry: &mut PluginRegistry) -> Result<(), &'static str> {
        self.install(registry)
    }
}
