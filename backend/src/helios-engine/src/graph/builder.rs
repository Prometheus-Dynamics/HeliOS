use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use uuid::Uuid;

use crate::ipc::{ResolvedStreamConfig, StreamPipelineBinding, StreamPipelineGridSlot};
use crate::pipelines;
use daedalus::planner::Graph;
use daedalus::planner::GraphPatch;

use super::context;
use super::multiplex::{MultiplexGraphExecutor, MultiplexPipeline};
use super::{GraphError, GraphExecutor, GraphHandle, HostBridgeHandle};

const MULTIPLEX_DIMENSION_MAX: u32 = 6;
const RAW_STREAM_PIPELINE_UUID: Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_0000000000aa);

fn clamp_dimension(value: u8) -> u32 {
    u32::from(value).clamp(1, MULTIPLEX_DIMENSION_MAX)
}

fn layout_from_manifest(manifest: &ResolvedStreamConfig) -> Option<(u32, u32, Vec<StreamPipelineGridSlot>)> {
    let layout = manifest.pipeline_layout.as_ref()?;
    let rows = clamp_dimension(layout.rows);
    let columns = clamp_dimension(layout.columns);
    Some((rows, columns, layout.slots.clone()))
}

fn layout_active_pipeline_id(rows: u32, cols: u32, slots: &[StreamPipelineGridSlot]) -> Option<Uuid> {
    if rows != 1 || cols != 1 {
        return None;
    }
    slots.iter().find(|slot| slot.row == 0 && slot.column == 0).and_then(|slot| slot.pipeline_id)
}

fn auto_layout_for_pipelines(pipelines: &[StreamPipelineBinding]) -> (u32, u32, Vec<StreamPipelineGridSlot>) {
    let target = pipelines.len().min((MULTIPLEX_DIMENSION_MAX * MULTIPLEX_DIMENSION_MAX) as usize);
    if target == 0 {
        return (1, 1, Vec::new());
    }
    let size = ((target as f64).sqrt().ceil() as u32).clamp(1, MULTIPLEX_DIMENSION_MAX);
    let mut slots = Vec::with_capacity(size.saturating_mul(size) as usize);
    for (index, pipeline) in pipelines.iter().take((size * size) as usize).enumerate() {
        let row = (index as u32) / size;
        let col = (index as u32) % size;
        slots.push(StreamPipelineGridSlot { row: row as u8, column: col as u8, pipeline_id: Some(pipeline.pipeline_id), output_key: None });
    }
    (size, size, slots)
}

fn active_pipeline_id(manifest: &ResolvedStreamConfig, _rows: u32, _cols: u32, _slots: &[StreamPipelineGridSlot]) -> Option<Uuid> {
    manifest.active_pipeline_id
}

fn output_override_for_active<'a>(manifest: &'a ResolvedStreamConfig, override_active_output: Option<&'a str>) -> Option<&'a str> {
    override_active_output.or(manifest.active_pipeline_output.as_deref())
}

fn nonempty_trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

fn raw_binding_output(manifest: &ResolvedStreamConfig) -> Option<&str> {
    manifest.pipelines.iter().find(|p| p.pipeline_id == RAW_STREAM_PIPELINE_UUID).and_then(|p| nonempty_trimmed(p.pipeline_output.as_deref()))
}

fn raw_slot_output(slots: &[StreamPipelineGridSlot]) -> Option<&str> {
    slots
        .iter()
        .find(|slot| slot.row == 0 && slot.column == 0 && slot.pipeline_id == Some(RAW_STREAM_PIPELINE_UUID))
        .and_then(|slot| nonempty_trimmed(slot.output_key.as_deref()))
        .or_else(|| slots.iter().find(|slot| slot.pipeline_id == Some(RAW_STREAM_PIPELINE_UUID)).and_then(|slot| nonempty_trimmed(slot.output_key.as_deref())))
}

