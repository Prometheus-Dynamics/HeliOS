use super::*;

pub(super) fn daedalus_value_to_json(value: &daedalus::data::model::Value) -> Option<Value> {
    match value {
        daedalus::data::model::Value::String(s) => serde_json::from_str::<Value>(s).ok().or_else(|| Some(Value::String(s.to_string()))),
        daedalus::data::model::Value::Bytes(bytes) => serde_json::from_slice::<Value>(bytes.as_ref()).ok().or_else(|| Some(Value::String(String::from_utf8_lossy(bytes).to_string()))),
        _ => daedalus_value_to_plain_json(value),
    }
}

pub(super) fn daedalus_value_to_plain_json(value: &daedalus::data::model::Value) -> Option<Value> {
    use daedalus::data::model::Value as RawValue;
    match value {
        RawValue::Unit => Some(Value::Null),
        RawValue::Bool(b) => Some(Value::Bool(*b)),
        RawValue::Int(i) => Some(Value::Number((*i).into())),
        RawValue::Float(f) => serde_json::Number::from_f64(*f).map(Value::Number),
        RawValue::String(s) => Some(Value::String(s.to_string())),
        RawValue::Bytes(bytes) => Some(Value::String(String::from_utf8_lossy(bytes).to_string())),
        RawValue::List(items) | RawValue::Tuple(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(daedalus_value_to_plain_json(item)?);
            }
            Some(Value::Array(out))
        }
        RawValue::Struct(fields) => {
            let mut out = serde_json::Map::new();
            for field in fields {
                out.insert(field.name.clone(), daedalus_value_to_plain_json(&field.value)?);
            }
            Some(Value::Object(out))
        }
        RawValue::Enum(ev) => {
            let mut out = serde_json::Map::new();
            out.insert("name".into(), Value::String(ev.name.clone()));
            if let Some(inner) = ev.value.as_deref() {
                out.insert("value".into(), daedalus_value_to_plain_json(inner)?);
            }
            Some(Value::Object(out))
        }
        RawValue::Map(entries) => {
            let mut obj = serde_json::Map::new();
            let mut arr = Vec::with_capacity(entries.len());
            let mut all_string_keys = true;
            for (key, value) in entries {
                let key_json = daedalus_value_to_plain_json(key)?;
                let value_json = daedalus_value_to_plain_json(value)?;
                if let Value::String(key_str) = key_json {
                    obj.insert(key_str, value_json);
                } else {
                    all_string_keys = false;
                    arr.push(Value::Array(vec![key_json, value_json]));
                }
            }
            if all_string_keys {
                Some(Value::Object(obj))
            } else {
                Some(Value::Array(arr))
            }
        }
    }
}

pub(super) fn sample_cache_metrics(
    image_samples: &Mutex<BTreeMap<String, DynamicImage>>,
    value_samples: &Mutex<BTreeMap<String, DaedalusValue>>,
    typed_samples: &Mutex<BTreeMap<String, TypedHostOutputSample>>,
) -> Option<PipelineSampleCacheMetrics> {
    let image_ports = image_samples.lock().ok().map(|guard| guard.iter().map(|(port, image)| (port.clone(), dynamic_image_size_bytes(image))).collect::<BTreeMap<_, _>>())?;
    let mut value_ports = value_samples.lock().ok().map(|guard| guard.iter().map(|(port, value)| (port.clone(), daedalus_value_size_bytes(value))).collect::<BTreeMap<_, _>>())?;
    let typed_ports = typed_samples.lock().ok().map(|guard| guard.iter().map(|(port, sample)| (port.clone(), sample.size_bytes())).collect::<BTreeMap<_, _>>())?;
    for (port, bytes) in typed_ports {
        value_ports.entry(port).or_insert(bytes);
    }

    let metrics = PipelineSampleCacheMetrics {
        image_sample_count: image_ports.len() as u64,
        image_sample_bytes: image_ports.values().copied().sum(),
        json_sample_count: 0,
        json_sample_bytes: 0,
        value_sample_count: value_ports.len() as u64,
        value_sample_bytes: value_ports.values().copied().sum(),
        image_ports,
        json_ports: BTreeMap::new(),
        value_ports,
    };

    if metrics.image_sample_count == 0 && metrics.value_sample_count == 0 && metrics.image_sample_bytes == 0 && metrics.value_sample_bytes == 0 {
        None
    } else {
        Some(metrics)
    }
}

