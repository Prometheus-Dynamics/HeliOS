use super::{build_demand_sinks, derive_host_aliases, infer_host_output_incoming_types, normalize_graph_json_for_runtime};

use daedalus::data::model::{EnumVariant, TypeExpr, Value};
use daedalus::gpu::ErasedPayload;
use daedalus::planner::ComputeAffinity;
use daedalus::registry::store::NodeDescriptorBuilder;
use daedalus::runtime::executor::EdgePayload;
use daedalus::runtime::plugins::PluginRegistry;
use daedalus::runtime::{EdgePolicyKind, RuntimeNode, RuntimePlan, RuntimeSegment};
use image::DynamicImage;
use image::GenericImageView;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Instant;

#[test]
fn normalize_graph_enum_consts_unwraps_struct_wrapped_values() {
    let graph_json = serde_json::json!({
        "nodes": [
            {
                "id": "test:mode_node",
                "inputs": ["mode"],
                "outputs": [],
                "const_inputs": [
                    ["mode", { "type": "String", "value": "gpu" }]
                ]
            }
        ],
        "edges": []
    });

    let mut graph: daedalus::planner::Graph = serde_json::from_value(graph_json).expect("graph parse");

    let mode_ty = TypeExpr::optional(TypeExpr::r#enum(vec![
        EnumVariant { name: "auto".to_string(), ty: None },
        EnumVariant { name: "cpu".to_string(), ty: None },
        EnumVariant { name: "gpu".to_string(), ty: None },
    ]));
    let desc = NodeDescriptorBuilder::new("test:mode_node").input("mode", mode_ty).build().expect("descriptor build");

    let mut registry = PluginRegistry::new();
    registry.registry.register_node(desc).expect("descriptor register");

    super::normalize_graph_enum_const_inputs(&mut graph, &registry);

    let value = graph.nodes[0].const_inputs.iter().find(|(name, _)| name == "mode").map(|(_, value)| value.clone()).expect("mode const input");
    assert_eq!(value, Value::Int(2));
}

#[test]
fn sync_graph_node_port_declarations_repairs_stale_ports() {
    let graph_json = serde_json::json!({
        "nodes": [
            {
                "id": "cv:test:target",
                "inputs": ["detections", "ins7"],
                "outputs": ["selected"],
                "const_inputs": [
                    ["fallback_to_nearest", { "type": "Bool", "value": true }],
                    ["legacy_port", { "type": "Int", "value": 1 }]
                ]
            }
        ],
        "edges": []
    });
    let mut graph: daedalus::planner::Graph = serde_json::from_value(graph_json).expect("graph parse");

    let desc = NodeDescriptorBuilder::new("cv:test:target")
        .input("detections", TypeExpr::opaque("cv:aruco_detection_2d_list"))
        .input("frame_width", TypeExpr::scalar(daedalus::data::model::ValueType::Int))
        .input("frame_height", TypeExpr::scalar(daedalus::data::model::ValueType::Int))
        .input("crosshair_x", TypeExpr::scalar(daedalus::data::model::ValueType::Int))
        .input("crosshair_y", TypeExpr::scalar(daedalus::data::model::ValueType::Int))
        .input("mode", TypeExpr::r#enum(vec![EnumVariant { name: "nearest".to_string(), ty: None }]))
        .input("fallback_to_nearest", TypeExpr::scalar(daedalus::data::model::ValueType::Bool))
        .fanin_input("ins", 0, TypeExpr::scalar(daedalus::data::model::ValueType::Int))
        .output("target", TypeExpr::opaque("cv:aruco_detection_2d"))
        .build()
        .expect("descriptor build");

    let mut registry = PluginRegistry::new();
    registry.registry.register_node(desc).expect("descriptor register");

    super::sync_graph_node_port_declarations(&mut graph, &registry);

    let node = &graph.nodes[0];
    let inputs = node.inputs.iter().map(|name| name.to_ascii_lowercase()).collect::<BTreeSet<_>>();
    assert!(inputs.contains("detections"));
    assert!(inputs.contains("frame_width"));
    assert!(inputs.contains("frame_height"));
    assert!(inputs.contains("crosshair_x"));
    assert!(inputs.contains("crosshair_y"));
    assert!(inputs.contains("mode"));
    assert!(inputs.contains("fallback_to_nearest"));
    assert!(inputs.contains("ins7"), "fanin input should be preserved");

    assert_eq!(node.outputs, vec!["target".to_string()]);
    let const_ports = node.const_inputs.iter().map(|(name, _)| name.to_ascii_lowercase()).collect::<BTreeSet<_>>();
    assert!(const_ports.contains("fallback_to_nearest"));
    assert!(!const_ports.contains("legacy_port"));
}

#[test]
#[ignore = "requires the dynamic Daedalus CV plugin (cv:image:undistort_optional) to be installed/loaded"]
fn raw_stream_graph_can_select_undistorted_output() {
    // This mirrors the built-in RAW stream graph (raw passthrough + optional undistort).
    let graph_json = serde_json::json!({
            "nodes": [
                {
                    "id": "io.host_bridge",
                    "label": "Input:frame+calibration",
                    "inputs": [],
                    "outputs": ["frame", "calibration"],
                    "metadata": { "host_bridge": { "type": "Bool", "value": true } }
                },
                {
                    "id": "cv:image:undistort_optional",
                    "label": "Undistort (calibration)",
                    "inputs": ["frame", "calibration", "border_mode", "zoom_mode", "zoom", "fill_margin"],
                    "outputs": ["frame"],
                    "const_inputs": [
                        ["border_mode", { "type": "String", "value": "clamp" }],
                        ["zoom_mode", { "type": "String", "value": "fill" }],
                        ["fill_margin", { "type": "Float", "value": 1.0 }]
                    ]
                },
                {
                    "id": "io.host_output",
                    "label": "Output:raw+undistorted",
                    "inputs": ["frame", "raw", "undistorted"],
                "outputs": [],
                "metadata": { "host_bridge": { "type": "Bool", "value": true } }
            }
        ],
        "edges": [
            { "from": { "node": 0, "port": "frame" }, "to": { "node": 2, "port": "frame" } },
            { "from": { "node": 0, "port": "frame" }, "to": { "node": 2, "port": "raw" } },
            { "from": { "node": 0, "port": "frame" }, "to": { "node": 1, "port": "frame" } },
            { "from": { "node": 0, "port": "calibration" }, "to": { "node": 1, "port": "calibration" } },
            { "from": { "node": 1, "port": "frame" }, "to": { "node": 2, "port": "undistorted" } }
        ]
    });

    // Create a simple synthetic image (high-frequency content makes undistortion differences obvious).
    let (w, h) = (160u32, 120u32);
    let mut rgba = image::RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = (((x / 4) ^ (y / 4)) & 1) as u8 * 255;
            rgba.put_pixel(x, y, image::Rgba([v, 255 - v, (x.wrapping_mul(3) as u8), 255]));
        }
    }
    let input = DynamicImage::ImageRgba8(rgba);

    let calib = crate::ipc::StreamCalibration {
        fx: 120.0,
        fy: 120.0,
        cx: (w as f64) / 2.0,
        cy: (h as f64) / 2.0,
        // Strong distortion so output must differ from raw.
        k1: -0.35,
        k2: 0.10,
        p1: 0.0,
        p2: 0.0,
        k3: 0.0,
        undistort_iters: 5,
        lens_model: lib_cv::modules::calibration::LensModel::Pinhole,
    };

    let raw = super::GraphHandle::from_json_with_output(1, &graph_json, Some("frame")).expect("raw graph");
    raw.set_calibration(Some(calib.clone()));
    let out_raw = raw.process(input.clone()).expect("raw output");

    let undist = super::GraphHandle::from_json_with_output(1, &graph_json, Some("undistorted")).expect("undist graph");
    undist.set_calibration(Some(calib));
    let out_undist = undist.process(input).expect("undistorted output");

    assert_eq!(out_raw.dimensions(), out_undist.dimensions());

    // Compare pixel buffers. We expect non-trivial differences when undistortion is active.
    let a = out_raw.to_rgba8().into_raw();
    let b = out_undist.to_rgba8().into_raw();
    let diff = a.iter().zip(b.iter()).filter(|(x, y)| x != y).count();
    assert!(diff > (a.len() / 200), "expected undistortion to change output (diff={diff} bytes out of {})", a.len());
}

