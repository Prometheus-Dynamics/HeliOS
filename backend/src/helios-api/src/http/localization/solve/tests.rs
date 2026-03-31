use super::request::{strip_overlay_from_field_map, trim_localization_solve_response_to_field_poses};
use helios_engine::localization::config::LocalizationSolverMode;
use helios_engine::localization::maps::{FieldMapDocument, FieldMapOverlay, FieldMapSource};
use helios_engine::localization::types::{
    LocalizationDetectionPose, LocalizationPose, LocalizationQuaternion, LocalizationRotation, LocalizationSolveResponse, LocalizationSolveTimings, LocalizationSolverOutputs, LocalizationSolverPose,
    LocalizationSolverResult, LocalizationSourcePose, LocalizationVector,
};

fn sample_pose() -> LocalizationPose {
    LocalizationPose {
        translation: LocalizationVector { x: 1.0, y: 2.0, z: 3.0 },
        rotation: LocalizationRotation { roll: 4.0, pitch: 5.0, yaw: 6.0, quaternion: LocalizationQuaternion { x: 0.0, y: 0.0, z: 0.0, w: 1.0 } },
    }
}

#[test]
fn strip_overlay_from_field_map_removes_embedded_image_data() {
    let field_map = FieldMapDocument {
        schema_version: 1,
        id: "test".to_string(),
        name: "Test".to_string(),
        width_m: 1.0,
        depth_m: 1.0,
        markers: Vec::new(),
        source: FieldMapSource::LimelightFmap { original_file_name: None, map_type: None },
        overlay: Some(FieldMapOverlay {
            data_url: "data:image/png;base64,AAAA".to_string(),
            mime_type: Some("image/png".to_string()),
            opacity: Some(1.0),
            width_m: Some(1.0),
            depth_m: Some(1.0),
            offset_x_m: Some(0.0),
            offset_z_m: Some(0.0),
            rotation_deg: Some(0.0),
        }),
    };

    let stripped = strip_overlay_from_field_map(Some(&field_map)).expect("stripped field map");

    assert!(stripped.overlay.is_none());
    assert_eq!(stripped.markers.len(), field_map.markers.len());
    assert_eq!(stripped.id, field_map.id);
    assert_eq!(stripped.name, field_map.name);
}

#[test]
fn field_poses_only_trim_drops_detection_outputs_but_keeps_field_outputs() {
    let detection = LocalizationDetectionPose {
        source_id: "src".to_string(),
        camera_uid: "cam".to_string(),
        tag_id: 3,
        pose: sample_pose(),
        weight: 1.0,
        quality: 1.0,
        tag_size: Some(0.165),
        code_rotation: Some(0),
        tag_bits: Some(lib_cv::modules::aruco::ArucoBitGrid { width: 6, border: 1, rows: vec!["010101".to_string(); 6] }),
    };
    let mut response = LocalizationSolveResponse {
        profile_id: "profile".to_string(),
        solvers: vec![LocalizationSolverResult {
            id: "solver".to_string(),
            name: "Solver".to_string(),
            mode: LocalizationSolverMode::GroupSolve,
            output_spaces: Vec::new(),
            outputs: LocalizationSolverOutputs {
                tag_in_camera: Some(vec![detection.clone()]),
                camera_in_tag: Some(vec![detection.clone()]),
                tag_in_robot: Some(vec![detection.clone()]),
                robot_in_tag: Some(vec![detection]),
                camera_in_field: Some(vec![LocalizationSourcePose { source_id: "src".to_string(), camera_uid: "cam".to_string(), weight: 1.0, pose: sample_pose() }]),
                robot_in_field: Some(LocalizationSolverPose { pose: sample_pose(), source_ids: vec!["src".to_string()] }),
            },
            errors: Vec::new(),
        }],
        sources: Vec::new(),
        timings: LocalizationSolveTimings::default(),
    };

    trim_localization_solve_response_to_field_poses(&mut response);

    let outputs = &response.solvers[0].outputs;
    assert!(outputs.tag_in_camera.is_none());
    assert!(outputs.camera_in_tag.is_none());
    assert!(outputs.tag_in_robot.is_none());
    assert!(outputs.robot_in_tag.is_none());
    assert_eq!(outputs.camera_in_field.as_ref().map(Vec::len), Some(1));
    assert!(outputs.robot_in_field.is_some());
}
