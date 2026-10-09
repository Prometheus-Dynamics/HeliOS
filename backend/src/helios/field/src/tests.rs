use eidos_calibration::{
    CalibrationStatus, CameraCalibration, CameraIntrinsics, DistortionCoefficients, LensModel, MultiTagPose, MultiTagPoseConfig, MultiTagPoseSolver, Point3, TagCorners, project_camera_point_checked,
    rotation_angle_between, se3_inverse, transform_point,
};

use super::*;

fn v(x: f64, y: f64, z: f64) -> [f64; 3] {
    [x, y, z]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unit(a: [f64; 3]) -> [f64; 3] {
    let n = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    scale(a, 1.0 / n)
}

fn column(r: &RotationMatrix3, j: usize) -> [f64; 3] {
    [r.m[0][j], r.m[1][j], r.m[2][j]]
}

fn translation(t: &Se3Transform) -> [f64; 3] {
    [t.translation.x, t.translation.y, t.translation.z]
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = sub(a, b);
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// A camera whose optical axis looks from `eye` at `target`, image up = field up: the camera's
/// optical frame (x right, y down, z forward) in the field.
fn field_from_camera_looking_at(eye: [f64; 3], target: [f64; 3]) -> Se3Transform {
    let forward = unit(sub(target, eye));
    let right = unit(cross(forward, v(0.0, 0.0, 1.0)));
    let down = cross(forward, right);
    let m = [[right[0], down[0], forward[0]], [right[1], down[1], forward[1]], [right[2], down[2], forward[2]]];
    Se3Transform { rotation: RotationMatrix3 { m }, translation: Translation3 { x: eye[0], y: eye[1], z: eye[2] } }
}

fn pinhole() -> CameraCalibration {
    CameraCalibration {
        intrinsics: CameraIntrinsics { fx: 910.0, fy: 910.0, cx: 639.5, cy: 399.5, skew: 0.0 },
        distortion: DistortionCoefficients { k1: -0.12, k2: 0.03, k3: 0.0, k4: 0.0, k5: 0.0, k6: 0.0, p1: 0.0, p2: 0.0 },
        undistort_iters: 5,
        lens_model: LensModel::Pinhole,
    }
}

/// What a camera at `field_from_camera` sees of `tag`, with the corners built from WPILib's tag
/// convention only (the tag's centre, its face direction `+X_wpilib`, up `+Z_wpilib`, the square
/// spanned by `±Y_wpilib` and `±Z_wpilib`), not from `wpilib_tag_from_eidos_tag`. They are
/// numbered by where they land in the image, as a detector numbers an upright marker: corner 0
/// top-left on screen, then clockwise. The robot and camera in these tests stand upright, so the
/// marker is upright in the image.
fn observe(tag: &FieldTag, field_from_camera: &Se3Transform, calibration: &CameraCalibration) -> Option<TagCorners> {
    let field_from_tag = tag.field_from_tag.to_se3();
    let centre = translation(&field_from_tag);
    let face = column(&field_from_tag.rotation, 0);
    let up = column(&field_from_tag.rotation, 2);
    let side_axis = column(&field_from_tag.rotation, 1);
    let half = tag.side_m / 2.0;
    // The camera must be in front of the face to see it.
    if (0..3).map(|i| (translation(field_from_camera)[i] - centre[i]) * face[i]).sum::<f64>() <= 0.0 {
        return None;
    }
    // The square's corners in no particular order, projected.
    let camera_from_field = se3_inverse(*field_from_camera);
    let mut pixels = Vec::with_capacity(4);
    for (a, b) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
        let p = add(add(centre, scale(side_axis, a * half)), scale(up, b * half));
        let pixel = project_camera_point_checked(transform_point(&camera_from_field, Point3 { x: p[0], y: p[1], z: p[2] }), calibration)?;
        if !(0.0..1280.0).contains(&pixel.u) || !(0.0..800.0).contains(&pixel.v) {
            return None;
        }
        pixels.push(pixel);
    }
    // Named as a detector names an upright marker in an upright image: the top two (smaller v),
    // left (smaller u) first, then the bottom right and the bottom left (clockwise on screen).
    pixels.sort_by(|a, b| a.v.total_cmp(&b.v));
    let (top, bottom) = pixels.split_at_mut(2);
    top.sort_by(|a, b| a.u.total_cmp(&b.u));
    bottom.sort_by(|a, b| b.u.total_cmp(&a.u));
    Some(TagCorners { id: tag.id, corners: [top[0], top[1], bottom[0], bottom[1]] })
}

fn solve(layout: &FieldLayout, observations: &[TagCorners], calibration: &CameraCalibration, extrinsics: Option<&CameraExtrinsics>) -> MultiTagPose {
    let mut solver = MultiTagPoseSolver::new(layout.known_tags(), MultiTagPoseConfig::default());
    let mut out = MultiTagPose::default();
    solver.solve_into(observations, calibration, extrinsics, &mut out);
    out
}

#[test]
fn the_tag_rotation_maps_the_documented_axes() {
    let r = wpilib_tag_from_eidos_tag();
    assert!(is_rotation(&r));
    // Eidos +X (top edge, viewer's right) = WPILib +Y; Eidos +Y (down) = -Z; Eidos +Z (into the
    // marker) = -X (the WPILib tag faces +X).
    assert_eq!(column(&r, 0), v(0.0, 1.0, 0.0));
    assert_eq!(column(&r, 1), v(0.0, 0.0, -1.0));
    assert_eq!(column(&r, 2), v(-1.0, 0.0, 0.0));
    let q = Pose::from_se3(&Se3Transform { rotation: r, translation: Translation3 { x: 0.0, y: 0.0, z: 0.0 } }).rotation;
    for (got, want) in [q.w, q.x, q.y, q.z].into_iter().zip([0.5, -0.5, -0.5, 0.5]) {
        assert!((got - want).abs() < 1e-12, "{q:?}");
    }
}

#[test]
fn synthetic_round_trip_recovers_the_camera_and_the_robot() {
    let layout = FieldLayout::frc_2026_andymark();
    let calibration = pinhole();
    // A robot 3 m from the red alliance wall, facing it and turned a little, with a camera
    // mounted 0.25 m forward, 0.1 m left, 0.5 m up, pitched up 15 degrees.
    let mount = CameraMount { x_m: 0.25, y_m: 0.1, z_m: 0.5, roll_rad: 0.0, pitch_rad: -15f64.to_radians(), yaw_rad: 0.0 };
    for (robot_x, robot_y, robot_yaw_deg) in [(13.4, 4.1, 0.0), (12.9, 4.6, -8.0), (13.8, 3.6, 8.0)] {
        let (s, c) = f64::to_radians(robot_yaw_deg).sin_cos();
        let field_from_robot = Se3Transform { rotation: RotationMatrix3 { m: [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]] }, translation: Translation3 { x: robot_x, y: robot_y, z: 0.0 } };
        let field_from_camera = compose(field_from_robot, mount.extrinsics().rig_from_camera);
        let observations = layout.tags.iter().filter_map(|tag| observe(tag, &field_from_camera, &calibration)).collect::<Vec<_>>();
        assert!(observations.len() >= 2, "the camera sees red-wall tags from ({robot_x}, {robot_y}): {observations:?}");
        let pose = solve(&layout, &observations, &calibration, Some(&mount.extrinsics()));
        assert_eq!(pose.status, CalibrationStatus::Calibrated);
        assert!(pose.valid, "{pose:?}");
        assert_eq!(pose.inlier_tags as usize, observations.len(), "every seen tag agrees with the joint pose: {pose:?}");
        assert!(pose.rms_px < 1e-3, "noise-free corners fit: {}", pose.rms_px);
        let camera = pose.reference_from_camera;
        assert!(distance(translation(&camera), translation(&field_from_camera)) < 1e-4, "camera in field {:?} vs {:?}", camera.translation, field_from_camera.translation);
        assert!(rotation_angle_between(&camera.rotation, &field_from_camera.rotation) < 1e-5);
        let robot = pose.reference_from_rig.expect("rig pose with extrinsics");
        assert!(distance(translation(&robot), translation(&field_from_robot)) < 1e-4, "robot in field {:?}", robot.translation);
        assert!(rotation_angle_between(&robot.rotation, &field_from_robot.rotation) < 1e-5);
    }
}