pub(super) fn annotate_retained_output_metrics(snapshot: &mut PipelineGraphMetrics, owners: &BTreeMap<String, usize>) {
    let Some(sample_cache) = snapshot.sample_cache.as_ref() else {
        return;
    };

    let mut bytes_by_node: BTreeMap<usize, u64> = BTreeMap::new();
    let mut counts_by_node: BTreeMap<usize, u64> = BTreeMap::new();
    let mut ports_by_node: BTreeMap<usize, BTreeMap<String, u64>> = BTreeMap::new();

    let mut record_ports = |ports: &BTreeMap<String, u64>| {
        for (port, bytes) in ports {
            let Some(node_index) = owners.get(&port.to_ascii_lowercase()).copied() else {
                continue;
            };
            *bytes_by_node.entry(node_index).or_default() += *bytes;
            *counts_by_node.entry(node_index).or_default() += 1;
            *ports_by_node.entry(node_index).or_default().entry(port.clone()).or_default() += *bytes;
        }
    };

    record_ports(&sample_cache.image_ports);
    record_ports(&sample_cache.json_ports);
    record_ports(&sample_cache.value_ports);

    for node in snapshot.nodes.values_mut() {
        let Some(node_index) = node.node_index.map(|value| value as usize) else {
            continue;
        };
        let retained_bytes = bytes_by_node.get(&node_index).copied().unwrap_or(0);
        let retained_count = counts_by_node.get(&node_index).copied().unwrap_or(0);
        if retained_bytes == 0 && retained_count == 0 {
            continue;
        }
        node.retained_output_sample_bytes = retained_bytes;
        node.retained_output_sample_count = retained_count;
        node.retained_output_ports = ports_by_node.get(&node_index).cloned();
    }
}

pub(super) fn dynamic_image_size_bytes(image: &DynamicImage) -> u64 {
    u64::from(image.width()).saturating_mul(u64::from(image.height())).saturating_mul(u64::from(image.color().bytes_per_pixel() as u32))
}

pub(super) fn gray_image_size_bytes(image: &GrayImage) -> u64 {
    u64::from(image.width()).saturating_mul(u64::from(image.height()))
}

pub(super) fn json_value_size_bytes(value: &Value) -> u64 {
    serde_json::to_vec(value).map(|bytes| bytes.len() as u64).unwrap_or(0)
}

pub(super) fn daedalus_value_size_bytes(value: &DaedalusValue) -> u64 {
    daedalus_value_to_plain_json(value).map(|json| json_value_size_bytes(&json)).unwrap_or(0)
}

pub(super) fn error_frame_like(image: &DynamicImage, title: &str, detail: Option<&str>) -> DynamicImage {
    let mut out = image.to_rgba8();
    let (width, height) = out.dimensions();
    if width == 0 || height == 0 {
        return DynamicImage::ImageRgba8(out);
    }

    let title = sanitize_error_text(title);
    let detail = detail.map(sanitize_error_text).filter(|s| !s.is_empty());
    let mut lines = vec![title];
    if let Some(detail) = detail {
        lines.push(detail);
    }

    draw_centered_text(&mut out, &lines);
    DynamicImage::ImageRgba8(out)
}

pub(super) fn error_frame(dims: (u32, u32), title: &str, detail: Option<&str>) -> DynamicImage {
    let (width, height) = dims;
    let mut out = RgbaImage::from_pixel(width.max(1), height.max(1), Rgba([0, 0, 0, 255]));
    let mut lines = vec![sanitize_error_text(title)];
    if let Some(detail) = detail.map(sanitize_error_text).filter(|s| !s.is_empty()) {
        lines.push(detail);
    }
    draw_centered_text(&mut out, &lines);
    DynamicImage::ImageRgba8(out)
}

pub(super) fn sanitize_error_text(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        let upper = ch.to_ascii_uppercase();
        if matches!(upper, 'A'..='Z' | '0'..='9' | ' ' | ':' | '-' | '_' | '.' | '/' | '(' | ')') {
            out.push(upper);
        } else if upper.is_whitespace() {
            out.push(' ');
        }
    }
    let trimmed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    trimmed.chars().take(64).collect()
}

