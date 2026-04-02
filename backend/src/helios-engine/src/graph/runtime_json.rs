use super::*;

pub(super) fn json_to_daedalus_value(value: &Value) -> DaedalusValue {
    match value {
        Value::Null => DaedalusValue::Unit,
        Value::Bool(b) => DaedalusValue::Bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                DaedalusValue::Int(i)
            } else if let Some(f) = n.as_f64() {
                DaedalusValue::Float(f)
            } else {
                DaedalusValue::Unit
            }
        }
        Value::String(s) => DaedalusValue::String(s.to_string().into()),
        Value::Array(items) => DaedalusValue::List(items.iter().map(json_to_daedalus_value).collect()),
        Value::Object(map) => {
            if map.contains_key("type") && map.contains_key("value") {
                return DaedalusValue::Unit;
            }
            DaedalusValue::Map(map.iter().map(|(k, v)| (DaedalusValue::String(k.to_string().into()), json_to_daedalus_value(v))).collect())
        }
    }
}

pub(super) fn calibration_to_daedalus_value(calibration: Option<crate::ipc::StreamCalibration>) -> DaedalusValue {
    let calib = calibration.unwrap_or(crate::ipc::StreamCalibration {
        fx: 0.0,
        fy: 0.0,
        cx: 0.0,
        cy: 0.0,
        k1: 0.0,
        k2: 0.0,
        p1: 0.0,
        p2: 0.0,
        k3: 0.0,
        undistort_iters: 5,
        lens_model: lib_cv::modules::calibration::LensModel::Pinhole,
    });

    let lens_model = match calib.lens_model {
        lib_cv::modules::calibration::LensModel::Pinhole => "pinhole",
        lib_cv::modules::calibration::LensModel::Fisheye => "fisheye",
    };

    DaedalusValue::Struct(vec![
        StructFieldValue { name: "fx".into(), value: DaedalusValue::Float(calib.fx) },
        StructFieldValue { name: "fy".into(), value: DaedalusValue::Float(calib.fy) },
        StructFieldValue { name: "cx".into(), value: DaedalusValue::Float(calib.cx) },
        StructFieldValue { name: "cy".into(), value: DaedalusValue::Float(calib.cy) },
        StructFieldValue { name: "k1".into(), value: DaedalusValue::Float(calib.k1) },
        StructFieldValue { name: "k2".into(), value: DaedalusValue::Float(calib.k2) },
        StructFieldValue { name: "p1".into(), value: DaedalusValue::Float(calib.p1) },
        StructFieldValue { name: "p2".into(), value: DaedalusValue::Float(calib.p2) },
        StructFieldValue { name: "k3".into(), value: DaedalusValue::Float(calib.k3) },
        StructFieldValue { name: "lensModel".into(), value: DaedalusValue::Enum(daedalus::data::model::EnumValue { name: lens_model.into(), value: None }) },
        StructFieldValue { name: "undistortIters".into(), value: DaedalusValue::Int(calib.undistort_iters) },
    ])
}

pub(super) fn default_host_bridge_input_value(port_lc: &str) -> Option<DaedalusValue> {
    match port_lc {
        "roi_x" | "roi_y" | "roi_w" | "roi_h" => Some(DaedalusValue::Int(0)),
        "crosshair_x" | "crosshair_y" => Some(DaedalusValue::Int(0)),
        "draw_crosshair" => Some(DaedalusValue::Bool(false)),
        "order_mode" => Some(DaedalusValue::String("none".into())),
        _ => None,
    }
}

pub(super) fn metadata_bool_flag(map: &BTreeMap<String, DaedalusValue>, key: &str) -> Option<bool> {
    map.get(key).and_then(daedalus_value_as_bool).or_else(|| {
        map.get(key).and_then(|value| match value {
            DaedalusValue::String(raw) => {
                let raw = raw.trim().to_ascii_lowercase();
                match raw.as_str() {
                    "1" | "true" | "yes" | "on" => Some(true),
                    "0" | "false" | "no" | "off" => Some(false),
                    _ => None,
                }
            }
            _ => None,
        })
    })
}