#[test]
fn the_wrong_tag_rotation_does_not_fit() {
    // Without the tag rotation (Eidos's tag frame taken to be WPILib's) the same corners do not
    // give the camera back: the conversion is what makes the round trip work.
    let layout = FieldLayout::frc_2026_andymark();
    let calibration = pinhole();
    let field_from_camera = field_from_camera_looking_at(v(13.5, 4.1, 0.6), v(16.5, 4.1, 0.55));
    let observations = layout.tags.iter().filter_map(|tag| observe(tag, &field_from_camera, &calibration)).collect::<Vec<_>>();
    assert!(observations.len() >= 2);
    let unconverted = KnownTagPoses::new(layout.tags.iter().map(|tag| KnownTag { id: tag.id, side: tag.side_m, reference_from_tag: tag.field_from_tag.to_se3() }).collect());
    let mut solver = MultiTagPoseSolver::new(unconverted, MultiTagPoseConfig::default());
    let mut out = MultiTagPose::default();
    solver.solve_into(&observations, &calibration, None, &mut out);
    let off = !out.valid || distance(translation(&out.reference_from_camera), translation(&field_from_camera)) > 0.1;
    assert!(off, "{out:?}");
    let converted = solve(&layout, &observations, &calibration, None);
    assert!(distance(translation(&converted.reference_from_camera), translation(&field_from_camera)) < 1e-4);
}