pub(super) fn format_panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(msg) = panic.downcast_ref::<&str>() {
        (*msg).to_string()
    } else if let Some(msg) = panic.downcast_ref::<String>() {
        msg.clone()
    } else {
        "unknown panic".to_string()
    }
}

pub(super) fn format_error_detail(err_text: &str, node_type: Option<&str>) -> String {
    if let Some(node) = node_type {
        return format!("NODE: {node}");
    }
    err_text.to_string()
}

pub(super) fn draw_centered_text(image: &mut RgbaImage, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    let (width, height) = image.dimensions();
    let min_dim = width.min(height) as f32;
    let mut scale = ((min_dim * 0.008).ceil() as u32).clamp(1, 6);
    let mut widths = Vec::new();

    let (line_gap, line_height, max_w, _total_h, pad, mut box_w, mut box_h) = loop {
        widths.clear();
        let mut max_w = 0u32;
        for line in lines {
            let w = text_width(line, scale);
            widths.push(w);
            max_w = max_w.max(w);
        }
        let line_gap = scale.saturating_div(2).max(1);
        let line_height = 7 * scale;
        let total_h = (lines.len() as u32 * line_height).saturating_add(line_gap.saturating_mul(lines.len().saturating_sub(1) as u32));
        let pad = scale.saturating_mul(2);
        let box_w = max_w.saturating_add(pad * 2);
        let box_h = total_h.saturating_add(pad * 2);
        if (box_w <= width && box_h <= height) || scale <= 1 {
            break (line_gap, line_height, max_w, total_h, pad, box_w, box_h);
        }
        scale = scale.saturating_sub(1).max(1);
    };
    if box_w > width {
        box_w = width;
    }
    if box_h > height {
        box_h = height;
    }
    let box_x = width.saturating_sub(box_w) / 2;
    let box_y = height.saturating_sub(box_h) / 2;

    fill_rect(image, box_x, box_y, box_w, box_h, Rgba([0, 0, 0, 255]));

    let mut y = box_y + pad;
    for (line, w) in lines.iter().zip(widths) {
        let x = box_x + pad + (max_w.saturating_sub(w) / 2);
        draw_text(image, line, x, y, scale, Rgba([255, 255, 255, 255]));
        y = y.saturating_add(line_height + line_gap);
    }
}

pub(super) fn text_width(text: &str, scale: u32) -> u32 {
    let count = text.chars().count() as u32;
    if count == 0 {
        return 0;
    }
    count.saturating_mul((5 + 1) * scale).saturating_sub(scale)
}

pub(super) fn fill_rect(image: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: Rgba<u8>) {
    let max_x = (x + w).min(image.width());
    let max_y = (y + h).min(image.height());
    for yy in y..max_y {
        for xx in x..max_x {
            image.put_pixel(xx, yy, color);
        }
    }
}

pub(super) fn draw_text(image: &mut RgbaImage, text: &str, x: u32, y: u32, scale: u32, color: Rgba<u8>) {
    let mut cursor_x = x;
    for ch in text.chars() {
        let rows = glyph_rows(ch);
        for (gy, row) in rows.iter().enumerate() {
            for gx in 0..5usize {
                if ((row >> (4 - gx)) & 1) == 0 {
                    continue;
                }
                for sy in 0..scale {
                    for sx in 0..scale {
                        let px = cursor_x + (gx as u32 * scale) + sx;
                        let py = y + (gy as u32 * scale) + sy;
                        if px < image.width() && py < image.height() {
                            image.put_pixel(px, py, color);
                        }
                    }
                }
            }
        }
        cursor_x = cursor_x.saturating_add((5 + 1) * scale);
    }
}

pub(super) fn glyph_rows(c: char) -> [u8; 7] {
    match c {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        ':' => [0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        '_' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00100],
        '/' => [0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b00000, 0b00000],
        '(' => [0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010],
        ')' => [0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000],
        ' ' => [0, 0, 0, 0, 0, 0, 0],
        _ => [0, 0, 0, 0, 0, 0, 0],
    }
}

