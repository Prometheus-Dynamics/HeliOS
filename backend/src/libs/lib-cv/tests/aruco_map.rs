use lib_cv::modules::aruco::map::{ArucoMap, MarkerPose};
use lib_cv::{Rotation3, Translation3};

#[test]
fn json_roundtrip() {
    let map = ArucoMap {
        markers: vec![
            MarkerPose { id: 1, translation: Translation3 { x: 1.0, y: 2.0, z: 3.0 }, rotation: Rotation3 { roll: 0.0, pitch: 0.0, yaw: 0.1 } },
            MarkerPose { id: 2, translation: Translation3 { x: 4.0, y: 5.0, z: 6.0 }, rotation: Rotation3 { roll: 0.0, pitch: 0.0, yaw: 0.2 } },
        ],
    };
    let json = map.to_json_string().unwrap();
    let parsed = ArucoMap::from_json_str(&json).unwrap();
    assert_eq!(map, parsed);
}

#[test]
fn pose_calculation() {
    let map = ArucoMap {
        markers: vec![
            MarkerPose { id: 1, translation: Translation3 { x: 0.0, y: 0.0, z: 0.0 }, rotation: Rotation3 { roll: 0.0, pitch: 0.0, yaw: 0.0 } },
            MarkerPose { id: 2, translation: Translation3 { x: 2.0, y: 0.0, z: 0.0 }, rotation: Rotation3 { roll: 0.0, pitch: 0.0, yaw: 0.5 } },
        ],
    };
    let (t, r) = map.calculate_2d_pose(&[1, 2]).unwrap();
    assert!((t.x - 1.0).abs() < 1e-6);
    assert!((t.y).abs() < 1e-6);
    assert!((r.yaw - 0.25).abs() < 1e-6);
}

#[test]
fn weighted_pose() {
    let map = ArucoMap {
        markers: vec![
            MarkerPose { id: 1, translation: Translation3 { x: 0.0, y: 0.0, z: 0.0 }, rotation: Rotation3 { roll: 0.0, pitch: 0.0, yaw: 0.0 } },
            MarkerPose { id: 2, translation: Translation3 { x: 2.0, y: 0.0, z: 0.0 }, rotation: Rotation3 { roll: 0.0, pitch: 0.0, yaw: 0.5 } },
        ],
    };
    let (t, r) = map.calculate_weighted_2d_pose(&[1, 2], &[2.0, 1.0]).unwrap();
    assert!((t.x - (0.0 * 2.0 + 2.0) / 3.0).abs() < 1e-6);
    assert!((r.yaw - (0.0 * 2.0 + 0.5) / 3.0).abs() < 1e-6);
}

#[test]
fn pose_3d() {
    let map = ArucoMap {
        markers: vec![
            MarkerPose { id: 1, translation: Translation3 { x: 0.0, y: 0.0, z: 1.0 }, rotation: Rotation3 { roll: 0.1, pitch: 0.0, yaw: 0.0 } },
            MarkerPose { id: 2, translation: Translation3 { x: 2.0, y: 0.0, z: 1.0 }, rotation: Rotation3 { roll: 0.3, pitch: 0.0, yaw: 0.2 } },
        ],
    };
    let (t, r) = map.calculate_3d_pose(&[1, 2]).unwrap();
    assert!((t.x - 1.0).abs() < 1e-6);
    assert!((t.z - 1.0).abs() < 1e-6);
    assert!((r.roll - 0.2).abs() < 1e-6);
    assert!((r.yaw - 0.1).abs() < 1e-6);
}

#[test]
fn weighted_pose_3d() {
    let map = ArucoMap {
        markers: vec![
            MarkerPose { id: 1, translation: Translation3 { x: 0.0, y: 0.0, z: 1.0 }, rotation: Rotation3 { roll: 0.0, pitch: 0.0, yaw: 0.0 } },
            MarkerPose { id: 2, translation: Translation3 { x: 2.0, y: 0.0, z: 3.0 }, rotation: Rotation3 { roll: 0.0, pitch: 0.5, yaw: 0.0 } },
        ],
    };
    let (t, r) = map.calculate_weighted_3d_pose(&[1, 2], &[1.0, 3.0]).unwrap();
    assert!((t.x - (0.0 + 6.0) / 4.0).abs() < 1e-6);
    assert!((t.z - (1.0 + 9.0) / 4.0).abs() < 1e-6);
    assert!((r.pitch - (0.0 + 1.5) / 4.0).abs() < 1e-6);
}

#[test]
fn map_ops() {
    let mut map = ArucoMap::default();
    map.add_marker(MarkerPose::new(1, Translation3 { x: 1.0, y: 0.0, z: 0.0 }, Rotation3::default()));
    assert_eq!(map.len(), 1);
    assert!(map.get_marker(1).is_some());
    map.add_marker(MarkerPose::new(1, Translation3 { x: 2.0, y: 0.0, z: 0.0 }, Rotation3::default()));
    assert_eq!(map.get_marker(1).unwrap().translation.x, 2.0);
    map.remove_marker(1);
    assert!(map.is_empty());
}

#[test]
fn json_file_roundtrip() {
    use std::fs;
    let map = ArucoMap { markers: vec![MarkerPose::new(3, Translation3 { x: 1.0, y: 2.0, z: 3.0 }, Rotation3::default())] };
    let path = std::env::temp_dir().join("map_test.json");
    map.to_json_file(&path).unwrap();
    let parsed = ArucoMap::from_json_file(&path).unwrap();
    fs::remove_file(path).unwrap();
    assert_eq!(map, parsed);
}
