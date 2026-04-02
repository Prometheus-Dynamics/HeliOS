use super::*;

pub(super) fn calibration_mode_output_port(_template_id: &str) -> &'static str {
    "frame"
}

pub(super) fn calibration_mode_host_buffer(_default_host_buffer: usize) -> usize {
    CALIBRATION_MODE_HOST_BUFFER
}

pub(super) fn load_calibration_mode_graph_json() -> std::io::Result<serde_json::Value> {
    crate::pipelines::load_template_graph_json(CALIBRATION_TEMPLATE_ID)
}

pub(super) fn patch_dictionary_const(graph: &mut serde_json::Value, dictionary: &str) {
    let Some(nodes) = graph.get_mut("nodes").and_then(|v| v.as_array_mut()) else {
        return;
    };
    let upsert = |consts: &mut Vec<serde_json::Value>, name: &str, value: &str| {
        for entry in consts.iter_mut() {
            let Some(pair) = entry.as_array_mut() else { continue };
            if pair.len() != 2 {
                continue;
            }
            if pair[0].as_str() == Some(name) {
                pair[1] = serde_json::json!({ "type": "String", "value": value });
                return;
            }
        }
        consts.push(serde_json::json!([name, { "type": "String", "value": value }]));
    };
    let mut patched_nodes = 0usize;
    for node in nodes {
        let Some(id) = node.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        if !id.starts_with("cv:aruco:") {
            continue;
        }
        let accepts_dictionary =
            node.get("inputs").and_then(|v| v.as_array()).is_some_and(|inputs| inputs.iter().any(|name| name.as_str().is_some_and(|value| value.eq_ignore_ascii_case("dictionary"))));
        if !accepts_dictionary {
            continue;
        }

        let consts = match node.get_mut("const_inputs").and_then(|v| v.as_array_mut()) {
            Some(arr) => arr,
            None => {
                let Some(obj) = node.as_object_mut() else {
                    continue;
                };
                obj.insert("const_inputs".into(), serde_json::Value::Array(Vec::new()));
                match obj.get_mut("const_inputs").and_then(|v| v.as_array_mut()) {
                    Some(arr) => arr,
                    None => continue,
                }
            }
        };
        upsert(consts, "dictionary", dictionary);
        patched_nodes += 1;
    }

    if patched_nodes == 0 {
        tracing::warn!("calibration dictionary patch skipped: no aruco nodes with dictionary input found");
    }
}

pub(super) fn upsert_const_input_bool(node: &mut serde_json::Value, name: &str, value: bool) -> bool {
    let Some(obj) = node.as_object_mut() else {
        return false;
    };
    if !obj.contains_key("const_inputs") {
        obj.insert("const_inputs".into(), serde_json::Value::Array(Vec::new()));
    }
    let Some(consts) = obj.get_mut("const_inputs").and_then(|v| v.as_array_mut()) else {
        return false;
    };
    for entry in consts.iter_mut() {
        let Some(pair) = entry.as_array_mut() else { continue };
        if pair.len() != 2 {
            continue;
        }
        if pair[0].as_str() == Some(name) {
            pair[1] = serde_json::json!({ "type": "Bool", "value": value });
            return true;
        }
    }
    consts.push(serde_json::json!([name, { "type": "Bool", "value": value }]));
    true
}

pub(super) fn patch_calibration_mode_detection_strictness(graph: &mut serde_json::Value, dictionary: Option<&str>) {
    let Some(nodes) = graph.get_mut("nodes").and_then(|v| v.as_array_mut()) else {
        return;
    };
    let selected_dictionary = dictionary.unwrap_or_default().trim().to_ascii_lowercase();
    let dictionary_max_id = calibration_dictionary_max_id(&selected_dictionary);

    let mut overlay_nodes = 0usize;

    for node in nodes {
        let Some(id) = node.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        if id == "cv:aruco:overlay_detections" {
            // Keep calibration stream clean by default; overlays are for explicit debug only.
            let _ = upsert_const_input_bool(node, "draw_boxes", false);
            let _ = upsert_const_input_bool(node, "draw_corners", false);
            let _ = upsert_const_input_bool(node, "draw_ids", false);
            let _ = upsert_const_input_bool(node, "draw_hud", false);
            overlay_nodes += 1;
        }
    }

    tracing::info!(
        dictionary = selected_dictionary,
        dictionary_max_id = ?dictionary_max_id,
        overlay_nodes,
        "patched calibration mode graph defaults"
    );
}

