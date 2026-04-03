use super::*;

pub(super) fn map_config(cfg: &CalibrationSolveConfig) -> CvSolveConfig {
    CvSolveConfig {
        min_views: cfg.min_views.max(1) as usize,
        min_points_per_view: cfg.min_points_per_view.max(1) as usize,
        refine_distortion: cfg.refine_distortion,
        undistort_iters: cfg.undistort_iters,
        refine_undistort_iters: cfg.refine_undistort_iters,
        lens_model: cfg.lens_model,
    }
}

pub(super) fn load_graph_json(request: &CalibrationSolveRequest) -> Result<JsonValue, CalibrationSolveFailure> {
    let dictionary_override = request
        .board
        .dictionary
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|value| normalize_calibration_dictionary_name(value).ok_or_else(|| CalibrationSolveFailure::InvalidInput(format!("unknown board dictionary '{value}'"))))
        .transpose()?;
    let max_board_id = board_marker_capacity(&request.board).saturating_sub(1);

    if let Some(graph) = request.graph.as_ref() {
        let mut graph = graph.as_value();
        if let Some(dictionary) = dictionary_override.as_deref() {
            patch_dictionary_const(&mut graph, dictionary);
        }
        patch_id_range_consts(&mut graph, 0, max_board_id as i64);
        return Ok(graph);
    }
    if let Some(graph_id) = request.graph_id {
        let mut graph = crate::pipelines::load_pipeline_graph_json(graph_id).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                CalibrationSolveFailure::NotFound(format!("graph {graph_id} not found"))
            } else {
                CalibrationSolveFailure::Internal(format!("failed to load graph {graph_id}: {err}"))
            }
        })?;
        if let Some(dictionary) = dictionary_override.as_deref() {
            patch_dictionary_const(&mut graph, dictionary);
        }
        patch_id_range_consts(&mut graph, 0, max_board_id as i64);
        return Ok(graph);
    }
    if let Some(template_id) = request.graph_template_id.as_deref() {
        let mut graph = crate::pipelines::load_template_graph_json(template_id).map_err(|err| CalibrationSolveFailure::Internal(format!("failed to load template {template_id}: {err}")))?;
        if let Some(dictionary) = dictionary_override.as_deref() {
            patch_dictionary_const(&mut graph, dictionary);
        }
        patch_id_range_consts(&mut graph, 0, max_board_id as i64);
        return Ok(graph);
    }
    Err(CalibrationSolveFailure::InvalidInput("calibration graph missing (graph, graph_id, or graph_template_id required)".into()))
}

pub(super) fn compact_calibration_graph_error(raw: &str) -> String {
    if !raw.contains("planner diagnostics") || !raw.contains("graph is missing input port `") {
        return format!("invalid calibration graph: {raw}");
    }

    let mut by_node: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let needle = "graph is missing input port `";
    let mut rest = raw;
    while let Some(idx) = rest.find(needle) {
        rest = &rest[idx + needle.len()..];
        let Some(port_end) = rest.find('`') else {
            break;
        };
        let port = rest[..port_end].trim();
        let after_port = &rest[port_end + 1..];

        let node = after_port
            .find(" on node ")
            .map(|node_idx| {
                let after_node = &after_port[node_idx + " on node ".len()..];
                let end = after_node.find([' ', '(', ',', '"']).unwrap_or(after_node.len());
                after_node[..end].trim()
            })
            .unwrap_or_default();

        if !port.is_empty() && !node.is_empty() {
            let entry = by_node.entry(node.to_string()).or_default();
            if !entry.iter().any(|existing| existing == port) {
                entry.push(port.to_string());
            }
        }

        rest = after_port;
    }

    if by_node.is_empty() {
        return "invalid calibration graph: stale node port declarations; regenerate graph ports from registry".to_string();
    }

    let summary = by_node
        .into_iter()
        .map(|(node, mut ports)| {
            ports.sort();
            let missing_total = ports.len();
            let mut sample = ports;
            sample.truncate(3);
            if missing_total > sample.len() {
                format!("{node} missing {missing_total} (e.g. {})", sample.join(", "))
            } else {
                format!("{node} missing {missing_total} ({})", sample.join(", "))
            }
        })
        .collect::<Vec<_>>()
        .join("; ");

    format!("invalid calibration graph: stale node port declarations: {summary}. Regenerate graph ports from registry.")
}

pub(super) fn patch_dictionary_const(graph: &mut JsonValue, dictionary: &str) {
    let Some(nodes) = graph.get_mut("nodes").and_then(|v| v.as_array_mut()) else {
        return;
    };
    for node in nodes {
        let Some(id) = node.get("id").and_then(|v| v.as_str()) else { continue };
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
                let Some(obj) = node.as_object_mut() else { continue };
                obj.insert("const_inputs".into(), JsonValue::Array(Vec::new()));
                match obj.get_mut("const_inputs").and_then(|v| v.as_array_mut()) {
                    Some(arr) => arr,
                    None => continue,
                }
            }
        };
        let mut found = false;
        for entry in consts.iter_mut() {
            let Some(pair) = entry.as_array_mut() else { continue };
            if pair.len() != 2 {
                continue;
            }
            if pair[0].as_str() == Some("dictionary") {
                pair[1] = serde_json::json!({ "type": "String", "value": dictionary });
                found = true;
                break;
            }
        }
        if !found {
            consts.push(serde_json::json!(["dictionary", { "type": "String", "value": dictionary }]));
        }
    }
}

pub(super) fn patch_id_range_consts(graph: &mut JsonValue, min_id: i64, max_id: i64) {
    let Some(nodes) = graph.get_mut("nodes").and_then(|v| v.as_array_mut()) else {
        return;
    };
    for node in nodes {
        let Some(id) = node.get("id").and_then(|v| v.as_str()) else { continue };
        if !id.starts_with("cv:aruco:") {
            continue;
        }
        let has_min = node.get("inputs").and_then(|v| v.as_array()).is_some_and(|inputs| inputs.iter().any(|name| name.as_str().is_some_and(|value| value.eq_ignore_ascii_case("min_id"))));
        let has_max = node.get("inputs").and_then(|v| v.as_array()).is_some_and(|inputs| inputs.iter().any(|name| name.as_str().is_some_and(|value| value.eq_ignore_ascii_case("max_id"))));
        if !has_min && !has_max {
            continue;
        }

        let consts = match node.get_mut("const_inputs").and_then(|v| v.as_array_mut()) {
            Some(arr) => arr,
            None => {
                let Some(obj) = node.as_object_mut() else { continue };
                obj.insert("const_inputs".into(), JsonValue::Array(Vec::new()));
                match obj.get_mut("const_inputs").and_then(|v| v.as_array_mut()) {
                    Some(arr) => arr,
                    None => continue,
                }
            }
        };

        if has_min {
            upsert_i64_const(consts, "min_id", min_id);
        }
        if has_max {
            upsert_i64_const(consts, "max_id", max_id);
        }
    }
}

pub(super) fn upsert_i64_const(consts: &mut Vec<JsonValue>, key: &str, value: i64) {
    for entry in consts.iter_mut() {
        let Some(pair) = entry.as_array_mut() else { continue };
        if pair.len() != 2 {
            continue;
        }
        if pair[0].as_str() == Some(key) {
            pair[1] = serde_json::json!({ "type": "Int", "value": value });
            return;
        }
    }
    consts.push(serde_json::json!([key, { "type": "Int", "value": value }]));
}