#[test]
fn the_2026_layout_tags_face_into_the_field() {
    let layout = FieldLayout::frc_2026_andymark();
    assert_eq!(layout.tags.len(), 32);
    assert_eq!((layout.length_m, layout.width_m), (16.518, 8.043));
    for tag in &layout.tags {
        assert!((tag.side_m - FRC_TAG_SIDE_M).abs() < 1e-9, "tag {}", tag.id);
        let t = tag.field_from_tag.translation;
        assert!((0.0..=layout.length_m).contains(&t.x) && (0.0..=layout.width_m).contains(&t.y) && t.z > 0.3 && t.z < 1.5, "tag {} inside the field: {t:?}", tag.id);
        let eidos = tag.field_from_eidos_tag().rotation;
        // Upright: Eidos +Y (down the marker) is the field's -Z; the face is horizontal.
        assert!(distance(column(&eidos, 1), v(0.0, 0.0, -1.0)) < 1e-9, "tag {} upright", tag.id);
        // The printed face looks along -Z_eidos, which is WPILib's +X.
        let face = scale(column(&eidos, 2), -1.0);
        let n = tag.normal();
        assert!(distance(face, v(n.x, n.y, n.z)) < 1e-9, "tag {}", tag.id);
    }
    // Alliance walls: red (13-16) at the far end facing the blue end (-X), blue (29-32) at the
    // origin end facing +X.
    for (ids, x_near, facing) in [(13..=16, layout.length_m, -1.0), (29..=32, 0.0, 1.0)] {
        for id in ids {
            let tag = layout.tag(id).expect("tag");
            assert!((tag.field_from_tag.translation.x - x_near).abs() < 0.05, "tag {id} on its wall: {:?}", tag.field_from_tag.translation);
            let face = scale(column(&tag.field_from_eidos_tag().rotation, 2), -1.0);
            assert!(distance(face, v(facing, 0.0, 0.0)) < 1e-9, "tag {id} faces into the field: {face:?}");
        }
    }
    // Tag 1 (WPILib yaw 180 degrees): Eidos +Z into the tag is +X, its top edge runs -Y.
    let r = layout.tag(1).expect("tag 1").field_from_eidos_tag().rotation;
    assert!(distance(column(&r, 2), v(1.0, 0.0, 0.0)) < 1e-9 && distance(column(&r, 0), v(0.0, -1.0, 0.0)) < 1e-9, "{r:?}");
}

#[test]
fn wpilib_json_and_fmap_agree() {
    // Tag 13 of the 2026 field as WPILib's JSON writes it (blue origin, yaw 180 degrees).
    let fmap = FieldLayout::frc_2026_andymark();
    let tag = fmap.tag(13).expect("tag 13");
    let t = tag.field_from_tag.translation;
    let json = format!(
        r#"{{"tags":[{{"ID":13,"pose":{{"translation":{{"x":{},"y":{},"z":{}}},"rotation":{{"quaternion":{{"W":0.0,"X":0.0,"Y":0.0,"Z":1.0}}}}}}}}],"field":{{"length":16.518,"width":8.043}}}}"#,
        t.x, t.y, t.z
    );
    let (wpilib, format) = FieldLayout::parse(&json, FRC_TAG_SIDE_M).expect("wpilib json");
    assert_eq!(format, LayoutFormat::Wpilib);
    let a = wpilib.known_tags().tags[0];
    let b = fmap.known_tags().tag(13).copied().expect("tag 13");
    assert!(distance(translation(&a.reference_from_tag), translation(&b.reference_from_tag)) < 1e-9);
    assert!(rotation_angle_between(&a.reference_from_tag.rotation, &b.reference_from_tag.rotation) < 1e-9);
    assert_eq!(a.side, b.side);
    let (_, format) = FieldLayout::parse(FRC_2026_ANDYMARK_FMAP, FRC_TAG_SIDE_M).expect("fmap");
    assert_eq!(format, LayoutFormat::Fmap);
}

