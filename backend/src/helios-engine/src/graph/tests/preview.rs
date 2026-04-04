use super::*;

#[test]
fn grayscale_input_pref_allows_overlay_dynamic_preview() {
    let host_output_port_types =
        BTreeMap::from([("overlay".to_string(), daedalus_types::image_dynamic()), ("clahe".to_string(), daedalus_types::image_gray8()), ("adaptive".to_string(), daedalus_types::image_gray8())]);

    assert!(super::super::graph_prefers_grayscale_input(false, &["overlay".to_string()], &host_output_port_types,));
}

#[test]
fn grayscale_input_pref_rejects_generic_dynamic_frame_preview() {
    let host_output_port_types = BTreeMap::from([("frame".to_string(), daedalus_types::image_dynamic())]);

    assert!(!super::super::graph_prefers_grayscale_input(false, &["frame".to_string()], &host_output_port_types,));
}

#[test]
#[ignore = "requires the dynamic Daedalus CV plugin registry"]
fn fast_overlay_preview_keeps_grayscale_output_for_blank_luma_input() {
    let template_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../gaia/assets/pipelines/templates/daedalus_aruco_fast.json");
    let template_text = std::fs::read_to_string(&template_path).unwrap_or_else(|err| panic!("failed to read {}: {err}", template_path.display()));
    let template_json: serde_json::Value = serde_json::from_str(&template_text).expect("fast template json");
    let graph_json = template_json.get("graph").cloned().expect("fast template graph payload");

    let graph = super::super::GraphHandle::from_json_with_output(1, &graph_json, Some("overlay")).expect("fast graph");

    let frame = DynamicImage::ImageLuma8(GrayImage::from_pixel(1280, 800, Luma([32])));
    let output = graph.process_preview_with_options(frame, super::super::GraphProcessOptions { require_image_output: true, preview_only: true }).expect("overlay preview output");

    match output {
        super::super::GraphPreviewOutput::Gray(gray) => {
            assert_eq!(gray.dimensions(), (1280, 800));
        }
        super::super::GraphPreviewOutput::Image(DynamicImage::ImageLuma8(gray)) => {
            assert_eq!(gray.dimensions(), (1280, 800));
        }
        super::super::GraphPreviewOutput::Image(DynamicImage::ImageLumaA8(_)) => panic!("expected grayscale overlay preview output, got luma-alpha image"),
        super::super::GraphPreviewOutput::Image(DynamicImage::ImageRgb8(_)) => panic!("expected grayscale overlay preview output, got rgb8 image"),
        super::super::GraphPreviewOutput::Image(DynamicImage::ImageRgba8(_)) => panic!("expected grayscale overlay preview output, got rgba8 image"),
        super::super::GraphPreviewOutput::Image(DynamicImage::ImageRgb16(_)) => panic!("expected grayscale overlay preview output, got rgb16 image"),
        super::super::GraphPreviewOutput::Image(DynamicImage::ImageRgba16(_)) => panic!("expected grayscale overlay preview output, got rgba16 image"),
        super::super::GraphPreviewOutput::Image(DynamicImage::ImageRgb32F(_)) => panic!("expected grayscale overlay preview output, got rgb32f image"),
        super::super::GraphPreviewOutput::Image(DynamicImage::ImageRgba32F(_)) => panic!("expected grayscale overlay preview output, got rgba32f image"),
        super::super::GraphPreviewOutput::Image(DynamicImage::ImageLuma16(_)) => panic!("expected grayscale overlay preview output, got luma16 image"),
        super::super::GraphPreviewOutput::Image(DynamicImage::ImageLumaA16(_)) => panic!("expected grayscale overlay preview output, got luma-alpha16 image"),
        super::super::GraphPreviewOutput::Image(_) => panic!("expected grayscale overlay preview output, got another dynamic image variant"),
    }
}

