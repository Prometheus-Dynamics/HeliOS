use super::*;

#[test]
fn demand_driven_sinks_skip_non_preview_image_outputs() {
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
        nodes: vec![host_output],
        edges: vec![],
        gpu_segments: vec![],
        gpu_edges: vec![],
        gpu_entries: vec![],
        gpu_exits: vec![],
        segments: vec![RuntimeSegment { nodes: vec![daedalus::planner::NodeRef(0)], compute: ComputeAffinity::CpuOnly }],
        schedule_order: vec![],
    };

    let mut port_types = BTreeMap::new();
    port_types.insert("overlay".to_string(), daedalus_types::image_dynamic());
    port_types.insert("detections".to_string(), TypeExpr::list(TypeExpr::scalar(daedalus::data::model::ValueType::Int)));

    let host_output_port_owners = BTreeMap::new();
    let sinks = build_demand_sinks(
        &plan,
        &["Output".to_string()],
        &["raw".to_string()],
        &["raw".to_string(), "overlay".to_string(), "detections".to_string()],
        &port_types,
        &host_output_port_owners,
        true,
        &[],
    );

    let ports = sinks.into_iter().filter_map(|sink| sink.port).collect::<BTreeSet<_>>();
    assert!(ports.contains("raw"));
    assert!(ports.contains("detections"));
    assert!(!ports.contains("overlay"));
}

#[test]
fn preview_only_demand_targets_host_output_sink_and_only_overlay_branch() {
    let clahe = RuntimeNode {
        id: "cv:aruco:clahe_gray".into(),
        stable_id: 0,
        bundle: None,
        label: Some("CLAHE".into()),
        compute: ComputeAffinity::CpuOnly,
        const_inputs: vec![],
        sync_groups: vec![],
        metadata: std::collections::BTreeMap::new(),
    };
    let adaptive = RuntimeNode {
        id: "cv:aruco:adaptive_threshold_gray".into(),
        stable_id: 0,
        bundle: None,
        label: Some("Adaptive".into()),
        compute: ComputeAffinity::CpuOnly,
        const_inputs: vec![],
        sync_groups: vec![],
        metadata: std::collections::BTreeMap::new(),
    };
    let crosshair = RuntimeNode {
        id: "cv:draw:drawcrosshairat".into(),
        stable_id: 0,
        bundle: None,
        label: Some("Crosshair".into()),
        compute: ComputeAffinity::CpuOnly,
        const_inputs: vec![],
        sync_groups: vec![],
        metadata: std::collections::BTreeMap::new(),
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
        nodes: vec![clahe, adaptive, crosshair, host_output],
        edges: vec![
            (daedalus::planner::NodeRef(0), "mask".into(), daedalus::planner::NodeRef(3), "clahe".into(), EdgePolicyKind::Fifo),
            (daedalus::planner::NodeRef(1), "mask".into(), daedalus::planner::NodeRef(3), "adaptive".into(), EdgePolicyKind::Fifo),
            (daedalus::planner::NodeRef(2), "frame".into(), daedalus::planner::NodeRef(3), "overlay".into(), EdgePolicyKind::Fifo),
        ],
        gpu_segments: vec![],
        gpu_edges: vec![],
        gpu_entries: vec![],
        gpu_exits: vec![],
        segments: vec![RuntimeSegment {
            nodes: vec![daedalus::planner::NodeRef(0), daedalus::planner::NodeRef(1), daedalus::planner::NodeRef(2), daedalus::planner::NodeRef(3)],
            compute: ComputeAffinity::CpuOnly,
        }],
        schedule_order: vec![],
    };

    let mut port_types = BTreeMap::new();
    port_types.insert("overlay".to_string(), daedalus_types::image_dynamic());
    port_types.insert("clahe".to_string(), daedalus_types::image_gray8());
    port_types.insert("adaptive".to_string(), daedalus_types::image_gray8());

    let owners = super::super::infer_host_output_port_owners(&plan, &["Output".to_string()]);
    assert_eq!(owners.get("overlay"), Some(&3));
    assert_eq!(owners.get("clahe"), Some(&3));
    assert_eq!(owners.get("adaptive"), Some(&3));

    let sinks = build_demand_sinks(&plan, &["Output".to_string()], &["overlay".to_string()], &["overlay".to_string(), "clahe".to_string(), "adaptive".to_string()], &port_types, &owners, true, &[]);
    assert_eq!(sinks.len(), 1);
    assert_eq!(sinks[0].port.as_deref(), Some("overlay"));
    assert_eq!(sinks[0].node.index, Some(3));

    let mask = super::super::build_demand_mask(
        &plan,
        &["Output".to_string()],
        &["overlay".to_string()],
        &["overlay".to_string(), "clahe".to_string(), "adaptive".to_string()],
        &port_types,
        &owners,
        true,
        true,
        false,
        &[],
    )
    .expect("preview-only demand mask");
    assert_eq!(mask, vec![false, false, true, true]);
}