#[test]
fn layouts_are_validated() {
    assert!(matches!(FieldLayout::parse("{}", FRC_TAG_SIDE_M), Err(LayoutError::UnknownFormat)));
    assert!(matches!(FieldLayout::parse("not json", FRC_TAG_SIDE_M), Err(LayoutError::Json(_))));
    let twice = r#"{"tags":[{"ID":1,"pose":{"translation":{"x":1,"y":1,"z":1},"rotation":{"quaternion":{"W":1,"X":0,"Y":0,"Z":0}}}},{"ID":1,"pose":{"translation":{"x":2,"y":1,"z":1},"rotation":{"quaternion":{"W":1,"X":0,"Y":0,"Z":0}}}}],"field":{"length":16.5,"width":8.0}}"#;
    assert!(FieldLayout::from_wpilib_json(twice, FRC_TAG_SIDE_M).is_err());
    let zero_q = r#"{"tags":[{"ID":1,"pose":{"translation":{"x":1,"y":1,"z":1},"rotation":{"quaternion":{"W":0,"X":0,"Y":0,"Z":0}}}}],"field":{"length":16.5,"width":8.0}}"#;
    assert!(FieldLayout::from_wpilib_json(zero_q, FRC_TAG_SIDE_M).is_err());
    let no_size = r#"{"fiducials":[{"id":1,"size":165.1,"transform":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}]}"#;
    assert!(FieldLayout::from_fmap(no_size).is_err(), "a field-centred .fmap needs the field size");
    let skewed = r#"{"fiducials":[{"id":1,"size":165.1,"transform":[2,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"fieldlength":16.5,"fieldwidth":8.0}"#;
    assert!(FieldLayout::from_fmap(skewed).is_err());
}

#[test]
fn known_tags_value_is_the_eidos_graph_constant_shape() {
    let layout = FieldLayout::frc_2026_andymark();
    let value = layout.known_tags_value();
    let first = &value[0];
    assert_eq!(first["id"], 1);
    assert!((first["side"].as_f64().expect("side") - FRC_TAG_SIDE_M).abs() < 1e-12);
    for key in ["x", "y", "z"] {
        assert!(first["reference_from_tag"]["translation"][key].is_f64(), "{first}");
    }
    for key in ["w", "x", "y", "z"] {
        assert!(first["reference_from_tag"]["rotation"][key].is_f64(), "{first}");
    }
    // The constant round-trips to the same poses.
    let pose: Pose = serde_json::from_value(first["reference_from_tag"].clone()).expect("pose");
    let known = layout.known_tags();
    assert!(rotation_angle_between(&pose.to_se3().rotation, &known.tags[0].reference_from_tag.rotation) < 1e-9);
}

#[test]
fn mount_angles_follow_wpilib() {
    // Pitch +15 degrees tilts the camera down: its optical axis (FLU +X) points below horizontal.
    let down = CameraMount { pitch_rad: 15f64.to_radians(), ..CameraMount::default() };
    let axis = column(&down.robot_from_camera_flu().rotation, 0);
    assert!(axis[2] < -0.25 && axis[0] > 0.9, "{axis:?}");
    // Yaw 90 degrees faces left (+Y).
    let left = CameraMount { yaw_rad: std::f64::consts::FRAC_PI_2, ..CameraMount::default() };
    assert!(distance(column(&left.robot_from_camera_flu().rotation, 0), v(0.0, 1.0, 0.0)) < 1e-12);
    // The optical axis (optical +Z) of an unrotated mount is the robot's forward.
    let ahead = CameraMount { x_m: 0.3, ..CameraMount::default() }.extrinsics();
    assert!(distance(column(&ahead.rig_from_camera.rotation, 2), v(1.0, 0.0, 0.0)) < 1e-12);
    assert!(distance(translation(&ahead.rig_from_camera), v(0.3, 0.0, 0.0)) < 1e-12);
}
