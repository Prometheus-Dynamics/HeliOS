use super::*;

fn refinement_is_not_worse(original: &ArucoDetectionF32, refined: &ArucoDetectionF32) -> bool {
    if let (Some(orig), Some(new)) = (original.best_distance, refined.best_distance)
        && new > orig
    {
        return false;
    }
    if let (Some(orig), Some(new)) = (original.border_mismatches, refined.border_mismatches)
        && new > orig
    {
        return false;
    }
    if let (Some(orig), Some(new)) = (original.second_distance, refined.second_distance)
        && new < orig
    {
        return false;
    }
    if let (Some(orig), Some(new)) = (original.score, refined.score)
        && new < orig
    {
        return false;
    }
    if let (Some(orig), Some(new)) = (original.contrast_range, refined.contrast_range)
        && new < orig
    {
        return false;
    }
    true
}

fn refine_marker_corners_warp_f32(frame: &DynamicImage, markers: &mut [ArucoDetectionF32], sample_scale: u32, family: &ArucoTagFamily, config: &ArucoTagDecodeConfig) -> usize {
    if markers.is_empty() {
        return 0;
    }
    with_luma8_frame(frame, |gray| {
        let mut refined = 0usize;
        for marker in markers.iter_mut() {
            let Some(corners) = &marker.corners else {
                continue;
            };
            if corners.len() < 4 {
                continue;
            }
            let original = marker.clone();

            let quad = [corners[0], corners[1], corners[2], corners[3]];
            let Ok(warped) = decode_quad_warp_result(gray, &quad, sample_scale, family, config) else {
                continue;
            };
            if warped.id != marker.id {
                continue;
            }
            let keep_by_quality = refinement_is_not_worse(&original, &warped);
            if let Some(warped_corners) = warped.corners.as_ref()
                && warped_corners.len() >= 4
            {
                let refined_quad = [warped_corners[0], warped_corners[1], warped_corners[2], warped_corners[3]];
                if keep_by_quality && refined_quad_is_reasonable(&quad, &refined_quad) {
                    marker.corners = Some(warped_corners.clone());
                    refined += 1;
                }
            }
        }

        refined
    })
}

fn refine_marker_corners_warp_aruco_f32(frame: &DynamicImage, markers: &mut [ArucoDetectionF32], sample_scale: u32, dict: &ArucoDictionary, cfg: &ArucoDecodeConfig) -> usize {
    if markers.is_empty() {
        return 0;
    }
    with_luma8_frame(frame, |gray| {
        let mut refined = 0usize;
        for marker in markers.iter_mut() {
            let Some(corners) = &marker.corners else {
                continue;
            };
            if corners.len() < 4 {
                continue;
            }
            let original = marker.clone();
            let quad = [corners[0], corners[1], corners[2], corners[3]];
            let Ok(warped) = decode_quad_aruco_warp_result(gray, &quad, sample_scale, dict, cfg) else {
                continue;
            };
            if warped.id != marker.id {
                continue;
            }
            let keep_by_quality = refinement_is_not_worse(&original, &warped);
            if let Some(warped_corners) = warped.corners.as_ref()
                && warped_corners.len() >= 4
            {
                let refined_quad = [warped_corners[0], warped_corners[1], warped_corners[2], warped_corners[3]];
                if keep_by_quality && refined_quad_is_reasonable(&quad, &refined_quad) {
                    marker.corners = Some(warped_corners.clone());
                    refined += 1;
                }
            }
        }
        refined
    })
}

pub fn refine_detection_corners_warp(frame: &DynamicImage, detections: &mut [ArucoDetection2D], sample_scale: u32, family: &ArucoTagFamily, config: &ArucoTagDecodeConfig) -> usize {
    if detections.is_empty() {
        return 0;
    }
    let mut tmp: Vec<ArucoDetectionF32> = Vec::with_capacity(detections.len());
    for det in detections.iter() {
        tmp.push(ArucoDetectionF32 {
            id: det.id,
            rotation: det.rotation,
            border_width: det.border_width.unwrap_or_else(|| family.border_size()),
            data_width: det.data_width.unwrap_or_else(|| family.data_width()),
            corners: Some(vec![
                Point::new(det.corners[0].x as f32, det.corners[0].y as f32),
                Point::new(det.corners[1].x as f32, det.corners[1].y as f32),
                Point::new(det.corners[2].x as f32, det.corners[2].y as f32),
                Point::new(det.corners[3].x as f32, det.corners[3].y as f32),
            ]),
            score: det.score,
            best_distance: det.best_distance,
            second_distance: det.second_distance,
            border_mismatches: det.border_mismatches,
            contrast_range: det.contrast_range,
        });
    }
    let refined = refine_marker_corners_warp_f32(frame, &mut tmp, sample_scale, family, config);
    for (dst, src) in detections.iter_mut().zip(tmp.iter()) {
        if let Some(corners) = &src.corners
            && corners.len() >= 4
        {
            dst.corners = [
                crate::Point { x: corners[0].x as f64, y: corners[0].y as f64 },
                crate::Point { x: corners[1].x as f64, y: corners[1].y as f64 },
                crate::Point { x: corners[2].x as f64, y: corners[2].y as f64 },
                crate::Point { x: corners[3].x as f64, y: corners[3].y as f64 },
            ];
        }
    }
    refined
}

pub fn refine_detection_corners_warp_aruco(frame: &DynamicImage, detections: &mut [ArucoDetection2D], sample_scale: u32, dict: &ArucoDictionary, cfg: &ArucoDecodeConfig) -> usize {
    if detections.is_empty() {
        return 0;
    }
    let mut tmp: Vec<ArucoDetectionF32> = Vec::with_capacity(detections.len());
    for det in detections.iter() {
        tmp.push(ArucoDetectionF32 {
            id: det.id,
            rotation: det.rotation,
            border_width: det.border_width.unwrap_or(1),
            data_width: det.data_width.unwrap_or(dict.marker_size()),
            corners: Some(vec![
                Point::new(det.corners[0].x as f32, det.corners[0].y as f32),
                Point::new(det.corners[1].x as f32, det.corners[1].y as f32),
                Point::new(det.corners[2].x as f32, det.corners[2].y as f32),
                Point::new(det.corners[3].x as f32, det.corners[3].y as f32),
            ]),
            score: det.score,
            best_distance: det.best_distance,
            second_distance: det.second_distance,
            border_mismatches: det.border_mismatches,
            contrast_range: det.contrast_range,
        });
    }
    let refined = refine_marker_corners_warp_aruco_f32(frame, &mut tmp, sample_scale, dict, cfg);
    for (dst, src) in detections.iter_mut().zip(tmp.iter()) {
        if let Some(corners) = &src.corners
            && corners.len() >= 4
        {
            dst.corners = [
                crate::Point { x: corners[0].x as f64, y: corners[0].y as f64 },
                crate::Point { x: corners[1].x as f64, y: corners[1].y as f64 },
                crate::Point { x: corners[2].x as f64, y: corners[2].y as f64 },
                crate::Point { x: corners[3].x as f64, y: corners[3].y as f64 },
            ];
        }
    }
    refined
}