pub(super) fn infer_host_bridge_image_input_port(plan: &RuntimePlan, host_node_idx: usize, input_ports: &BTreeSet<String>) -> Option<String> {
    let node = plan.nodes.get(host_node_idx)?;
    let mut input_ports_by_lower: BTreeMap<String, String> = BTreeMap::new();
    for port in input_ports {
        input_ports_by_lower.entry(port.to_ascii_lowercase()).or_insert_with(|| port.clone());
    }

    let mut image_ports: BTreeSet<String> = BTreeSet::new();

    // Primary source: solved dynamic output types emitted by planner/runtime.
    if let Some(DaedalusValue::Map(items)) = node.metadata.get("dynamic_output_types") {
        for (key, value) in items {
            let (DaedalusValue::String(port_name), DaedalusValue::String(raw_ty)) = (key, value) else { continue };
            let Some(actual_port) = input_ports_by_lower.get(&port_name.to_ascii_lowercase()) else { continue };
            match serde_json::from_str::<DaedalusTypeExpr>(raw_ty) {
                Ok(ty) => {
                    if is_image_payload(&ty) {
                        image_ports.insert(actual_port.clone());
                    }
                }
                Err(_) => {
                    let raw_lower = raw_ty.to_ascii_lowercase();
                    if raw_lower == "image" || raw_lower.starts_with("image:") {
                        image_ports.insert(actual_port.clone());
                    }
                }
            }
        }
    }

    if image_ports.len() == 1 {
        return image_ports.iter().next().cloned();
    }

    // Deterministic fallback: `frame` is the canonical host camera image port.
    if let Some(frame_port) = input_ports.iter().find(|port| port.eq_ignore_ascii_case("frame")).cloned() {
        if image_ports.is_empty() || image_ports.iter().any(|port| port.eq_ignore_ascii_case("frame")) {
            return Some(frame_port);
        }
    }

    None
}

pub(super) fn derive_host_aliases(plan: &RuntimePlan, host_mgr: &DaedalusBridgeManager) -> Result<(String, String, Vec<String>), GraphError> {
    let host_nodes: Vec<(usize, String)> = plan
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(idx, node)| {
            let is_bridge = matches!(node.metadata.get(HOST_BRIDGE_META_KEY), Some(DaedalusValue::Bool(true)));
            if !is_bridge {
                return None;
            }
            Some((idx, node.label.clone().unwrap_or_else(|| node.id.clone())))
        })
        .collect();
    if host_nodes.is_empty() {
        return Err(GraphError::MissingHostBridge);
    }

    let mut input_alias: Option<String> = None;
    let mut input_node_idx: Option<usize> = None;
    let mut input_ports: BTreeSet<String> = BTreeSet::new();
    let mut output_aliases: Vec<String> = Vec::new();

    // Optional explicit selection: if the graph authors want a specific host output port
    // to be the "stream input", they can set this metadata on the host bridge node.
    const HELIOS_HOST_INPUT_PORT_KEY: &str = "helios.host_input_port";

    let mut explicit_input_port: Option<String> = None;
    for (idx, alias) in &host_nodes {
        let outgoing_ports: BTreeSet<String> = plan.edges.iter().filter(|(from, _, _, _, _)| from.0 == *idx).map(|(_, from_port, _, _, _)| from_port.clone()).collect();
        let has_outgoing = !outgoing_ports.is_empty();
        let has_incoming = plan.edges.iter().any(|(_, _, to, _, _)| to.0 == *idx);
        tracing::debug!(
            host_alias = %alias,
            host_index = *idx,
            has_outgoing,
            has_incoming,
            outgoing_ports = ?outgoing_ports,
            "daedalus graph: host bridge ports"
        );
        if explicit_input_port.is_none() {
            if let Some(DaedalusValue::String(port)) = plan.nodes.get(*idx).and_then(|n| n.metadata.get(HELIOS_HOST_INPUT_PORT_KEY)) {
                let trimmed = port.as_ref().trim();
                if !trimmed.is_empty() {
                    explicit_input_port = Some(trimmed.to_string());
                }
            }
        }
        if has_outgoing && input_alias.is_none() {
            input_alias = Some(alias.clone());
            input_node_idx = Some(*idx);
            input_ports = outgoing_ports;
        }
        if has_incoming {
            output_aliases.push(alias.clone());
        }
    }

    let input_alias = input_alias.ok_or(GraphError::MissingHostBridge)?;
    if host_mgr.handle(&input_alias).is_none() {
        return Err(GraphError::MissingHostBridge);
    }
    let input_port = if let Some(explicit) = explicit_input_port {
        input_ports.iter().find(|p| p.eq_ignore_ascii_case(&explicit)).cloned().ok_or_else(|| {
            let ports: Vec<_> = input_ports.iter().cloned().collect();
            GraphError::Build(format!("graph host bridge input port {:?} not found (ports={ports:?}); set {} to a valid outgoing port", explicit, HELIOS_HOST_INPUT_PORT_KEY))
        })?
    } else if input_ports.len() == 1 {
        input_ports.iter().next().cloned().ok_or_else(|| GraphError::Build("graph host bridge has no outgoing ports".to_string()))?
    } else if let Some(node_idx) = input_node_idx {
        if let Some(inferred) = infer_host_bridge_image_input_port(plan, node_idx, &input_ports) {
            tracing::debug!(input_host = %input_alias, input_port = %inferred, "daedalus graph: resolved host input by inferred port type");
            inferred
        } else {
            let ports: Vec<_> = input_ports.iter().cloned().collect();
            return Err(GraphError::Build(format!("graph host bridge input port is ambiguous (ports={ports:?}); set {} metadata on the host bridge node", HELIOS_HOST_INPUT_PORT_KEY)));
        }
    } else {
        let ports: Vec<_> = input_ports.iter().cloned().collect();
        return Err(GraphError::Build(format!("graph host bridge input port is ambiguous (ports={ports:?}); set {} metadata on the host bridge node", HELIOS_HOST_INPUT_PORT_KEY)));
    };
    tracing::debug!(input_host = %input_alias, input_port = %input_port, "daedalus graph: resolved host input");

    if output_aliases.is_empty() {
        return Err(GraphError::MissingHostBridge);
    }

    Ok((input_alias, input_port, output_aliases))
}