#[test]
fn normalize_graph_runtime_prunes_disconnected_host_output_ports() {
    let graph_json = json!({
        "nodes": [
            {
                "id": "io.host_bridge",
                "label": "Input",
                "inputs": [],
                "outputs": ["frame"],
                "metadata": {
                    "host_bridge": { "type": "Bool", "value": true }
                }
            },
            {
                "id": "io.host_output",
                "label": "Output",
                "inputs": ["detections", "tv", "overlay"],
                "outputs": [],
                "metadata": {
                    "host_bridge": { "type": "Bool", "value": true },
                    "host_bridge_inputs": {
                        "type": "String",
                        "value": "detections:list<cv:aruco_detection_2d>,tv:float,overlay:image:dynamic"
                    },
                    "host_bridge_inputs_display": {
                        "type": "String",
                        "value": "{\"detections\":\"Detections\",\"tv\":\"TV\",\"overlay\":\"Overlay\"}"
                    }
                }
            }
        ],
        "edges": [
            {
                "from": { "node": 0, "port": "frame" },
                "to": { "node": 1, "port": "detections" }
            }
        ]
    });

    let normalized = normalize_graph_json_for_runtime(&graph_json);
    let nodes = normalized.get("nodes").and_then(|value| value.as_array()).expect("nodes");
    let output = nodes[1].as_object().expect("host output node");
    let inputs = output.get("inputs").and_then(|value| value.as_array()).expect("inputs");
    assert_eq!(inputs, &vec![json!("detections")]);

    let metadata = output.get("metadata").and_then(|value| value.as_object()).expect("metadata");
    assert_eq!(metadata.get("host_bridge_inputs").and_then(|value| value.get("value")).and_then(|value| value.as_str()), Some("detections:list<cv:aruco_detection_2d>"));
    assert_eq!(metadata.get("host_bridge_inputs_display").and_then(|value| value.get("value")).and_then(|value| value.as_str()), Some("{\"detections\":\"Detections\"}"));
}

