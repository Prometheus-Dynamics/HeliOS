use super::*;

#[derive(Clone, Debug, NodeConfig)]
struct ArucoTagPosesConfig {
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.001))]
    tag_size_m: f64,
    #[port(default = "auto")]
    solver: TagPoseMethod,
}

fn camera_calibration_from_daedalus_value(value: &daedalus::data::model::Value) -> Result<CameraCalibration, NodeError> {
    use daedalus::data::model::{EnumValue as DaedalusEnumValue, StructFieldValue as DaedalusStructFieldValue, Value as DaedalusValue};

    fn get_field<'a>(fields: &'a [DaedalusStructFieldValue], name: &str) -> Option<&'a DaedalusValue> {
        fields.iter().find(|f| f.name.eq_ignore_ascii_case(name)).map(|f| &f.value)
    }

    fn as_f32(v: &DaedalusValue) -> Option<f32> {
        match v {
            DaedalusValue::Float(f) => Some(*f as f32),
            DaedalusValue::Int(i) => Some(*i as f32),
            _ => None,
        }
    }

    fn as_i64(v: &DaedalusValue) -> Option<i64> {
        match v {
            DaedalusValue::Int(i) => Some(*i),
            DaedalusValue::Float(f) => Some(*f as i64),
            _ => None,
        }
    }

    fn parse_lens_model(v: &DaedalusValue) -> Option<LensModel> {
        match v {
            // Newer Daedalus commonly represents enums by discriminant.
            DaedalusValue::Int(0) => Some(LensModel::Pinhole),
            DaedalusValue::Int(1) => Some(LensModel::Fisheye),
            DaedalusValue::Enum(DaedalusEnumValue { name, .. }) => match name.trim().to_ascii_lowercase().as_str() {
                "pinhole" => Some(LensModel::Pinhole),
                "fisheye" => Some(LensModel::Fisheye),
                _ => None,
            },
            DaedalusValue::String(s) => match s.trim().to_ascii_lowercase().as_str() {
                "pinhole" => Some(LensModel::Pinhole),
                "fisheye" => Some(LensModel::Fisheye),
                _ => None,
            },
            _ => None,
        }
    }

    let fields = match value {
        DaedalusValue::Struct(fields) => fields.as_slice(),
        // Some host bridges emitted maps; accept those too.
        DaedalusValue::Map(entries) => {
            // Convert to a temporary struct-like list (string keys only).
            let mut tmp = Vec::with_capacity(entries.len());
            for (k, v) in entries {
                let DaedalusValue::String(name) = k else { continue };
                tmp.push(DaedalusStructFieldValue { name: name.to_string(), value: v.clone() });
            }
            return camera_calibration_from_daedalus_value(&DaedalusValue::Struct(tmp));
        }
        _ => return Err(NodeError::InvalidInput("calibration must be a struct".into())),
    };

    let fx = get_field(fields, "fx").and_then(as_f32).ok_or_else(|| NodeError::InvalidInput("calibration.fx missing/invalid".into()))?;
    let fy = get_field(fields, "fy").and_then(as_f32).ok_or_else(|| NodeError::InvalidInput("calibration.fy missing/invalid".into()))?;
    let cx = get_field(fields, "cx").and_then(as_f32).ok_or_else(|| NodeError::InvalidInput("calibration.cx missing/invalid".into()))?;
    let cy = get_field(fields, "cy").and_then(as_f32).ok_or_else(|| NodeError::InvalidInput("calibration.cy missing/invalid".into()))?;
    let k1 = get_field(fields, "k1").and_then(as_f32).ok_or_else(|| NodeError::InvalidInput("calibration.k1 missing/invalid".into()))?;
    let k2 = get_field(fields, "k2").and_then(as_f32).ok_or_else(|| NodeError::InvalidInput("calibration.k2 missing/invalid".into()))?;
    let p1 = get_field(fields, "p1").and_then(as_f32).ok_or_else(|| NodeError::InvalidInput("calibration.p1 missing/invalid".into()))?;
    let p2 = get_field(fields, "p2").and_then(as_f32).unwrap_or(0.0);
    let k3 = get_field(fields, "k3").and_then(as_f32).ok_or_else(|| NodeError::InvalidInput("calibration.k3 missing/invalid".into()))?;

    let undistort_iters = get_field(fields, "undistortIters").and_then(as_i64).and_then(|v| u8::try_from(v).ok()).unwrap_or(5);
    let lens_model = get_field(fields, "lensModel").and_then(parse_lens_model).unwrap_or(LensModel::Pinhole);

    Ok(CameraCalibration { fx, fy, cx, cy, k1, k2, p1, p2, k3, undistort_iters, lens_model })
}

#[node(
    id = "tag_poses",
    inputs(
        port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        port(name = "calibration", source = "Calibration", ty = crate::daedalus_types::camera_calibration()),
        config = ArucoTagPosesConfig
    ),
    outputs(port(name = "poses", source = "DetectionPoses", ty = crate::daedalus_types::detection_pose_output()))
)]
fn cv_aruco_tag_poses(detections: &Vec<ArucoDetection2D>, calibration: daedalus::data::model::Value, cfg: ArucoTagPosesConfig) -> Result<DetectionPoseOutput, NodeError> {
    let calibration = camera_calibration_from_daedalus_value(&calibration)?;
    let calib = TagPoseCalibration {
        fx: calibration.fx as f64,
        fy: calibration.fy as f64,
        cx: calibration.cx as f64,
        cy: calibration.cy as f64,
        k1: calibration.k1 as f64,
        k2: calibration.k2 as f64,
        p1: calibration.p1 as f64,
        p2: calibration.p2 as f64,
        k3: calibration.k3 as f64,
        undistort_iters: calibration.undistort_iters.max(1),
        lens_model: calibration.lens_model,
    };

    let method = cfg.solver.resolve();
    Ok(detections_to_tag_pose_output(detections, calib, cfg.tag_size_m, method))
}