fn normalize_graph_metadata(graph: &mut Value) {
    let Some(obj) = graph.as_object_mut() else { return };
    let Some(values) = obj.get_mut("metadata") else { return };
    let Some(values_obj) = values.as_object_mut() else { return };

    for (_, value) in values_obj.iter_mut() {
        if let Value::Object(map) = value {
            if map.contains_key("type") && map.contains_key("value") {
                continue;
            }
        }
    }
}

fn ensure_host_bridge_node_shape(node: &mut serde_json::Map<String, Value>) {
    let id = node.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let is_host_bridge = id == "io.host_bridge" || id.ends_with(":io.host_bridge");
    let is_host_output = id == "io.host_output" || id.ends_with(":io.host_output");
    if !is_host_bridge && !is_host_output {
        return;
    }

    let metadata = node.entry("metadata").or_insert_with(|| serde_json::json!({}));
    if let Some(meta_obj) = metadata.as_object_mut() {
        meta_obj.entry(HOST_BRIDGE_META_KEY.to_string()).or_insert_with(|| serde_json::to_value(DaedalusValue::Bool(true)).unwrap_or(Value::Null));
    }

    if is_host_bridge {
        let _ = node.entry("outputs").or_insert_with(|| serde_json::json!([]));
    } else if is_host_output {
        let _ = node.entry("inputs").or_insert_with(|| serde_json::json!([]));
    }
}

fn json_string_metadata_value(value: &Value) -> Option<&str> {
    match value {
        Value::String(raw) => Some(raw.as_str()),
        Value::Object(map) => {
            let ty = map.get("type").and_then(|raw| raw.as_str())?;
            if !ty.eq_ignore_ascii_case("string") {
                return None;
            }
            map.get("value").and_then(|raw| raw.as_str())
        }
        _ => None,
    }
}

fn set_json_string_metadata_value(slot: &mut Value, raw: String) {
    match slot {
        Value::Object(map) if map.get("type").and_then(|value| value.as_str()).is_some_and(|ty| ty.eq_ignore_ascii_case("string")) => {
            map.insert("value".to_string(), Value::String(raw));
        }
        _ => *slot = Value::String(raw),
    }
}

fn prune_host_output_metadata_string_list(metadata: &mut serde_json::Map<String, Value>, key: &str, keep: &BTreeSet<String>) {
    let Some(raw) = metadata.get(key).and_then(json_string_metadata_value).map(str::to_string) else {
        return;
    };
    let filtered = raw
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .filter(|entry| {
            let port = entry.split_once(':').map(|(name, _)| name).unwrap_or(*entry).trim();
            keep.contains(&port.to_ascii_lowercase())
        })
        .map(str::to_string)
        .collect::<Vec<_>>();

    if filtered.is_empty() {
        metadata.remove(key);
        return;
    }

    if let Some(slot) = metadata.get_mut(key) {
        set_json_string_metadata_value(slot, filtered.join(","));
    }
}