#[test]
fn normalize_preview_output_keeps_overlay_luma_on_gray_fast_path() {
    let port_types = BTreeMap::from([("overlay".to_string(), daedalus_types::image_dynamic())]);
    let output = super::super::normalize_preview_output_for_port(super::super::GraphPreviewOutput::Image(DynamicImage::ImageLuma8(GrayImage::from_pixel(8, 8, Luma([12])))), "overlay", &port_types);

    match output {
        super::super::GraphPreviewOutput::Gray(gray) => assert_eq!(gray.dimensions(), (8, 8)),
        _ => panic!("expected overlay preview to stay on gray fast path"),
    }
}

#[test]
fn normalize_preview_output_converts_overlay_rgba_to_gray_fast_path() {
    let port_types = BTreeMap::from([("overlay".to_string(), daedalus_types::image_dynamic())]);
    let output = super::super::normalize_preview_output_for_port(
        super::super::GraphPreviewOutput::Image(DynamicImage::ImageRgba8(RgbaImage::from_pixel(8, 8, image::Rgba([0, 255, 0, 255])))),
        "overlay",
        &port_types,
    );

    match output {
        super::super::GraphPreviewOutput::Gray(gray) => assert_eq!(gray.dimensions(), (8, 8)),
        _ => panic!("expected overlay preview to force grayscale fast path"),
    }
}

#[test]
fn normalize_preview_output_leaves_generic_dynamic_preview_unchanged() {
    let port_types = BTreeMap::from([("frame".to_string(), daedalus_types::image_dynamic())]);
    let output = super::super::normalize_preview_output_for_port(super::super::GraphPreviewOutput::Image(DynamicImage::ImageLuma8(GrayImage::from_pixel(8, 8, Luma([12])))), "frame", &port_types);

    match output {
        super::super::GraphPreviewOutput::Image(DynamicImage::ImageLuma8(gray)) => assert_eq!(gray.dimensions(), (8, 8)),
        _ => panic!("expected generic dynamic preview to remain unchanged"),
    }
}