#[test]
fn host_output_port_types_include_host_bridge_dynamic_outputs() {
    let frame_ty = TypeExpr::opaque("image:dynamic");
    let frame_json = serde_json::to_string(&frame_ty).expect("type json");

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
            ("dynamic_output_types".into(), Value::Map(vec![(Value::String(std::borrow::Cow::Owned("frame".into())), Value::String(std::borrow::Cow::Owned(frame_json)))])),
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
        edges: vec![(daedalus::planner::NodeRef(0), "frame".into(), daedalus::planner::NodeRef(1), "raw".into(), EdgePolicyKind::Fifo)],
        gpu_segments: vec![],
        gpu_edges: vec![],
        gpu_entries: vec![],
        gpu_exits: vec![],
        segments: vec![RuntimeSegment { nodes: vec![daedalus::planner::NodeRef(0), daedalus::planner::NodeRef(1)], compute: ComputeAffinity::CpuOnly }],
        schedule_order: vec![],
    };

    let registry = daedalus::registry::store::Registry::new();
    let inferred = infer_host_output_incoming_types(&plan, &registry);
    let output = inferred.get("output").expect("output host alias");
    assert_eq!(output.get("raw"), Some(&TypeExpr::opaque("image:dynamic")));
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
    let payload = EdgePayload::Payload(ErasedPayload::from_cpu::<DynamicImage>(DynamicImage::new_rgb8(1, 1)));
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
    let mut rolling = super::RollingGraphMetrics::new(
        8,
        vec![],
        vec![super::EdgeInfo {
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

    rolling.edge_samples.insert(0, VecDeque::from([(Instant::now(), super::EdgeMetricSample { samples: 1, max_depth: 2, current_depth: 1, ..Default::default() })]));

    let metrics = rolling.snapshot();
    let edge = metrics.edges.expect("edge metrics").get("edge_0").cloned().expect("edge_0");
    assert_eq!(edge.capacity, Some(4));
    assert_eq!(edge.policy.as_deref(), Some("Bounded { cap: 4 }"));
}

#[test]
fn host_bridge_input_port_infers_frame_from_solved_types() {
    let frame_ty = TypeExpr::opaque("image:dynamic");
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
    port_types.insert("overlay".to_string(), TypeExpr::opaque("image:dynamic"));
    port_types.insert("detections".to_string(), TypeExpr::list(TypeExpr::scalar(daedalus::data::model::ValueType::Int)));

    let host_output_port_owners = BTreeMap::new();
    let sinks =
        build_demand_sinks(&plan, &["Output".to_string()], &["raw".to_string()], &["raw".to_string(), "overlay".to_string(), "detections".to_string()], &port_types, &host_output_port_owners, true);

    let ports = sinks.into_iter().filter_map(|sink| sink.port).collect::<BTreeSet<_>>();
    assert!(ports.contains("raw"));
    assert!(ports.contains("detections"));
    assert!(!ports.contains("overlay"));
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