pub(super) fn ensure_calibration_mode_frame_output(graph: &mut serde_json::Value) {
    let Some(nodes) = graph.get_mut("nodes").and_then(|v| v.as_array_mut()) else {
        return;
    };

    let mut host_bridge_idx: Option<usize> = None;
    let mut host_output_idx: Option<usize> = None;

    for (idx, node) in nodes.iter_mut().enumerate() {
        let Some(id) = node.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        if id == "io.host_bridge" || id.ends_with(":io.host_bridge") {
            host_bridge_idx = Some(idx);
            let Some(obj) = node.as_object_mut() else {
                continue;
            };
            if !obj.get("metadata").is_some_and(|value| value.is_object()) {
                obj.insert("metadata".into(), serde_json::json!({}));
            }
            let Some(metadata) = obj.get_mut("metadata").and_then(|value| value.as_object_mut()) else {
                continue;
            };
            // `io.host_bridge` can expose multiple outputs (frame + ROI ports). Force the
            // bridge input selection so graph build does not fail with ambiguous input ports.
            metadata.insert("helios.host_input_port".into(), serde_json::json!({ "type": "String", "value": "frame" }));
        } else if id == "io.host_output" || id.ends_with(":io.host_output") {
            host_output_idx = Some(idx);
            let Some(obj) = node.as_object_mut() else {
                continue;
            };
            if !obj.contains_key("inputs") {
                obj.insert("inputs".into(), serde_json::Value::Array(Vec::new()));
            }
            let Some(inputs) = obj.get_mut("inputs").and_then(|v| v.as_array_mut()) else {
                continue;
            };
            let has_frame = inputs.iter().any(|value| value.as_str() == Some("frame"));
            if !has_frame {
                inputs.push(serde_json::Value::String("frame".to_string()));
            }
        }
    }

    let (Some(from_idx), Some(to_idx)) = (host_bridge_idx, host_output_idx) else {
        tracing::warn!("calibration mode frame output patch skipped: missing io.host_bridge or io.host_output");
        return;
    };

    let Some(edges) = graph.get_mut("edges").and_then(|v| v.as_array_mut()) else {
        return;
    };
    let has_frame_edge = edges.iter().any(|edge| {
        let from_node = edge.get("from").and_then(|v| v.get("node")).and_then(|v| v.as_u64());
        let from_port = edge.get("from").and_then(|v| v.get("port")).and_then(|v| v.as_str());
        let to_node = edge.get("to").and_then(|v| v.get("node")).and_then(|v| v.as_u64());
        let to_port = edge.get("to").and_then(|v| v.get("port")).and_then(|v| v.as_str());
        from_node == Some(from_idx as u64) && from_port == Some("frame") && to_node == Some(to_idx as u64) && to_port == Some("frame")
    });
    if !has_frame_edge {
        edges.push(serde_json::json!({
            "from": { "node": from_idx, "port": "frame" },
            "to": { "node": to_idx, "port": "frame" }
        }));
    }
}