pub(super) fn host_output_sink_node_index(plan: &RuntimePlan, output_hosts: &[String]) -> Option<usize> {
    let mut incoming = vec![false; plan.nodes.len()];
    for (_, _, to, _, _) in &plan.edges {
        if to.0 < incoming.len() {
            incoming[to.0] = true;
        }
    }

    for alias in output_hosts {
        if let Some((idx, _)) = plan.nodes.iter().enumerate().find(|(idx, node)| {
            incoming.get(*idx).copied().unwrap_or(false)
                && (node.id == "io.host_output" || node.id.ends_with(":io.host_output"))
                && node.label.as_deref().is_some_and(|label| label.eq_ignore_ascii_case(alias))
        }) {
            return Some(idx);
        }
    }

    plan.nodes.iter().enumerate().find_map(|(idx, node)| {
        if !incoming.get(idx).copied().unwrap_or(false) {
            return None;
        }
        if node.id == "io.host_output" || node.id.ends_with(":io.host_output") {
            Some(idx)
        } else {
            None
        }
    })
}

pub(super) fn infer_host_output_port_owners(plan: &RuntimePlan, output_hosts: &[String]) -> BTreeMap<String, usize> {
    let mut owners = BTreeMap::new();
    for (from, _from_port, to, to_port, _) in &plan.edges {
        let _ = from;
        let Some(to_node) = plan.nodes.get(to.0) else {
            continue;
        };
        if !(to_node.id == "io.host_output" || to_node.id.ends_with(":io.host_output")) {
            continue;
        }
        if !output_hosts.is_empty() && !output_hosts.iter().any(|alias| to_node.label.as_deref().is_some_and(|label| label.eq_ignore_ascii_case(alias))) {
            continue;
        }
        let key = to_port.to_ascii_lowercase();
        // Demand-driven masking must target the host-output sink and preserve the selected input
        // port. Daedalus will then walk only that port's upstream closure. Pointing this at the
        // producer node skips `io.host_output` entirely, which makes preview publication disappear.
        owners.entry(key).or_insert(to.0);
    }
    owners
}

