use super::*;

#[test]
fn auto_target_roi_tracks_and_holds_selected_detection() {
    let mut state = super::super::AutoTargetRoiState::default();
    let detection = ArucoDetection2D {
        id: 3,
        rotation: 0,
        corners: [lib_cv::Point { x: 600.0, y: 360.0 }, lib_cv::Point { x: 680.0, y: 360.0 }, lib_cv::Point { x: 680.0, y: 440.0 }, lib_cv::Point { x: 600.0, y: 440.0 }],
        score: None,
        best_distance: None,
        second_distance: None,
        border_mismatches: None,
        contrast_range: None,
        border_width: None,
        data_width: None,
        bits: None,
    };

    let rect = update_auto_target_roi_state(&mut state, (1280, 800), &[detection.clone()]).expect("roi rect");
    assert!(rect.w > 80 && rect.w < 1280);
    assert!(rect.h > 80 && rect.h < 800);
    assert!(rect.x <= 600 && rect.y <= 360);
    assert!(state.ever_detected);

    let held = update_auto_target_roi_state(&mut state, (1280, 800), &[]).expect("held roi");
    assert!(held.w >= rect.w);
    assert!(held.h >= rect.h);
    assert_eq!(state.misses, 1);

    for _ in 0..4 {
        let _ = update_auto_target_roi_state(&mut state, (1280, 800), &[]);
    }
    assert!(state.rect.is_none());
    assert!(state.ever_detected);
}

#[test]
fn bootstrap_auto_target_roi_defaults_to_center_before_first_lock() {
    let inputs = BTreeMap::new();
    let rect = super::super::bootstrap_auto_target_roi_rect((1280, 800), &inputs).expect("bootstrap rect");
    assert!((rect.x + rect.w / 2 - 640).abs() <= 2);
    assert!((rect.y + rect.h / 2 - 400).abs() <= 2);
    assert!(rect.w < 1280);
    assert!(rect.h < 800);
}

#[test]
fn bootstrap_auto_target_roi_prefers_explicit_crosshair() {
    let mut inputs = BTreeMap::new();
    inputs.insert("crosshair_x".to_string(), Value::Int(960));
    inputs.insert("crosshair_y".to_string(), Value::Int(220));
    inputs.insert("draw_crosshair".to_string(), Value::Bool(true));
    let rect = super::super::bootstrap_auto_target_roi_rect((1280, 800), &inputs).expect("bootstrap rect");
    assert!((rect.x + rect.w / 2 - 960).abs() <= 2);
    assert!((rect.y + rect.h / 2 - 220).abs() <= 2);
}

#[test]
fn bootstrap_auto_target_roi_crosshair_is_tighter_than_default() {
    let default_rect = super::super::bootstrap_auto_target_roi_rect((1280, 800), &BTreeMap::new()).expect("default rect");

    let mut inputs = BTreeMap::new();
    inputs.insert("crosshair_x".to_string(), Value::Int(640));
    inputs.insert("crosshair_y".to_string(), Value::Int(400));
    inputs.insert("draw_crosshair".to_string(), Value::Bool(true));

    let crosshair_rect = super::super::bootstrap_auto_target_roi_rect((1280, 800), &inputs).expect("crosshair rect");
    assert!(crosshair_rect.w < default_rect.w);
    assert!(crosshair_rect.h < default_rect.h);
}

#[test]
fn metadata_bool_flag_accepts_bool_and_string_values() {
    let mut map = BTreeMap::new();
    map.insert("helios.auto_target_roi".to_string(), Value::Bool(false));
    assert_eq!(super::super::metadata_bool_flag(&map, "helios.auto_target_roi"), Some(false));

    map.insert("helios.auto_target_roi".to_string(), Value::String("true".into()));
    assert_eq!(super::super::metadata_bool_flag(&map, "helios.auto_target_roi"), Some(true));

    map.insert("helios.auto_target_roi".to_string(), Value::String("off".into()));
    assert_eq!(super::super::metadata_bool_flag(&map, "helios.auto_target_roi"), Some(false));
}

