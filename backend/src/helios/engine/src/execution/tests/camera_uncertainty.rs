//! The calibration's uncertainty reaches the pose nodes: a solved calibration (its own sigmas
//! and covariance, from the fit) gives a tighter multi-tag pose covariance than the same camera
//! with no uncertainty, which Eidos reads as its conservative `estimated` defaults.

use super::*;

/// The `camera.uncertainty.*` fields of a solved fit as helios-api writes them, for the pinhole
/// calibration of `calibration_context`: 0.5 px focal length and 0.3 px principal point sigmas,
/// small distortion sigmas, the covariance diagonal of those (Eidos's order: fx, fy, cx, cy, then
/// k1, k2, p1, p2, k3).
fn solved_context() -> Vec<(&'static str, ContextValue)> {
    let sigmas = [0.5, 0.5, 0.3, 0.3, 0.002, 0.002, 0.001, 0.001, 0.002];
    let mut covariance = vec![0.0; sigmas.len() * sigmas.len()];
    for (i, sigma) in sigmas.iter().enumerate() {
        covariance[i * sigmas.len() + i] = sigma * sigma;
    }
    vec![
        ("camera.uncertainty.kind", ContextValue::Name("solved".into())),
        ("camera.uncertainty.rms_px", ContextValue::Number(0.25)),
        ("camera.uncertainty.views", ContextValue::Number(40.0)),
        ("camera.uncertainty.sigma.fx", ContextValue::Number(0.5)),
        ("camera.uncertainty.sigma.fy", ContextValue::Number(0.5)),
        ("camera.uncertainty.sigma.cx", ContextValue::Number(0.3)),
        ("camera.uncertainty.sigma.cy", ContextValue::Number(0.3)),
        ("camera.uncertainty.sigma.distortion", ContextValue::Name("[0.002,0.002,0.001,0.001,0.002]".into())),
        ("camera.uncertainty.covariance", ContextValue::Name(serde_json::to_string(&covariance).expect("json"))),
    ]
}

/// The multi-tag pose's 6 x 6 covariance (row-major, `[rx, ry, rz, tx, ty, tz]`) for one frame of
/// the known tag, with `extra` context added to the calibration.
fn multi_tag_covariance(extra: Vec<(&'static str, ContextValue)>) -> Vec<f64> {
    let plugins = TestPlugins::load();
    let mut document: serde_json::Value = serde_json::from_str(APRILTAG_GRAPH_DOCUMENT).expect("document");
    let nodes = document["graph"]["nodes"].as_array_mut().expect("nodes");
    let node = nodes.iter_mut().find(|node| node["id"] == "eidos:aruco.multi_tag_pose").expect("multi-tag pose node");
    let consts = node["const_inputs"].as_array_mut().expect("const inputs");
    let entry = consts.iter_mut().find(|entry| entry[0] == "known_tags").expect("known_tags constant");
    let value = daedalus::data::json::from_plain_json(&serde_json::from_str(&test_known_tags()).expect("json")).expect("value");
    entry[1] = serde_json::to_value(value).expect("value json");
    let mut context = calibration_context(600.0, "pinhole");
    context.extend(extra);
    let mut pose = workload("workload.uncertainty", document.to_string(), vec![camera_binding(&context)]);
    pose.plugin_requirements = vec![PluginRequirement { plugin_name: "eidos".into(), version: None }];
    let graph = graph::compile_workload_graph(plugins.registry(), &plugins.metadata, &pose, &["frame".to_string()], GraphSettings::default()).expect("compile");
    let (sender, receiver) = test_camera();
    let mut driver =
        WorkloadDriver::start(pose.clone(), graph, vec![(frame_spec("frame"), Box::new(receiver) as Box<dyn FrameSource>)], Arc::new(Notify::new()), crate::nt4::Nt4Sink::default(), PipelineBus::default()).expect("driver");
    driver.update_camera_context(&pose);
    sender.send(marker_scene(ArucoDictionaryKind::AprilTag36h11, 1, 1)).expect("send frame");
    wait_for("the frame", || driver.stats().ticks_processed == 1);
    assert!(driver.stats().last_error.is_none(), "{:?}", driver.stats().last_error);
    let (_, artifacts) = driver.snapshot();
    let multi = artifact_json(&artifacts, "host_output:multi_tag_pose");
    assert_eq!(multi["status"], "calibrated", "{multi}");
    assert_eq!(multi["valid"], true, "{multi}");
    multi["covariance"].as_array().expect("covariance").iter().map(|value| value.as_f64().expect("number")).collect()
}

#[test]
fn solved_uncertainty_tightens_the_multi_tag_pose_covariance() {
    let estimated = multi_tag_covariance(Vec::new());
    let solved = multi_tag_covariance(solved_context());
    assert_eq!((estimated.len(), solved.len()), (36, 36));
    for index in 0..6 {
        let (loose, tight) = (estimated[index * 6 + index], solved[index * 6 + index]);
        assert!(loose.is_finite() && tight.is_finite() && tight > 0.0, "axis {index}: {loose} {tight}");
        assert!(tight < loose, "axis {index}: solved {tight} is not below estimated {loose}");
    }
    let trace = |covariance: &[f64]| (0..6).map(|index| covariance[index * 6 + index]).sum::<f64>();
    assert!(trace(&solved) < trace(&estimated) / 2.0, "solved {} vs estimated {}", trace(&solved), trace(&estimated));
}