fn resolve_raw_output_for_active<'a>(manifest: &'a ResolvedStreamConfig, slots: &'a [StreamPipelineGridSlot], override_active_output: Option<&'a str>) -> Option<&'a str> {
    // In 1x1 layout mode, slot (0,0) is the rendered view source. Prefer the slot output key
    // over global active output overrides so stale active-pipeline state cannot force RAW view
    // back to `raw` when the slot explicitly requests `undistorted`.
    raw_slot_output(slots).or_else(|| nonempty_trimmed(override_active_output)).or_else(|| nonempty_trimmed(manifest.active_pipeline_output.as_deref())).or_else(|| raw_binding_output(manifest))
}

fn resolve_graph_json(binding: &StreamPipelineBinding) -> Result<Value, GraphError> {
    // RAW stream pipeline is reserved/system-owned: always use the built-in graph so the RAW
    // stream shape is fully controlled by the engine (not persisted user graphs).
    if binding.pipeline_id == RAW_STREAM_PIPELINE_UUID {
        return Ok(builtin_raw_stream_graph_json());
    }
    let mut graph_json = if let Some(graph) = binding.pipeline_graph.as_ref() {
        graph.as_value().clone()
    } else {
        pipelines::load_pipeline_graph_json(binding.pipeline_id).map_err(|err| GraphError::Build(format!("pipeline {} missing graph payload: {err}", binding.pipeline_id)))?
    };

    if let Some(patch_wire) = binding.pipeline_patch.as_ref() {
        let patch: GraphPatch = serde_json::from_value(patch_wire.as_value().clone()).map_err(|err| GraphError::Build(format!("pipeline {} patch invalid: {err}", binding.pipeline_id)))?;
        let mut graph: Graph = serde_json::from_value(graph_json.clone()).map_err(|err| GraphError::Build(format!("pipeline {} graph invalid: {err}", binding.pipeline_id)))?;
        patch.apply_to_graph(&mut graph);
        graph_json = serde_json::to_value(graph).map_err(|err| GraphError::Build(format!("pipeline {} patch encode failed: {err}", binding.pipeline_id)))?;
    }

    Ok(graph_json)
}

