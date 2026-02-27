use lib_cv::modules::localization::{CameraIntrinsics, MarkerDefinition, MarkerMap, MarkerObservation, PixelCoordinate, PnpRefineConfig, PoseEstimationMethod, RansacConfig};
use lib_cv::{Rotation3, Translation3};
use nalgebra::{Matrix3, Rotation3 as NaRotation3, Vector3};

fn build_map() -> MarkerMap {
    MarkerMap {
        markers: vec![
            MarkerDefinition::new(1, Translation3 { x: 4.0, y: 5.0, z: 6.0 }, None),
            MarkerDefinition::new(2, Translation3 { x: 2.0, y: 8.0, z: 7.0 }, None),
            MarkerDefinition::new(3, Translation3 { x: -1.0, y: 4.0, z: 3.5 }, None),
            MarkerDefinition::new(4, Translation3 { x: -2.0, y: 6.5, z: 5.0 }, None),
            MarkerDefinition::new(5, Translation3 { x: 3.5, y: -1.0, z: 4.5 }, None),
            MarkerDefinition::new(6, Translation3 { x: -3.0, y: 3.0, z: 6.0 }, None),
        ],
    }
}

#[test]
fn parses_marker_map_from_json() {
    let json = r#"
    {
        "markers": [
            {
                "id": 10,
                "translation": { "x": 1.0, "y": 2.0, "z": 3.0 },
                "rotation": { "roll": 0.0, "pitch": 0.0, "yaw": 0.0 }
            }
        ]
    }"#;

    let map = MarkerMap::from_json_str(json).unwrap();
    assert_eq!(map.markers.len(), 1);
    assert_eq!(map.markers[0].id, 10);
    assert_eq!(map.markers[0].translation.x, 1.0);
}

#[test]
fn centroid_solver_recovers_translation() {
    let map = build_map();
    let camera_translation = Translation3 { x: 1.0, y: 2.0, z: 3.0 };
    let marker = &map.markers[0];
    let observation = MarkerObservation::translation(
        marker.id,
        Translation3 { x: marker.translation.x - camera_translation.x, y: marker.translation.y - camera_translation.y, z: marker.translation.z - camera_translation.z },
    );

    let pose = map.estimate_pose(&[observation], PoseEstimationMethod::Centroid).unwrap();
    assert!(approx_translation(pose.translation, camera_translation));
    assert_eq!(pose.rotation, Rotation3::default());
}

#[test]
fn procrustes_solver_recovers_pose() {
    let map = build_map();
    let rotation = Matrix3::new(
        0.0, -1.0, 0.0, //
        1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0,
    );
    let camera_position = Vector3::new(1.0, 2.0, 0.5);

    let observations: Vec<_> = map
        .markers
        .iter()
        .take(3)
        .map(|marker| {
            let world = Vector3::new(marker.translation.x, marker.translation.y, marker.translation.z);
            let camera = rotation.transpose() * (world - camera_position);
            MarkerObservation::translation(marker.id, Translation3 { x: camera.x, y: camera.y, z: camera.z })
        })
        .collect();

    let pose = map.estimate_pose(&observations, PoseEstimationMethod::RigidProcrustes).unwrap();
    assert!(approx_translation(pose.translation, Translation3 { x: camera_position.x, y: camera_position.y, z: camera_position.z }));
    assert!(approx_angle(pose.rotation.roll, 0.0));
    assert!(approx_angle(pose.rotation.pitch, 0.0));
    assert!(approx_angle(pose.rotation.yaw, std::f64::consts::FRAC_PI_2));
}

#[test]
fn pnp_solver_recovers_pose() {
    let map = build_map();
    let intrinsics = CameraIntrinsics::new(800.0, 800.0, 320.0, 240.0);
    let yaw = std::f64::consts::FRAC_PI_4;
    let camera_rotation = NaRotation3::from_euler_angles(0.0, 0.0, yaw);
    let camera_position = Vector3::new(0.25, -0.15, 2.5);
    let world_to_camera = camera_rotation.transpose();

    let observations: Vec<_> = map
        .markers
        .iter()
        .map(|marker| {
            let world = Vector3::new(marker.translation.x, marker.translation.y, marker.translation.z);
            let cam = world_to_camera * (world - camera_position);
            let u = intrinsics.fx * (cam.x / cam.z) + intrinsics.cx;
            let v = intrinsics.fy * (cam.y / cam.z) + intrinsics.cy;
            MarkerObservation::pixel(marker.id, PixelCoordinate::new(u, v))
        })
        .collect();

    let pose = map.estimate_pose(&observations, PoseEstimationMethod::PerspectiveNPoint(intrinsics)).unwrap();

    assert!(approx_translation(pose.translation, Translation3 { x: camera_position.x, y: camera_position.y, z: camera_position.z }));
    assert!(approx_angle(pose.rotation.roll, 0.0));
    assert!(approx_angle(pose.rotation.pitch, 0.0));
    assert!(approx_angle(pose.rotation.yaw, yaw));
}

