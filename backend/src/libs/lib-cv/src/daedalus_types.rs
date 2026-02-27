use daedalus::data::model::{StructField, TypeExpr, ValueType};

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

pub fn pixel_rgba() -> TypeExpr {
    TypeExpr::Struct(vec![
        StructField { name: "r".into(), ty: TypeExpr::Scalar(ValueType::Int) },
        StructField { name: "g".into(), ty: TypeExpr::Scalar(ValueType::Int) },
        StructField { name: "b".into(), ty: TypeExpr::Scalar(ValueType::Int) },
        StructField { name: "a".into(), ty: TypeExpr::Scalar(ValueType::Int) },
    ])
}