pub(super) fn build_demand_mask(
    plan: &RuntimePlan,
    output_hosts: &[String],
    preview_ports: &[String],
    host_output_ports: &[String],
    host_output_port_types: &BTreeMap<String, DaedalusTypeExpr>,
    host_output_port_owners: &BTreeMap<String, usize>,
    demand_driven: bool,
    include_preview_ports: bool,
    include_value_ports: bool,
    extra_sink_ports: &[String],
) -> Option<Vec<bool>> {
    if !demand_driven {
        return None;
    }

    let fallback_selector = if let Some(index) = host_output_sink_node_index(plan, output_hosts) {
        daedalus::planner::GraphNodeSelector { index: Some(index), id: None, metadata: None }
    } else {
        daedalus::planner::GraphNodeSelector { index: None, id: Some("io.host_output".to_string()), metadata: None }
    };

    let mut sink_ports: BTreeSet<String> = BTreeSet::new();
    if include_preview_ports {
        for port in preview_ports {
            if !port.trim().is_empty() {
                sink_ports.insert(port.clone());
            }
        }
    }
    if include_value_ports {
        // Demand-driven execution still needs non-preview host outputs (for example `detections`)
        // so sampling and calibration solve can read value ports from the same graph tick.
        for port in host_output_ports {
            if port.trim().is_empty() {
                continue;
            }
            let key = port.to_ascii_lowercase();
            if sink_ports.contains(port) {
                continue;
            }
            let is_image = host_output_port_types.get(&key).map(is_image_payload).unwrap_or(false);
            if !is_image {
                sink_ports.insert(port.clone());
            }
        }
    }
    for port in extra_sink_ports {
        if !port.trim().is_empty() {
            sink_ports.insert(port.clone());
        }
    }

    let mut sinks = Vec::new();
    for port in sink_ports {
        let key = port.to_ascii_lowercase();
        let selector =
            host_output_port_owners.get(&key).copied().map(|index| daedalus::planner::GraphNodeSelector { index: Some(index), id: None, metadata: None }).unwrap_or_else(|| fallback_selector.clone());
        sinks.push(RuntimeSink { node: selector, port: Some(port) });
    }
    if sinks.is_empty() {
        None
    } else {
        plan.active_nodes_for_sinks(&sinks).ok()
    }
}

#[cfg(test)]
pub(super) fn build_demand_sinks(
    plan: &RuntimePlan,
    output_hosts: &[String],
    preview_ports: &[String],
    host_output_ports: &[String],
    host_output_port_types: &BTreeMap<String, DaedalusTypeExpr>,
    host_output_port_owners: &BTreeMap<String, usize>,
    demand_driven: bool,
    extra_sink_ports: &[String],
) -> Vec<RuntimeSink> {
    if !demand_driven {
        return Vec::new();
    }

    let fallback_selector = if let Some(index) = host_output_sink_node_index(plan, output_hosts) {
        daedalus::planner::GraphNodeSelector { index: Some(index), id: None, metadata: None }
    } else {
        daedalus::planner::GraphNodeSelector { index: None, id: Some("io.host_output".to_string()), metadata: None }
    };

    let mut sink_ports: BTreeSet<String> = BTreeSet::new();
    for port in preview_ports {
        if !port.trim().is_empty() {
            sink_ports.insert(port.clone());
        }
    }
    for port in host_output_ports {
        if port.trim().is_empty() {
            continue;
        }
        let key = port.to_ascii_lowercase();
        if sink_ports.contains(port) {
            continue;
        }
        let is_image = host_output_port_types.get(&key).map(is_image_payload).unwrap_or(false);
        if !is_image {
            sink_ports.insert(port.clone());
        }
    }
    for port in extra_sink_ports {
        if !port.trim().is_empty() {
            sink_ports.insert(port.clone());
        }
    }

    sink_ports
        .into_iter()
        .map(|port| {
            let key = port.to_ascii_lowercase();
            let selector = host_output_port_owners
                .get(&key)
                .copied()
                .map(|index| daedalus::planner::GraphNodeSelector { index: Some(index), id: None, metadata: None })
                .unwrap_or_else(|| fallback_selector.clone());
            RuntimeSink { node: selector, port: Some(port) }
        })
        .collect()
}

pub(super) fn plan_uses_gpu(plan: &RuntimePlan) -> bool {
    plan.segments.iter().any(|segment| !matches!(segment.compute, ComputeAffinity::CpuOnly))
}

pub(super) fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub(super) fn extract_node_type(error: &str) -> Option<String> {
    let needle = "node: \"";
    let start = error.find(needle)? + needle.len();
    let rest = &error[start..];
    let end = rest.find('"')?;
    let node = rest[..end].trim();
    if node.is_empty() {
        None
    } else {
        Some(node.to_string())
    }
}