fn prune_host_output_metadata_display_map(metadata: &mut serde_json::Map<String, Value>, key: &str, keep: &BTreeSet<String>) {
    let Some(raw) = metadata.get(key).and_then(json_string_metadata_value).map(str::to_string) else {
        return;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Map<String, Value>>(&raw) else {
        return;
    };
    let filtered = parsed.into_iter().filter(|(name, _)| keep.contains(&name.trim().to_ascii_lowercase())).collect::<serde_json::Map<String, Value>>();

    if filtered.is_empty() {
        metadata.remove(key);
        return;
    }

    let serialized = match serde_json::to_string(&filtered) {
        Ok(value) => value,
        Err(_) => return,
    };
    if let Some(slot) = metadata.get_mut(key) {
        set_json_string_metadata_value(slot, serialized);
    }
}

fn prune_disconnected_host_output_ports(nodes: &mut [Value], edges: &[Value]) {
    let mut connected_ports: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    let mut seen_ports: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();

    for edge in edges {
        let Some(to) = edge.get("to").and_then(|value| value.as_object()) else {
            continue;
        };
        let Some(node_idx) = to.get("node").and_then(|value| value.as_u64()).map(|value| value as usize) else {
            continue;
        };
        let Some(port) = to.get("port").and_then(|value| value.as_str()).map(str::trim).filter(|value| !value.is_empty()) else {
            continue;
        };
        let Some(node_obj) = nodes.get(node_idx).and_then(|value| value.as_object()) else {
            continue;
        };
        let id = node_obj.get("id").and_then(|value| value.as_str()).unwrap_or("");
        if !(id == "io.host_output" || id.ends_with(":io.host_output")) {
            continue;
        }
        let key = port.to_ascii_lowercase();
        if seen_ports.entry(node_idx).or_default().insert(key) {
            connected_ports.entry(node_idx).or_default().push(port.to_string());
        }
    }

    for (idx, node) in nodes.iter_mut().enumerate() {
        let Some(node_obj) = node.as_object_mut() else { continue };
        let id = node_obj.get("id").and_then(|value| value.as_str()).unwrap_or("");
        if !(id == "io.host_output" || id.ends_with(":io.host_output")) {
            continue;
        }

        let discovered = connected_ports.remove(&idx).unwrap_or_default();
        let keep = discovered.iter().map(|port| port.to_ascii_lowercase()).collect::<BTreeSet<_>>();

        let mut pruned_inputs = Vec::new();
        let mut inserted = BTreeSet::new();
        if let Some(inputs) = node_obj.get("inputs").and_then(|value| value.as_array()) {
            for input in inputs {
                let Some(name) = input.as_str().map(str::trim).filter(|value| !value.is_empty()) else {
                    continue;
                };
                let key = name.to_ascii_lowercase();
                if keep.contains(&key) && inserted.insert(key) {
                    pruned_inputs.push(Value::String(name.to_string()));
                }
            }
        }
        for port in discovered {
            let key = port.to_ascii_lowercase();
            if inserted.insert(key) {
                pruned_inputs.push(Value::String(port));
            }
        }
        node_obj.insert("inputs".to_string(), Value::Array(pruned_inputs));

        let Some(metadata) = node_obj.get_mut("metadata").and_then(|value| value.as_object_mut()) else {
            continue;
        };
        prune_host_output_metadata_string_list(metadata, "host_bridge_inputs", &keep);
        prune_host_output_metadata_display_map(metadata, "host_bridge_inputs_display", &keep);
    }
}

fn prune_isolated_runtime_nodes(graph_obj: &mut serde_json::Map<String, Value>) {
    let Some(nodes) = graph_obj.get("nodes").and_then(|value| value.as_array()) else {
        return;
    };
    let Some(edges) = graph_obj.get("edges").and_then(|value| value.as_array()) else {
        return;
    };

    let mut degree = vec![0usize; nodes.len()];
    for edge in edges {
        let Some(edge_obj) = edge.as_object() else { continue };
        for endpoint in ["from", "to"] {
            let Some(node_idx) = edge_obj.get(endpoint).and_then(|value| value.as_object()).and_then(|value| value.get("node")).and_then(|value| value.as_u64()).map(|value| value as usize) else {
                continue;
            };
            if let Some(count) = degree.get_mut(node_idx) {
                *count += 1;
            }
        }
    }

    if degree.iter().all(|count| *count > 0) {
        return;
    }

    let mut remap: Vec<Option<usize>> = vec![None; nodes.len()];
    let mut kept_nodes = Vec::with_capacity(nodes.len());
    for (idx, node) in nodes.iter().enumerate() {
        if degree.get(idx).copied().unwrap_or_default() == 0 {
            continue;
        }
        remap[idx] = Some(kept_nodes.len());
        kept_nodes.push(node.clone());
    }

    let mut kept_edges = Vec::with_capacity(edges.len());
    for edge in edges {
        let mut edge_value = edge.clone();
        let Some(edge_obj) = edge_value.as_object_mut() else { continue };
        let mut keep_edge = true;
        for endpoint in ["from", "to"] {
            let Some(endpoint_obj) = edge_obj.get_mut(endpoint).and_then(|value| value.as_object_mut()) else {
                keep_edge = false;
                break;
            };
            let Some(old_idx) = endpoint_obj.get("node").and_then(|value| value.as_u64()).map(|value| value as usize) else {
                keep_edge = false;
                break;
            };
            let Some(new_idx) = remap.get(old_idx).and_then(|value| *value) else {
                keep_edge = false;
                break;
            };
            endpoint_obj.insert("node".to_string(), Value::from(new_idx as u64));
        }
        if keep_edge {
            kept_edges.push(edge_value);
        }
    }

    graph_obj.insert("nodes".to_string(), Value::Array(kept_nodes));
    graph_obj.insert("edges".to_string(), Value::Array(kept_edges));
}

pub(super) fn normalize_graph_json_for_runtime(json: &Value) -> Value {
    let mut normalized = json.clone();
    normalize_graph_metadata(&mut normalized);

    let Some(obj) = normalized.as_object_mut() else {
        return normalized;
    };
    let edges = obj.get("edges").and_then(|value| value.as_array()).cloned().unwrap_or_default();
    let Some(nodes) = obj.get_mut("nodes").and_then(|v| v.as_array_mut()) else {
        return normalized;
    };
    for node in &mut *nodes {
        let Some(node_obj) = node.as_object_mut() else { continue };
        ensure_host_bridge_node_shape(node_obj);
    }
    prune_disconnected_host_output_ports(nodes, &edges);
    prune_isolated_runtime_nodes(obj);
    normalized
}

fn prune_unselected_host_output_edges(graph_obj: &mut serde_json::Map<String, Value>, keep_port_lc: &str) {
    let Some(nodes) = graph_obj.get("nodes").and_then(|value| value.as_array()) else {
        return;
    };
    let host_output_nodes = nodes
        .iter()
        .enumerate()
        .filter_map(|(idx, node)| {
            let node_obj = node.as_object()?;
            let id = node_obj.get("id").and_then(|value| value.as_str()).unwrap_or("");
            ((id == "io.host_output") || id.ends_with(":io.host_output")).then_some(idx)
        })
        .collect::<BTreeSet<_>>();
    if host_output_nodes.is_empty() {
        return;
    }

    let Some(edges) = graph_obj.get_mut("edges").and_then(|value| value.as_array_mut()) else {
        return;
    };
    edges.retain(|edge| {
        let Some(to) = edge.get("to").and_then(|value| value.as_object()) else {
            return true;
        };
        let Some(node_idx) = to.get("node").and_then(|value| value.as_u64()).map(|value| value as usize) else {
            return true;
        };
        if !host_output_nodes.contains(&node_idx) {
            return true;
        }
        let Some(port) = to.get("port").and_then(|value| value.as_str()) else {
            return false;
        };
        port.trim().eq_ignore_ascii_case(keep_port_lc)
    });
}

pub(super) fn normalize_preview_graph_json_for_runtime(json: &Value, selected_output: &str) -> Value {
    let mut normalized = normalize_graph_json_for_runtime(json);
    let keep_port_lc = selected_output.trim().to_ascii_lowercase();
    if keep_port_lc.is_empty() {
        return normalized;
    }
    let Some(obj) = normalized.as_object_mut() else {
        return normalized;
    };
    prune_unselected_host_output_edges(obj, &keep_port_lc);
    let edges = obj.get("edges").and_then(|value| value.as_array()).cloned().unwrap_or_default();
    if let Some(nodes) = obj.get_mut("nodes").and_then(|value| value.as_array_mut()) {
        prune_disconnected_host_output_ports(nodes, &edges);
    }
    prune_isolated_runtime_nodes(obj);
    normalized
}

pub(super) fn canonicalize_graph_const_inputs(graph: &mut Graph, registry: &daedalus::runtime::plugins::PluginRegistry) {
    fn unwrap_serialized_typed_value(value: &DaedalusValue) -> Option<DaedalusValue> {
        let mut ty: Option<String> = None;
        let mut raw: Option<DaedalusValue> = None;
        match value {
            DaedalusValue::Struct(fields) => {
                for field in fields {
                    match field.name.as_str() {
                        "type" => {
                            if let DaedalusValue::String(kind) = &field.value {
                                ty = Some(kind.to_string());
                            }
                        }
                        "value" => raw = Some(field.value.clone()),
                        _ => {}
                    }
                }
            }
            _ => return None,
        }

        let kind = ty?;
        let raw = raw?;

        match kind.to_ascii_lowercase().as_str() {
            "string" | "str" => match raw {
                DaedalusValue::String(_) => Some(raw),
                _ => None,
            },
            "int" | "integer" => match raw {
                DaedalusValue::Int(_) => Some(raw),
                _ => None,
            },
            "float" | "double" => match raw {
                DaedalusValue::Float(_) => Some(raw),
                DaedalusValue::Int(i) => Some(DaedalusValue::Float(i as f64)),
                _ => None,
            },
            "bool" | "boolean" => match raw {
                DaedalusValue::Bool(_) => Some(raw),
                _ => None,
            },
            _ => Some(raw),
        }
    }

    let view = registry.registry.view();
    for node in &mut graph.nodes {
        let Some(desc) = view.nodes.get(&daedalus::registry::ids::NodeId(node.id.0.clone())) else {
            continue;
        };
        for (port, value) in &mut node.const_inputs {
            if let Some(unwrapped) = unwrap_serialized_typed_value(value) {
                *value = unwrapped;
            }
            let Some(input) = desc.inputs.iter().find(|p| p.name == *port) else {
                continue;
            };
            let variants = match &input.ty {
                DaedalusTypeExpr::Enum(v) => v,
                DaedalusTypeExpr::Optional(inner) => {
                    if let DaedalusTypeExpr::Enum(v) = inner.as_ref() {
                        v
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };

            if matches!(value, DaedalusValue::Int(_)) {
                continue;
            }

            if let DaedalusValue::String(name) = value {
                if let Some((idx, _)) = variants.iter().enumerate().find(|(_, ev)| ev.name.eq_ignore_ascii_case(name.as_ref())) {
                    *value = DaedalusValue::Int(idx as i64);
                }
            }
        }
    }
}

fn canonical_fanin_input_name(name: &str, fanins: &[daedalus::registry::store::FanInPort]) -> Option<String> {
    let name_lc = name.trim().to_ascii_lowercase();
    if name_lc.is_empty() {
        return None;
    }
    for fanin in fanins {
        let prefix_raw = fanin.prefix.trim();
        if prefix_raw.is_empty() {
            continue;
        }
        let prefix_lc = prefix_raw.to_ascii_lowercase();
        let Some(suffix) = name_lc.strip_prefix(&prefix_lc) else { continue };
        let idx = if suffix.is_empty() {
            fanin.start
        } else {
            if !suffix.chars().all(|ch| ch.is_ascii_digit()) {
                continue;
            }
            let Ok(idx) = suffix.parse::<u32>() else { continue };
            if idx < fanin.start {
                continue;
            }
            idx
        };
        return Some(format!("{prefix_raw}{idx}"));
    }
    None
}

fn is_fanin_input_name(name: &str, fanins: &[daedalus::registry::store::FanInPort]) -> bool {
    canonical_fanin_input_name(name, fanins).is_some()
}

pub(super) fn sync_graph_node_port_declarations(graph: &mut Graph, registry: &daedalus::runtime::plugins::PluginRegistry) {
    let view = registry.registry.view();

    for edge in &mut graph.edges {
        let Some(to_node_id) = graph.nodes.get(edge.to.node.0).map(|node| node.id.clone()) else {
            continue;
        };
        let Some(desc) = view.nodes.get(&to_node_id) else {
            continue;
        };
        if let Some(canonical) = canonical_fanin_input_name(&edge.to.port, &desc.fanin_inputs) {
            edge.to.port = canonical;
        }
    }

    for node in &mut graph.nodes {
        let id = node.id.0.as_str();
        if id == "io.host_bridge" || id.ends_with(":io.host_bridge") || id == "io.host_output" || id.ends_with(":io.host_output") {
            continue;
        }

        let Some(desc) = view.nodes.get(&daedalus::registry::ids::NodeId(node.id.0.clone())) else {
            continue;
        };

        let has_dynamic_inputs = desc.metadata.contains_key("dynamic_inputs");
        let has_dynamic_outputs = desc.metadata.contains_key("dynamic_outputs");
        let fixed_inputs: Vec<String> = desc.inputs.iter().map(|p| p.name.clone()).collect();
        let fixed_outputs: Vec<String> = desc.outputs.iter().map(|p| p.name.clone()).collect();

        for input in &mut node.inputs {
            if let Some(canonical) = canonical_fanin_input_name(input, &desc.fanin_inputs) {
                *input = canonical;
            }
        }
        for (name, _) in &mut node.const_inputs {
            if let Some(canonical) = canonical_fanin_input_name(name, &desc.fanin_inputs) {
                *name = canonical;
            }
        }

        if has_dynamic_inputs {
            let mut existing_lc: BTreeSet<String> = node.inputs.iter().map(|name| name.to_ascii_lowercase()).collect();
            for input in &fixed_inputs {
                let key = input.to_ascii_lowercase();
                if existing_lc.insert(key) {
                    node.inputs.push(input.clone());
                }
            }
        } else {
            let mut merged = fixed_inputs.clone();
            let mut merged_lc: BTreeSet<String> = merged.iter().map(|name| name.to_ascii_lowercase()).collect();
            for input in &node.inputs {
                let Some(canonical) = canonical_fanin_input_name(input, &desc.fanin_inputs) else {
                    continue;
                };
                let key = canonical.to_ascii_lowercase();
                if merged_lc.insert(key) {
                    merged.push(canonical);
                }
            }
            node.inputs = merged;
        }

        if has_dynamic_outputs {
            let mut existing_lc: BTreeSet<String> = node.outputs.iter().map(|name| name.to_ascii_lowercase()).collect();
            for output in &fixed_outputs {
                let key = output.to_ascii_lowercase();
                if existing_lc.insert(key) {
                    node.outputs.push(output.clone());
                }
            }
        } else {
            node.outputs = fixed_outputs;
        }

        if !has_dynamic_inputs {
            let fixed_input_lc: BTreeSet<String> = fixed_inputs.iter().map(|name| name.to_ascii_lowercase()).collect();
            node.const_inputs.retain(|(name, _)| fixed_input_lc.contains(&name.to_ascii_lowercase()) || is_fanin_input_name(name, &desc.fanin_inputs));
        }
    }
}

pub(super) fn enforce_registry_default_compute_affinity(graph: &mut Graph, registry: &daedalus::runtime::plugins::PluginRegistry) {
    let view = registry.registry.view();
    for node in &mut graph.nodes {
        let Some(desc) = view.nodes.get(&daedalus::registry::ids::NodeId(node.id.0.clone())) else {
            continue;
        };
        node.compute = desc.default_compute;
    }
}

pub(super) fn infer_host_output_incoming_types(plan: &RuntimePlan, registry: &daedalus::registry::store::Registry) -> BTreeMap<String, BTreeMap<String, daedalus::data::model::TypeExpr>> {
    use daedalus::data::model::TypeExpr;
    use daedalus::registry::ids::NodeId;

    let mut dynamic_outputs: BTreeMap<usize, BTreeMap<String, TypeExpr>> = BTreeMap::new();
    for (idx, node) in plan.nodes.iter().enumerate() {
        let Some(DaedalusValue::Map(items)) = node.metadata.get("dynamic_output_types") else { continue };
        let mut map = BTreeMap::new();
        for (key, value) in items {
            let (DaedalusValue::String(name), DaedalusValue::String(raw)) = (key, value) else { continue };
            let Ok(ty) = serde_json::from_str::<TypeExpr>(raw) else { continue };
            map.insert(name.to_string(), ty);
        }
        if !map.is_empty() {
            dynamic_outputs.insert(idx, map);
        }
    }

    let mut out: BTreeMap<String, BTreeMap<String, TypeExpr>> = BTreeMap::new();
    let view = registry.view();
    for (from, from_port, to, to_port, _edge_policy) in &plan.edges {
        let Some(to_node) = plan.nodes.get(to.0) else { continue };
        if !(to_node.id == "io.host_output" || to_node.id.ends_with(":io.host_output")) {
            continue;
        }

        let ty = if let Some(types) = dynamic_outputs.get(&from.0) {
            let Some(ty) = types.get(from_port) else { continue };
            ty.clone()
        } else {
            let Some(from_node) = plan.nodes.get(from.0) else { continue };
            let node_id = NodeId::new(from_node.id.clone());
            let Some(desc) = view.nodes.get(&node_id) else { continue };
            let Some(port) = desc.outputs.iter().find(|p| p.name.eq_ignore_ascii_case(from_port)) else { continue };
            port.ty.clone()
        };

        let alias = to_node.label.as_deref().unwrap_or(to_node.id.as_str()).to_ascii_lowercase();
        out.entry(alias).or_default().entry(to_port.to_ascii_lowercase()).or_insert_with(|| ty.clone());
    }

    out
}