#[test]
fn host_bridge_injects_any_payload_into_graph() {
    let host_bridge = RuntimeNode {
        id: "io.host_bridge".into(),
        stable_id: 0,
        bundle: None,
        label: Some("Input:frame".into()),
        compute: ComputeAffinity::CpuOnly,
        const_inputs: vec![],
        sync_groups: vec![],
        metadata: std::collections::BTreeMap::from([("host_bridge".into(), Value::Bool(true))]),
    };

    let sink_node = RuntimeNode {
        id: "test.sink".into(),
        stable_id: 0,
        bundle: None,
        label: Some("Sink".into()),
        compute: ComputeAffinity::CpuOnly,
        const_inputs: vec![],
        sync_groups: vec![],
        metadata: std::collections::BTreeMap::new(),
    };

    let plan = RuntimePlan {
        default_policy: EdgePolicyKind::Fifo,
        backpressure: daedalus::runtime::BackpressureStrategy::None,
        lockfree_queues: false,
        graph_metadata: std::collections::BTreeMap::new(),
        nodes: vec![host_bridge, sink_node],
        edges: vec![(daedalus::planner::NodeRef(0), "frame".into(), daedalus::planner::NodeRef(1), "frame".into(), EdgePolicyKind::Fifo)],
        gpu_segments: vec![],
        gpu_edges: vec![],
        gpu_entries: vec![],
        gpu_exits: vec![],
        segments: vec![RuntimeSegment { nodes: vec![daedalus::planner::NodeRef(0), daedalus::planner::NodeRef(1)], compute: ComputeAffinity::CpuOnly }],
        schedule_order: vec![],
    };

    let plan = std::sync::Arc::new(plan);
    let host_mgr = daedalus::runtime::host_bridge::HostBridgeManager::new();
    host_mgr.populate_from_plan(&plan);
    let host = host_mgr.handle("Input:frame").expect("host handle");
    let payload = RuntimeValue::Data(DataCell::from_cpu::<DynamicImage>(DynamicImage::new_rgb8(1, 1)));
    host.push("frame", payload, None);

    let saw_frame = std::sync::Arc::new(std::sync::Mutex::new(false));
    let saw_frame_for_handler = saw_frame.clone();

    let mut handlers = daedalus::runtime::handler_registry::HandlerRegistry::new();
    handlers.on("test.sink", move |_node, _ctx, io| {
        if io.get_any::<DynamicImage>("frame").is_some() {
            if let Ok(mut guard) = saw_frame_for_handler.lock() {
                *guard = true;
            }
        }
        Ok(())
    });

    let mut exec = daedalus::runtime::executor::OwnedExecutor::new(plan, handlers).with_host_bridges(host_mgr);
    let _ = exec.run_in_place().expect("executor run");
    assert!(*saw_frame.lock().unwrap());
}

#[test]
fn pipeline_edge_metrics_fall_back_to_planned_bounded_capacity() {
    let mut rolling = super::super::RollingGraphMetrics::new(
        8,
        vec![],
        vec![super::super::EdgeInfo {
            from_node_index: 0,
            from_node_label: Some("Input".into()),
            from_port: "frame".into(),
            to_node_index: 1,
            to_node_label: Some("Output".into()),
            to_port: "preview".into(),
            queue_capacity: Some(4),
            policy: "Bounded { cap: 4 }".into(),
        }],
    );

    rolling.edge_samples.insert(0, VecDeque::from([super::super::EdgeMetricSample { samples: 1, max_depth: 2, current_depth: 1, ..Default::default() }]));

    let metrics = rolling.snapshot();
    let edge = metrics.edges.expect("edge metrics").get("edge_0").cloned().expect("edge_0");
    assert_eq!(edge.capacity, Some(4));
    assert_eq!(edge.policy.as_deref(), Some("Bounded { cap: 4 }"));
}

#[test]
fn host_bridge_input_port_infers_frame_from_solved_types() {
    let frame_ty = daedalus_types::image_dynamic();
    let roi_ty = TypeExpr::opaque("int");
    let frame_ty_json = serde_json::to_string(&frame_ty).expect("frame type json");
    let roi_ty_json = serde_json::to_string(&roi_ty).expect("roi type json");

    let host_bridge = RuntimeNode {
        id: "io.host_bridge".into(),
        stable_id: 0,
        bundle: None,
        label: Some("Input".into()),
        compute: ComputeAffinity::CpuOnly,
        const_inputs: vec![],
        sync_groups: vec![],
        metadata: std::collections::BTreeMap::from([
            ("host_bridge".into(), Value::Bool(true)),
            (
                "dynamic_output_types".into(),
                Value::Map(vec![
                    (Value::String(std::borrow::Cow::Owned("frame".into())), Value::String(std::borrow::Cow::Owned(frame_ty_json))),
                    (Value::String(std::borrow::Cow::Owned("roi_x".into())), Value::String(std::borrow::Cow::Owned(roi_ty_json))),
                ]),
            ),
        ]),
    };

    let host_output = RuntimeNode {
        id: "io.host_output".into(),
        stable_id: 0,
        bundle: None,
        label: Some("Output".into()),
        compute: ComputeAffinity::CpuOnly,
        const_inputs: vec![],
        sync_groups: vec![],
        metadata: std::collections::BTreeMap::from([("host_bridge".into(), Value::Bool(true))]),
    };

    let plan = RuntimePlan {
        default_policy: EdgePolicyKind::Fifo,
        backpressure: daedalus::runtime::BackpressureStrategy::None,
        lockfree_queues: false,
        graph_metadata: std::collections::BTreeMap::new(),
        nodes: vec![host_bridge, host_output],
        edges: vec![
            (daedalus::planner::NodeRef(0), "frame".into(), daedalus::planner::NodeRef(1), "raw".into(), EdgePolicyKind::Fifo),
            (daedalus::planner::NodeRef(0), "roi_x".into(), daedalus::planner::NodeRef(1), "roi_x".into(), EdgePolicyKind::Fifo),
        ],
        gpu_segments: vec![],
        gpu_edges: vec![],
        gpu_entries: vec![],
        gpu_exits: vec![],
        segments: vec![RuntimeSegment { nodes: vec![daedalus::planner::NodeRef(0), daedalus::planner::NodeRef(1)], compute: ComputeAffinity::CpuOnly }],
        schedule_order: vec![],
    };

    let host_mgr = daedalus::runtime::host_bridge::HostBridgeManager::new();
    let plan = std::sync::Arc::new(plan);
    host_mgr.populate_from_plan(&plan);

    let (_input_alias, input_port, _output_aliases) = derive_host_aliases(plan.as_ref(), &host_mgr).expect("host alias resolve");
    assert_eq!(input_port, "frame");
}