#[test]
fn pnp_ransac_handles_outliers() {
    let map = build_map();
    let intrinsics = CameraIntrinsics::new(820.0, 815.0, 320.0, 240.0);
    let yaw = std::f64::consts::FRAC_PI_6;
    let pitch = -0.1;
    let roll = 0.05;
    let camera_rotation = NaRotation3::from_euler_angles(roll, pitch, yaw);
    let camera_position = Vector3::new(0.45, -0.3, 2.8);
    let world_to_camera = camera_rotation.transpose();

    let observations: Vec<_> = map
        .markers
        .iter()
        .enumerate()
        .map(|(idx, marker)| {
            let world = Vector3::new(marker.translation.x, marker.translation.y, marker.translation.z);
            let cam = world_to_camera * (world - camera_position);
            let mut pixel = PixelCoordinate::new(intrinsics.fx * (cam.x / cam.z) + intrinsics.cx, intrinsics.fy * (cam.y / cam.z) + intrinsics.cy);

            if idx == 1 {
                pixel = PixelCoordinate::new(1200.0, -800.0);
            } else if idx == 4 {
                pixel = PixelCoordinate::new(-450.0, 900.0);
            }
            MarkerObservation::pixel(marker.id, pixel)
        })
        .collect();

    let config = RansacConfig::new(128, 3.5, 4);
    let pose = map.estimate_pose(&observations, PoseEstimationMethod::PerspectiveNPointRansac { intrinsics, config }).unwrap();

    assert!(approx_translation_eps(pose.translation, Translation3 { x: camera_position.x, y: camera_position.y, z: camera_position.z }, 1e-3));
    assert!(approx_angle_eps(pose.rotation.roll, roll, 1e-3));
    assert!(approx_angle_eps(pose.rotation.pitch, pitch, 1e-3));
    assert!(approx_angle_eps(pose.rotation.yaw, yaw, 1e-3));
}

#[test]
fn pnp_ransac_refine_handles_outliers() {
    let map = build_map();
    let intrinsics = CameraIntrinsics::new(820.0, 815.0, 320.0, 240.0);
    let yaw = std::f64::consts::FRAC_PI_6;
    let pitch = -0.1;
    let roll = 0.05;
    let camera_rotation = NaRotation3::from_euler_angles(roll, pitch, yaw);
    let camera_position = Vector3::new(0.45, -0.3, 2.8);
    let world_to_camera = camera_rotation.transpose();

    let observations: Vec<_> = map
        .markers
        .iter()
        .enumerate()
        .map(|(idx, marker)| {
            let world = Vector3::new(marker.translation.x, marker.translation.y, marker.translation.z);
            let cam = world_to_camera * (world - camera_position);
            let mut pixel = PixelCoordinate::new(intrinsics.fx * (cam.x / cam.z) + intrinsics.cx, intrinsics.fy * (cam.y / cam.z) + intrinsics.cy);

            if idx == 1 {
                pixel = PixelCoordinate::new(1200.0, -800.0);
            } else if idx == 4 {
                pixel = PixelCoordinate::new(-450.0, 900.0);
            }
            MarkerObservation::pixel(marker.id, pixel)
        })
        .collect();

    let config = RansacConfig::new(128, 3.5, 4);
    let refine = PnpRefineConfig::default();
    let pose = map.estimate_pose(&observations, PoseEstimationMethod::PerspectiveNPointRansacRefine { intrinsics, config, refine }).unwrap();

    assert!(approx_translation_eps(pose.translation, Translation3 { x: camera_position.x, y: camera_position.y, z: camera_position.z }, 1e-3));
    assert!(approx_angle_eps(pose.rotation.roll, roll, 1e-3));
    assert!(approx_angle_eps(pose.rotation.pitch, pitch, 1e-3));
    assert!(approx_angle_eps(pose.rotation.yaw, yaw, 1e-3));
}

fn approx_translation(lhs: Translation3, rhs: Translation3) -> bool {
    (lhs.x - rhs.x).abs() < 1e-6 && (lhs.y - rhs.y).abs() < 1e-6 && (lhs.z - rhs.z).abs() < 1e-6
}

fn approx_angle(lhs: f64, rhs: f64) -> bool {
    (lhs - rhs).abs() < 1e-6
}

fn approx_translation_eps(lhs: Translation3, rhs: Translation3, eps: f64) -> bool {
    (lhs.x - rhs.x).abs() < eps && (lhs.y - rhs.y).abs() < eps && (lhs.z - rhs.z).abs() < eps
}

fn approx_angle_eps(lhs: f64, rhs: f64, eps: f64) -> bool {
    (lhs - rhs).abs() < eps
}