pub(super) fn ensure_calibration_mode_detections_json_output(graph: &mut serde_json::Value) {
    let Some(nodes) = graph.get_mut("nodes").and_then(|v| v.as_array_mut()) else {
        return;
    };

    let mut host_output_idx: Option<usize> = None;
    let mut detections_json_idx: Option<usize> = None;

    for (idx, node) in nodes.iter_mut().enumerate() {
        let Some(id) = node.get("id").and_then(|v| v.as_str()) else {
            continue;
        };

        if id == "io.host_output" {
            host_output_idx = Some(idx);
            let Some(obj) = node.as_object_mut() else {
                continue;
            };
            if !obj.contains_key("inputs") {
                obj.insert("inputs".into(), serde_json::Value::Array(Vec::new()));
            }
            let Some(inputs) = obj.get_mut("inputs").and_then(|v| v.as_array_mut()) else {
                continue;
            };
            if !inputs.iter().any(|value| value.as_str() == Some("detections_json")) {
                inputs.push(serde_json::Value::String("detections_json".to_string()));
            }
        } else if id == "cv:aruco:detections_json" {
            detections_json_idx = Some(idx);
        }
    }

    let Some(host_output_idx) = host_output_idx else {
        tracing::warn!("calibration mode detections_json patch skipped: missing io.host_output");
        return;
    };

    if detections_json_idx.is_none() {
        nodes.push(serde_json::json!({
            "bundle": null,
            "compute": "CpuOnly",
            "const_inputs": [
                ["min_id", { "type": "Int", "value": -1 }],
                ["max_id", { "type": "Int", "value": -1 }]
            ],
            "id": "cv:aruco:detections_json",
            "inputs": ["detections", "min_id", "max_id"],
            "label": "Detections JSON",
            "metadata": {},
            "outputs": ["json"]
        }));
        detections_json_idx = Some(nodes.len() - 1);
    }

    let Some(detections_json_idx) = detections_json_idx else {
        return;
    };

    let Some(edges) = graph.get_mut("edges").and_then(|v| v.as_array_mut()) else {
        return;
    };

    let detections_source = edges.iter().find_map(|edge| {
        let to_node = edge.get("to").and_then(|v| v.get("node")).and_then(|v| v.as_u64())?;
        let to_port = edge.get("to").and_then(|v| v.get("port")).and_then(|v| v.as_str())?;
        if to_node != host_output_idx as u64 || to_port != "detections" {
            return None;
        }
        let from_node = edge.get("from").and_then(|v| v.get("node")).and_then(|v| v.as_u64())?;
        let from_port = edge.get("from").and_then(|v| v.get("port")).and_then(|v| v.as_str())?;
        Some((from_node, from_port.to_string()))
    });

    let Some((source_node, source_port)) = detections_source else {
        tracing::warn!("calibration mode detections_json patch skipped: missing detections source edge");
        return;
    };

    let has_source_edge = edges.iter().any(|edge| {
        let from_node = edge.get("from").and_then(|v| v.get("node")).and_then(|v| v.as_u64());
        let from_port = edge.get("from").and_then(|v| v.get("port")).and_then(|v| v.as_str());
        let to_node = edge.get("to").and_then(|v| v.get("node")).and_then(|v| v.as_u64());
        let to_port = edge.get("to").and_then(|v| v.get("port")).and_then(|v| v.as_str());
        from_node == Some(source_node) && from_port == Some(source_port.as_str()) && to_node == Some(detections_json_idx as u64) && to_port == Some("detections")
    });
    if !has_source_edge {
        edges.push(serde_json::json!({
            "from": { "node": source_node, "port": source_port },
            "to": { "node": detections_json_idx, "port": "detections" }
        }));
    }

    let has_output_edge = edges.iter().any(|edge| {
        let from_node = edge.get("from").and_then(|v| v.get("node")).and_then(|v| v.as_u64());
        let from_port = edge.get("from").and_then(|v| v.get("port")).and_then(|v| v.as_str());
        let to_node = edge.get("to").and_then(|v| v.get("node")).and_then(|v| v.as_u64());
        let to_port = edge.get("to").and_then(|v| v.get("port")).and_then(|v| v.as_str());
        from_node == Some(detections_json_idx as u64) && from_port == Some("json") && to_node == Some(host_output_idx as u64) && to_port == Some("detections_json")
    });
    if !has_output_edge {
        edges.push(serde_json::json!({
            "from": { "node": detections_json_idx, "port": "json" },
            "to": { "node": host_output_idx, "port": "detections_json" }
        }));
    }
}

pub(super) fn normalize_calibration_dictionary_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = trimmed.to_ascii_lowercase();
    let mut candidates = vec![normalized.clone()];
    if let Some(stripped) = normalized.strip_prefix("dict_") {
        candidates.push(stripped.to_string());
    }
    if let Some(stripped) = normalized.strip_prefix("dict") {
        if stripped.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
            candidates.push(stripped.to_string());
        }
    }

    candidates.into_iter().find(|candidate| lib_cv::modules::aruco::ArucoDictionaryKind::from_str(candidate).is_ok())
}

pub(super) fn calibration_dictionary_max_id(dictionary: &str) -> Option<i64> {
    let normalized = dictionary.trim().to_ascii_lowercase();
    let (_, count_str) = normalized.split_once('_')?;
    if !count_str.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let count = count_str.parse::<i64>().ok()?;
    (count > 0).then_some(count - 1)
}