#[test]
fn selected_host_output_preview_with_sibling_image_port_emits_frame() {
    let image_ty = serde_json::to_string(&daedalus_types::image_dynamic()).expect("image type json");
    let graph_json = serde_json::json!({
        "nodes": [
            {
                "id": "io.host_bridge",
                "label": "Input",
                "inputs": [],
                "outputs": ["frame"],
                "metadata": {
                    "host_bridge": { "type": "Bool", "value": true },
                    "dynamic_output_types": {
                        "type": "Map",
                        "value": [
                            [
                                { "type": "String", "value": "frame" },
                                { "type": "String", "value": image_ty }
                            ]
                        ]
                    }
                }
            },
            {
                "id": "io.host_output",
                "label": "Output",
                "inputs": ["overlay", "clahe"],
                "outputs": [],
                "metadata": { "host_bridge": { "type": "Bool", "value": true } }
            }
        ],
        "edges": [
            { "from": { "node": 0, "port": "frame" }, "to": { "node": 1, "port": "overlay" } },
            { "from": { "node": 0, "port": "frame" }, "to": { "node": 1, "port": "clahe" } }
        ]
    });

    let graph = super::super::GraphHandle::from_json_with_output(1, &graph_json, Some("overlay")).expect("graph build");
    let input = DynamicImage::ImageLuma8(GrayImage::from_pixel(32, 24, Luma([77])));

    let output = graph.process_preview_with_options(input, super::super::GraphProcessOptions { require_image_output: true, preview_only: true });

    match output {
        Some(super::super::GraphPreviewOutput::Gray(gray)) => assert_eq!(gray.dimensions(), (32, 24)),
        Some(super::super::GraphPreviewOutput::Image(image)) => assert_eq!(image.dimensions(), (32, 24)),
        None => panic!("expected selected preview output to emit a frame"),
    }
}

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

    super::super::canonicalize_graph_const_inputs(&mut graph, &registry);

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
        .input("detections", daedalus_types::aruco_detections_2d())
        .input("frame_width", TypeExpr::scalar(daedalus::data::model::ValueType::Int))
        .input("frame_height", TypeExpr::scalar(daedalus::data::model::ValueType::Int))
        .input("crosshair_x", TypeExpr::scalar(daedalus::data::model::ValueType::Int))
        .input("crosshair_y", TypeExpr::scalar(daedalus::data::model::ValueType::Int))
        .input("mode", TypeExpr::r#enum(vec![EnumVariant { name: "nearest".to_string(), ty: None }]))
        .input("fallback_to_nearest", TypeExpr::scalar(daedalus::data::model::ValueType::Bool))
        .fanin_input("ins", 0, TypeExpr::scalar(daedalus::data::model::ValueType::Int))
        .output("target", daedalus_types::aruco_detection_2d())
        .build()
        .expect("descriptor build");

    let mut registry = PluginRegistry::new();
    registry.registry.register_node(desc).expect("descriptor register");

    super::super::sync_graph_node_port_declarations(&mut graph, &registry);

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
fn sync_graph_node_port_declarations_rejects_plus_suffix_fanin_names() {
    let graph_json = serde_json::json!({
        "nodes": [
            {
                "id": "cv:test:fanin",
                "inputs": ["sources0+"],
                "outputs": [],
                "const_inputs": []
            }
        ],
        "edges": []
    });
    let mut graph: daedalus::planner::Graph = serde_json::from_value(graph_json).expect("graph parse");

    let desc = NodeDescriptorBuilder::new("cv:test:fanin").fanin_input("sources", 0, TypeExpr::scalar(daedalus::data::model::ValueType::Int)).build().expect("descriptor build");

    let mut registry = PluginRegistry::new();
    registry.registry.register_node(desc).expect("descriptor register");

    super::super::sync_graph_node_port_declarations(&mut graph, &registry);

    assert_eq!(graph.nodes[0].inputs, Vec::<String>::new());
}

#[test]
#[ignore = "requires the dynamic Daedalus CV plugin (cv:image:undistort_optional) to be installed/loaded"]
fn raw_stream_graph_can_select_undistorted_output() {
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
        k1: -0.35,
        k2: 0.10,
        p1: 0.0,
        p2: 0.0,
        k3: 0.0,
        undistort_iters: 5,
        lens_model: lib_cv::modules::calibration::LensModel::Pinhole,
    };

    let raw = super::super::GraphHandle::from_persisted_json_with_output(1, &graph_json, Some("frame")).expect("raw graph");
    raw.set_calibration(Some(calib.clone()));
    let out_raw = raw.process(input.clone()).expect("raw output");

    let undist = super::super::GraphHandle::from_persisted_json_with_output(1, &graph_json, Some("undistorted")).expect("undist graph");
    undist.set_calibration(Some(calib));
    let out_undist = undist.process(input).expect("undistorted output");

    assert_eq!(out_raw.dimensions(), out_undist.dimensions());

    let a = out_raw.to_rgba8().into_raw();
    let b = out_undist.to_rgba8().into_raw();
    let diff = a.iter().zip(b.iter()).filter(|(x, y)| x != y).count();
    assert!(diff > (a.len() / 200), "expected undistortion to change output (diff={diff} bytes out of {})", a.len());
}

#[test]
fn host_output_port_types_include_host_bridge_dynamic_outputs() {
    let frame_ty = daedalus_types::image_dynamic();
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
    assert_eq!(output.get("raw"), Some(&daedalus_types::image_dynamic()));
}

#[test]
fn default_host_bridge_inputs_cover_overlay_crosshair_toggle() {
    assert_eq!(super::super::default_host_bridge_input_value("crosshair_x"), Some(Value::Int(0)));
    assert_eq!(super::super::default_host_bridge_input_value("crosshair_y"), Some(Value::Int(0)));
    assert_eq!(super::super::default_host_bridge_input_value("draw_crosshair"), Some(Value::Bool(false)));
    assert_eq!(super::super::default_host_bridge_input_value("order_mode"), Some(Value::String(std::borrow::Cow::Borrowed("none"))));
}