fn builtin_raw_stream_graph_json_variant(include_undistort: bool) -> Value {
    // Built-in graph used for the reserved RAW stream pipeline UUID.
    //
    // This must not rely on any on-disk pipeline graph payload since the RAW pipeline is a
    // virtual/system pipeline rather than a user-managed graph document.
    if include_undistort {
        return serde_json::json!({
            "nodes": [
                {
                    "id": "io.host_bridge",
                    "label": "Input:frame+calibration",
                    "inputs": [],
                    "outputs": ["frame", "calibration"],
                    "metadata": {
                        "host_bridge": { "type": "Bool", "value": true },
                        "helios.host_input_port": { "type": "String", "value": "frame" }
                    }
                },
                {
                    "id": "cv:image:undistort_optional",
                    "label": "Undistort (calibration)",
                    "inputs": ["frame", "calibration", "border_mode", "zoom_mode", "zoom", "fill_margin"],
                    "outputs": ["frame"],
                    "const_inputs": [
                        ["border_mode", { "type": "String", "value": "zero" }],
                        ["zoom_mode", { "type": "String", "value": "fill" }],
                        ["zoom", { "type": "Float", "value": 1.0 }],
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
            ],
            "metadata": {
                "helios.pipeline.alias": "Raw stream"
            }
        });
    }

    // Faster RAW passthrough variant: no undistortion node at all.
    serde_json::json!({
        "nodes": [
            {
                "id": "io.host_bridge",
                "label": "Input:frame",
                "inputs": [],
                "outputs": ["frame"],
                "metadata": {
                    "host_bridge": { "type": "Bool", "value": true },
                    "helios.host_input_port": { "type": "String", "value": "frame" }
                }
            },
            {
                "id": "io.host_output",
                "label": "Output:raw",
                "inputs": ["frame", "raw"],
                "outputs": [],
                "metadata": { "host_bridge": { "type": "Bool", "value": true } }
            }
        ],
        "edges": [
            { "from": { "node": 0, "port": "frame" }, "to": { "node": 1, "port": "frame" } },
            { "from": { "node": 0, "port": "frame" }, "to": { "node": 1, "port": "raw" } }
        ],
        "metadata": {
            "helios.pipeline.alias": "Raw stream"
        }
    })
}

fn builtin_raw_stream_graph_json() -> Value {
    builtin_raw_stream_graph_json_variant(true)
}

fn build_builtin_raw_stream_graph_handle(host_buffer: usize, manifest: &ResolvedStreamConfig, output_port: Option<&str>) -> Result<GraphHandle, GraphError> {
    // RAW stream outputs are explicitly `raw` (fast path) or `undistorted` (requires undistort node).
    // Keep `frame` as compatibility alias and coerce stale/invalid values back to `raw`.
    let canonical = normalize_raw_output_key(output_port).unwrap_or_else(|| "raw".to_string());
    // Keep `raw` as the public/raw-stream selector while routing runtime selection through
    // `frame` so solved image typing remains stable across cold starts.
    let runtime_output = match canonical.as_str() {
        "raw" => Some("frame"),
        "undistorted" => Some("undistorted"),
        _ => Some("frame"),
    };
    let pipeline_id = RAW_STREAM_PIPELINE_UUID;
    let wants_undistorted = canonical.eq_ignore_ascii_case("undistorted");
    let include_undistort = wants_undistorted;

    let graph_json = builtin_raw_stream_graph_json_variant(include_undistort);
    let stream_alias = stream_alias_from_manifest(manifest);
    let pipeline_alias = context::pipeline_alias_from_graph(&graph_json, &pipeline_id.to_string());
    let mut graph_json_for_build = graph_json;
    context::inject_node_context(&mut graph_json_for_build, &stream_alias, &pipeline_alias);

    let graph = GraphHandle::from_json_with_output(host_buffer, &graph_json_for_build, runtime_output)?;
    graph.set_calibration(manifest.calibration.clone());
    apply_manifest_host_inputs(&graph, manifest, Some(pipeline_id));
    Ok(graph)
}

fn stream_alias_from_manifest(manifest: &ResolvedStreamConfig) -> String {
    let fallback = "stream";
    if let Some(alias) = manifest.identity.alias.as_deref() {
        return context::sanitize_segment(alias, fallback);
    }
    if let Some(hw) = manifest.identity.hardware_id.as_deref() {
        return context::sanitize_segment(hw, fallback);
    }
    if let Some(id) = manifest.identity.id {
        return id.to_string();
    }
    fallback.to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PipelineInstanceKey {
    pipeline_id: Uuid,
    output_key: Option<String>,
}

fn normalize_output_key(key: Option<&str>) -> Option<String> {
    key.and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn manifest_host_inputs_for_runtime(manifest: &ResolvedStreamConfig) -> BTreeMap<String, Option<Value>> {
    let mut out: BTreeMap<String, Option<Value>> = BTreeMap::new();
    for (raw_key, raw_value) in &manifest.pipeline_host_inputs {
        let key = raw_key.trim().to_ascii_lowercase();
        if key.is_empty() {
            continue;
        }
        out.insert(key, Some(raw_value.as_value().clone()));
    }
    out
}

fn apply_manifest_host_inputs(graph: &GraphHandle, manifest: &ResolvedStreamConfig, pipeline_id: Option<Uuid>) {
    let inputs = manifest_host_inputs_for_runtime(manifest);
    if inputs.is_empty() {
        return;
    }
    graph.set_pipeline_inputs(pipeline_id, &inputs);
}

fn normalize_raw_output_key(key: Option<&str>) -> Option<String> {
    normalize_output_key(key).map(|value| {
        if value.eq_ignore_ascii_case("undistorted") {
            "undistorted".to_string()
        } else {
            // RAW pipeline only supports `raw` and `undistorted`.
            // Treat legacy/invalid values (e.g. `frame`, `overlay`) as `raw`.
            "raw".to_string()
        }
    })
}

fn wire_source_instance_key(endpoint: &crate::ipc::StreamPipelineEndpoint) -> Option<String> {
    if endpoint.pipeline_id != RAW_STREAM_PIPELINE_UUID {
        return normalize_output_key(endpoint.output_key.as_deref());
    }

    if let Some(key) = normalize_raw_output_key(endpoint.output_key.as_deref()) {
        return Some(key);
    }

    match normalize_raw_output_key(endpoint.port.as_deref()) {
        Some(port) if matches!(port.as_str(), "raw" | "undistorted") => Some(port),
        _ => None,
    }
}

fn slot_output_key(slot: &StreamPipelineGridSlot) -> Option<String> {
    normalize_output_key(slot.output_key.as_deref())
}

pub(crate) fn build_graph_handle_for_manifest(host_buffer: usize, manifest: &ResolvedStreamConfig, override_active_output: Option<&str>) -> Result<GraphHandle, GraphError> {
    if !manifest.pipeline_enabled {
        return Ok(GraphHandle::with_default_host(host_buffer));
    }

    let (rows, cols, slots) = layout_from_manifest(manifest).unwrap_or_else(|| auto_layout_for_pipelines(&manifest.pipelines));
    let wires = manifest.pipeline_wires.clone();

    let multiplex_enabled = rows.saturating_mul(cols) > 1 || manifest.pipelines.len() > 1 || !wires.is_empty();

    if manifest.pipelines.is_empty() {
        let slots_only_raw = slots.iter().all(|slot| slot.pipeline_id.is_none() || slot.pipeline_id == Some(RAW_STREAM_PIPELINE_UUID));
        if slots_only_raw {
            if !multiplex_enabled {
                let output = resolve_raw_output_for_active(manifest, &slots, override_active_output);
                return build_builtin_raw_stream_graph_handle(host_buffer, manifest, output);
            }

            let slot_output = slots.iter().find(|slot| slot.row == 0 && slot.column == 0 && slot.pipeline_id == Some(RAW_STREAM_PIPELINE_UUID)).and_then(|slot| slot.output_key.as_deref());
            let output = slot_output.or(output_override_for_active(manifest, override_active_output));
            let output_port = normalize_raw_output_key(output);
            let raw_graph = build_builtin_raw_stream_graph_handle(host_buffer, manifest, output)?;
            let pipelines = vec![MultiplexPipeline { pipeline_id: RAW_STREAM_PIPELINE_UUID, output_key: output_port.clone(), output_port, graph: raw_graph }];

            let mut cells: Vec<Option<usize>> = vec![None; (rows * cols) as usize];
            for slot in &slots {
                let row = u32::from(slot.row);
                let col = u32::from(slot.column);
                if row >= rows || col >= cols {
                    continue;
                }
                if slot.pipeline_id == Some(RAW_STREAM_PIPELINE_UUID) {
                    let cell_idx = (row * cols + col) as usize;
                    if let Some(cell) = cells.get_mut(cell_idx) {
                        *cell = Some(0);
                    }
                }
            }

            let (host, rx) = HostBridgeHandle::new(host_buffer);
            let _ = rx;
            let exec = MultiplexGraphExecutor::new(pipelines, rows, cols, cells, 0, false, wires);
            exec.set_calibration(manifest.calibration.clone());
            let graph = GraphHandle::with_executor(host, Arc::new(exec));
            apply_manifest_host_inputs(&graph, manifest, Some(RAW_STREAM_PIPELINE_UUID));
            return Ok(graph);
        }

        return Err(GraphError::Build("stream manifest has pipelines enabled but no pipelines are bound".to_string()));
    }

    if !multiplex_enabled {
        let explicit_empty_layout = manifest.pipeline_layout.is_some() && slots.iter().all(|slot| slot.pipeline_id.is_none());
        if explicit_empty_layout {
            // Preserve an explicit "cleared" 1x1 layout as blank output instead of resurrecting
            // an implicit active/first pipeline.
            let (host, rx) = HostBridgeHandle::new(host_buffer);
            let _ = rx;
            let cells: Vec<Option<usize>> = vec![None; (rows * cols) as usize];
            let exec = MultiplexGraphExecutor::new(Vec::new(), rows, cols, cells, 0, false, wires);
            exec.set_calibration(manifest.calibration.clone());
            let graph = GraphHandle::with_executor(host, Arc::new(exec));
            apply_manifest_host_inputs(&graph, manifest, manifest.active_pipeline_id);
            return Ok(graph);
        }

        let active_id = layout_active_pipeline_id(rows, cols, &slots).or_else(|| active_pipeline_id(manifest, rows, cols, &slots)).or_else(|| manifest.pipelines.first().map(|p| p.pipeline_id));
        if let Some(active_id) = active_id {
            // RAW stream pipeline is a reserved/system pipeline. Always build it via the built-in
            // constructor so we can select an optimized variant (raw passthrough vs undistorted)
            // based on the selected output port and wiring.
            if active_id == RAW_STREAM_PIPELINE_UUID {
                let output = resolve_raw_output_for_active(manifest, &slots, override_active_output);
                return build_builtin_raw_stream_graph_handle(host_buffer, manifest, output);
            }
            if let Some(binding) = manifest.pipelines.iter().find(|p| p.pipeline_id == active_id) {
                let graph_json = resolve_graph_json(binding)?;
                let output = output_override_for_active(manifest, override_active_output);
                let stream_alias = stream_alias_from_manifest(manifest);
                let pipeline_alias = context::pipeline_alias_from_graph(&graph_json, &active_id.to_string());
                let mut graph_json_for_build = graph_json;
                context::inject_node_context(&mut graph_json_for_build, &stream_alias, &pipeline_alias);
                let graph = GraphHandle::from_json_with_output(host_buffer, &graph_json_for_build, output)?;
                graph.set_calibration(manifest.calibration.clone());
                apply_manifest_host_inputs(&graph, manifest, Some(active_id));
                return Ok(graph);
            }
        }
        return Err(GraphError::Build("stream manifest references an active pipeline id that is missing from `pipelines`".to_string()));
    }

    let mut requested: BTreeSet<PipelineInstanceKey> = BTreeSet::new();
    let mut requested_ids: BTreeSet<Uuid> = BTreeSet::new();
    for slot in &slots {
        let Some(id) = slot.pipeline_id else { continue };
        requested.insert(PipelineInstanceKey { pipeline_id: id, output_key: slot_output_key(slot) });
        requested_ids.insert(id);
    }

    // Ensure pipelines referenced by wiring are constructed even if they're not in the grid.
    for wire in &wires {
        let from_key = PipelineInstanceKey { pipeline_id: wire.from.pipeline_id, output_key: wire_source_instance_key(&wire.from) };
        requested.insert(from_key.clone());
        requested_ids.insert(from_key.pipeline_id);

        let to_key = PipelineInstanceKey { pipeline_id: wire.to.pipeline_id, output_key: normalize_output_key(wire.to.output_key.as_deref()) };
        requested.insert(to_key.clone());
        requested_ids.insert(to_key.pipeline_id);
    }

    // In multiplex mode, an explicit empty layout (no populated slots/wires) should stay empty.
    // Only honor active selection if that pipeline is referenced by layout slots or wiring.
    let active_id = active_pipeline_id(manifest, rows, cols, &slots).filter(|id| requested_ids.contains(id));
    let active_output_override = normalize_output_key(output_override_for_active(manifest, override_active_output));
    if let Some(active_id) = active_id {
        if manifest.pipelines.iter().any(|p| p.pipeline_id == active_id) {
            requested.insert(PipelineInstanceKey { pipeline_id: active_id, output_key: active_output_override.clone() });
            requested_ids.insert(active_id);
        }
    }

    // Pipelines that are neither in layout, active selection, nor referenced by wiring should be
    // treated as inactive bindings. Building/running them here can unexpectedly consume CPU/GPU
    // and skew metrics for the selected pipeline.
    let process_all_pipelines = false;

    let mut pipelines: Vec<MultiplexPipeline> = Vec::new();
    for key in requested.iter().cloned() {
        if key.pipeline_id == RAW_STREAM_PIPELINE_UUID {
            // RAW stream pipeline: always use the built-in graph builder so we can omit
            // undistortion when the selected output does not require it.
            let binding_output = manifest.pipelines.iter().find(|p| p.pipeline_id == RAW_STREAM_PIPELINE_UUID).and_then(|p| p.pipeline_output.as_deref());

            let mut output = key.output_key.as_deref().or(binding_output);
            if Some(key.pipeline_id) == active_id && key.output_key == active_output_override {
                output = active_output_override.as_deref();
            }

            let output_port = normalize_raw_output_key(output);
            let graph = build_builtin_raw_stream_graph_handle(host_buffer, manifest, output)?;
            pipelines.push(MultiplexPipeline { pipeline_id: RAW_STREAM_PIPELINE_UUID, output_key: key.output_key.clone(), output_port, graph });
            continue;
        }

        let binding = manifest.pipelines.iter().find(|p| p.pipeline_id == key.pipeline_id);
        let Some(binding) = binding else {
            return Err(GraphError::Build(format!("pipeline {} referenced by layout but missing from manifest", key.pipeline_id)));
        };
        let graph_json = resolve_graph_json(binding)?;

        // Slot-level `output_key` selects which host-output port is forwarded for this pipeline
        // instance. If unset, fall back to the binding's default output.
        let mut output = key.output_key.as_deref().or(binding.pipeline_output.as_deref());
        // The active pipeline output override is used for the active pipeline instance only.
        if Some(key.pipeline_id) == active_id && key.output_key == active_output_override {
            output = active_output_override.as_deref();
        }

        let stream_alias = stream_alias_from_manifest(manifest);
        let pipeline_alias = context::pipeline_alias_from_graph(&graph_json, &key.pipeline_id.to_string());
        let mut graph_json_for_build = graph_json;
        context::inject_node_context(&mut graph_json_for_build, &stream_alias, &pipeline_alias);
        let graph = GraphHandle::from_json_with_output(host_buffer, &graph_json_for_build, output)?;
        graph.set_calibration(manifest.calibration.clone());
        let output_port = output.map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
        pipelines.push(MultiplexPipeline { pipeline_id: key.pipeline_id, output_key: key.output_key.clone(), output_port, graph });
    }

    let mut index_by_key: BTreeMap<PipelineInstanceKey, usize> = BTreeMap::new();
    for (idx, pipeline) in pipelines.iter().enumerate() {
        index_by_key.insert(PipelineInstanceKey { pipeline_id: pipeline.pipeline_id, output_key: pipeline.output_key.clone() }, idx);
    }

    let mut cells: Vec<Option<usize>> = vec![None; (rows * cols) as usize];
    for slot in &slots {
        let row = u32::from(slot.row);
        let col = u32::from(slot.column);
        if row >= rows || col >= cols {
            continue;
        }
        let Some(pipeline_id) = slot.pipeline_id else { continue };
        let key = PipelineInstanceKey { pipeline_id, output_key: slot_output_key(slot) };
        let Some(&pipeline_idx) = index_by_key.get(&key) else { continue };
        let cell_idx = (row * cols + col) as usize;
        if let Some(cell) = cells.get_mut(cell_idx) {
            *cell = Some(pipeline_idx);
        }
    }

    let active_pipeline_index = active_id
        .and_then(|pipeline_id| {
            let key = PipelineInstanceKey { pipeline_id, output_key: active_output_override.clone() };
            index_by_key.get(&key).copied()
        })
        .or_else(|| index_by_key.values().next().copied())
        .unwrap_or(0);

    let (host, rx) = HostBridgeHandle::new(host_buffer);
    let _ = rx;
    let exec = MultiplexGraphExecutor::new(pipelines, rows, cols, cells, active_pipeline_index, process_all_pipelines, wires);
    exec.set_calibration(manifest.calibration.clone());
    let graph = GraphHandle::with_executor(host, Arc::new(exec));
    apply_manifest_host_inputs(&graph, manifest, active_id);
    Ok(graph)
}

pub(crate) fn build_graph_handle_for_pipeline_output(host_buffer: usize, manifest: &ResolvedStreamConfig, pipeline_id: Uuid, output_key: Option<&str>) -> Result<GraphHandle, GraphError> {
    if pipeline_id == RAW_STREAM_PIPELINE_UUID {
        let binding_output = manifest.pipelines.iter().find(|p| p.pipeline_id == RAW_STREAM_PIPELINE_UUID).and_then(|p| p.pipeline_output.as_deref());
        let output = output_key.or(binding_output);
        return build_builtin_raw_stream_graph_handle(host_buffer, manifest, output);
    }

    let binding = manifest.pipelines.iter().find(|p| p.pipeline_id == pipeline_id);
    let Some(binding) = binding else {
        return Err(GraphError::Build(format!("pipeline {} missing from manifest", pipeline_id)));
    };
    let graph_json = resolve_graph_json(binding)?;
    let output = output_key.or(binding.pipeline_output.as_deref());
    let stream_alias = stream_alias_from_manifest(manifest);
    let pipeline_alias = context::pipeline_alias_from_graph(&graph_json, &pipeline_id.to_string());
    let mut graph_json_for_build = graph_json;
    context::inject_node_context(&mut graph_json_for_build, &stream_alias, &pipeline_alias);
    let graph = GraphHandle::from_json_with_output(host_buffer, &graph_json_for_build, output)?;
    graph.set_calibration(manifest.calibration.clone());
    apply_manifest_host_inputs(&graph, manifest, Some(pipeline_id));
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::{RequestedDecoderConfig, RequestedEncoderConfig, StreamManifest};
    use styx::prelude::{ColorSpace, FourCc, MediaFormat, Resolution};

    fn sample_manifest() -> ResolvedStreamConfig {
        let identity = crate::identity::DeviceIdentity { id: None, alias: None, hardware_id: None };
        let fmt = MediaFormat::new(FourCc::new(*b"RGB3"), Resolution::new(1, 1).expect("resolution"), ColorSpace::Srgb);
        let capture = crate::capture::CaptureConfig {
            device_keys: vec![],
            backend: crate::capture::BackendKind::Virtual,
            handle: crate::capture::BackendHandle::Virtual,
            mode: crate::capture::ModeId { format: fmt, interval: None },
            target_fps: None,
            interval: None,
            controls: vec![],
            enable_tdn_output: false,
        };
        StreamManifest {
            identity,
            capture,
            host_buffer: 1,
            internal: false,
            pipeline_enabled: true,
            pipelines: vec![StreamPipelineBinding { pipeline_id: RAW_STREAM_PIPELINE_UUID, pipeline_graph: None, pipeline_output: Some("raw".to_string()), pipeline_patch: None }],
            active_pipeline_id: Some(RAW_STREAM_PIPELINE_UUID),
            active_pipeline_output: Some("raw".to_string()),
            pipeline_layout: None,
            pipeline_wires: Vec::new(),
            pipeline_host_inputs: BTreeMap::new(),
            calibration: None,
            pose: None,
            encoder: RequestedEncoderConfig::default(),
            decoder: RequestedDecoderConfig::default(),
            preview_jpeg_quality: 30,
            shadow_recorder_enabled: true,
            start_on_boot: false,
        }
        .resolve()
    }

    #[test]
    fn resolve_raw_output_prefers_slot_key_over_override() {
        let manifest = sample_manifest();
        let slots = vec![StreamPipelineGridSlot { row: 0, column: 0, pipeline_id: Some(RAW_STREAM_PIPELINE_UUID), output_key: Some("undistorted".to_string()) }];

        let resolved = resolve_raw_output_for_active(&manifest, &slots, Some("raw"));
        assert_eq!(resolved, Some("undistorted"));
    }

    #[test]
    fn resolve_raw_output_uses_override_when_slot_missing() {
        let manifest = sample_manifest();
        let slots = vec![StreamPipelineGridSlot { row: 0, column: 0, pipeline_id: Some(RAW_STREAM_PIPELINE_UUID), output_key: None }];

        let resolved = resolve_raw_output_for_active(&manifest, &slots, Some("undistorted"));
        assert_eq!(resolved, Some("undistorted"));
    }

    #[test]
    fn normalize_raw_output_key_coerces_invalid_to_raw() {
        assert_eq!(normalize_raw_output_key(Some("frame")), Some("raw".to_string()));
        assert_eq!(normalize_raw_output_key(Some("raw")), Some("raw".to_string()));
        assert_eq!(normalize_raw_output_key(Some("undistorted")), Some("undistorted".to_string()));
        assert_eq!(normalize_raw_output_key(Some("overlay")), Some("raw".to_string()));
    }
}