#[test]
fn normalize_graph_runtime_prunes_isolated_nodes() {
    let graph_json = json!({
        "nodes": [
            {
                "id": "io.host_bridge",
                "label": "Input",
                "inputs": [],
                "outputs": ["frame"],
                "metadata": {
                    "host_bridge": { "type": "Bool", "value": true }
                }
            },
            {
                "id": "cv:image:crop_roi_gray",
                "label": "ROI",
                "inputs": ["frame"],
                "outputs": ["frame"]
            },
            {
                "id": "io.host_output",
                "label": "Output",
                "inputs": ["frame"],
                "outputs": [],
                "metadata": {
                    "host_bridge": { "type": "Bool", "value": true },
                    "host_bridge_inputs": { "type": "String", "value": "frame:image:dynamic" }
                }
            },
            {
                "id": "cv:image:clahe",
                "label": "Dead CLAHE",
                "inputs": ["mask"],
                "outputs": ["mask"]
            },
            {
                "id": "io.host_output",
                "label": "Dead Output",
                "inputs": ["clahe"],
                "outputs": [],
                "metadata": {
                    "host_bridge": { "type": "Bool", "value": true },
                    "host_bridge_inputs": { "type": "String", "value": "clahe:image:dynamic" }
                }
            }
        ],
        "edges": [
            {
                "from": { "node": 0, "port": "frame" },
                "to": { "node": 1, "port": "frame" }
            },
            {
                "from": { "node": 1, "port": "frame" },
                "to": { "node": 2, "port": "frame" }
            }
        ]
    });

    let normalized = normalize_graph_json_for_runtime(&graph_json);
    let nodes = normalized.get("nodes").and_then(|value| value.as_array()).expect("nodes");
    let ids = nodes.iter().map(|node| node.get("id").and_then(|value| value.as_str()).unwrap_or_default().to_string()).collect::<Vec<_>>();
    assert_eq!(ids, vec!["io.host_bridge", "cv:image:crop_roi_gray", "io.host_output"]);

    let edges = normalized.get("edges").and_then(|value| value.as_array()).expect("edges");
    let edge_indices = edges
        .iter()
        .map(|edge| {
            let from = edge.get("from").and_then(|value| value.get("node")).and_then(|value| value.as_u64()).expect("from node");
            let to = edge.get("to").and_then(|value| value.get("node")).and_then(|value| value.as_u64()).expect("to node");
            (from, to)
        })
        .collect::<Vec<_>>();
    assert_eq!(edge_indices, vec![(0, 1), (1, 2)]);
}

#[test]
fn decode_runtime_value_fallback_preserves_lazy_json_readability() {
    let payload = super::super::DaedalusEdgePayload::Any(std::sync::Arc::new("{\"tag\":3,\"ok\":true}".to_string()));
    let value = super::super::decode_runtime_value_fallback(&payload, None).expect("decoded value");
    assert_eq!(super::super::daedalus_value_to_json(&value), Some(json!({ "tag": 3, "ok": true })));
}

#[test]
fn structured_output_retention_is_request_gated_for_preview_graphs() {
    let requested = BTreeSet::from(["detections".to_string()]);
    assert!(super::super::should_retain_structured_output_port("detections", &requested, None, true));
    assert!(super::super::should_retain_structured_output_port("target_detections", &BTreeSet::new(), Some("target_detections"), true));
    assert!(!super::super::should_retain_structured_output_port("tid", &BTreeSet::new(), Some("target_detections"), true));
}

#[test]
fn structured_output_retention_stays_enabled_for_structured_only_graphs() {
    assert!(super::super::should_retain_structured_output_port("solver_pose", &BTreeSet::new(), None, false));
    assert!(super::super::should_retain_structured_output_port("detections", &BTreeSet::new(), None, false));
}

#[test]
fn node_metrics_window_scales_with_metrics_level() {
    assert_eq!(super::super::node_metrics_window(daedalus::runtime::MetricsLevel::Off), 16);
    assert_eq!(super::super::node_metrics_window(daedalus::runtime::MetricsLevel::Basic), 32);
    assert_eq!(super::super::node_metrics_window(daedalus::runtime::MetricsLevel::Detailed), 64);
    assert_eq!(super::super::node_metrics_window(daedalus::runtime::MetricsLevel::Profile), 100);
}

#[test]
fn rolling_graph_metrics_release_idle_retention_clears_samples_but_keeps_warnings() {
    let mut metrics = super::super::RollingGraphMetrics::new(8, Vec::new(), Vec::new());
    metrics.record_graph_duration(std::time::Duration::from_millis(5));
    metrics.record_wrapper_duration(std::time::Duration::from_millis(2));
    metrics.record_warning("keep me".to_string());

    metrics.release_idle_retention();

    assert!(metrics.graph_samples.is_empty());
    assert!(metrics.wrapper_samples.is_empty());
    assert!(metrics.samples.is_empty());
    assert!(metrics.edge_samples.is_empty());
    assert_eq!(metrics.warnings.len(), 1);
    assert_eq!(metrics.warnings.back().map(|(_, msg)| msg.as_str()), Some("keep me"));
}
