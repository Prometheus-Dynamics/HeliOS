use daedalus::planner::{GraphPatch, PatchReport};
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, RgbaImage};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use lib_cv::modules::calibration::LensModel;

use crate::stream::PipelineGraphMetrics;

use super::{DaedalusValue, GraphExecutor, GraphHandle, GraphProcessOptions};

#[derive(Clone)]
pub(crate) struct MultiplexPipeline {
    pub pipeline_id: uuid::Uuid,
    pub output_key: Option<String>,
    /// Selected host-output port forwarded for this pipeline instance (best-effort).
    pub output_port: Option<String>,
    pub graph: GraphHandle,
}

#[derive(Debug, Clone)]
struct FrameSource {
    src_idx: usize,
    /// Source port override; if unset, use the source instance's selected output port (or `frame`).
    from_port: Option<String>,
}

#[derive(Debug, Clone)]
struct ValueSource {
    to_port: String,
    src_idx: usize,
    /// Source port override; if unset, use the source instance's selected output port (or `frame`).
    from_port: Option<String>,
}

#[derive(Clone)]
pub(crate) struct MultiplexGraphExecutor {
    pipelines: Vec<MultiplexPipeline>,
    rows: u32,
    columns: u32,
    /// Row-major mapping of grid cells to pipeline indices.
    cells: Vec<Option<usize>>,
    active_pipeline_index: usize,
    /// Pipeline indices that should be executed (visible cells + unplaced pipelines + wiring deps).
    used_indices: Vec<usize>,
    /// Pre-resolved frame input sources (`to:frame`).
    frame_sources: BTreeMap<usize, FrameSource>,
    /// Pre-resolved value sources (`to:port` where port != frame).
    value_sources: BTreeMap<usize, Vec<ValueSource>>,
    /// Execution order respecting wiring dependencies (best-effort).
    exec_order: Vec<usize>,
    /// Indices that must be kept at full resolution because they feed downstream pipelines.
    full_res_sources: BTreeSet<usize>,
    cell_rect_cache: Arc<Mutex<CellRectCache>>,
    // Latest stream calibration, used to derive an "undistorted" calibration payload when
    // routing an undistorted image into downstream pipelines.
    calibration: Arc<std::sync::RwLock<Option<crate::ipc::StreamCalibration>>>,
}

impl MultiplexGraphExecutor {
    pub(crate) fn new(
        pipelines: Vec<MultiplexPipeline>,
        rows: u32,
        columns: u32,
        cells: Vec<Option<usize>>,
        active_pipeline_index: usize,
        process_all_pipelines: bool,
        wires: Vec<crate::ipc::StreamPipelineWire>,
    ) -> Self {
        fn norm_key(value: Option<&str>) -> Option<String> {
            value.map(str::trim).filter(|v| !v.is_empty()).map(|v| v.to_string())
        }

        const RAW_STREAM_PIPELINE_UUID: Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_0000000000aa);

        let mut used: BTreeSet<usize> = if process_all_pipelines {
            (0..pipelines.len()).collect()
        } else {
            let mut used = BTreeSet::new();
            for idx in cells.iter().copied().flatten() {
                if idx < pipelines.len() {
                    used.insert(idx);
                }
            }
            used
        };

        let mut by_id: BTreeMap<Uuid, Vec<usize>> = BTreeMap::new();
        for (idx, pipeline) in pipelines.iter().enumerate() {
            by_id.entry(pipeline.pipeline_id).or_default().push(idx);
        }

        let resolve_endpoint = |endpoint: &crate::ipc::StreamPipelineEndpoint, preferred_source_port: Option<&str>| -> Option<usize> {
            let candidates = by_id.get(&endpoint.pipeline_id)?;
            if candidates.is_empty() {
                return None;
            }
            let want_key =
                norm_key(endpoint.output_key.as_deref()).map(|value| if endpoint.pipeline_id == RAW_STREAM_PIPELINE_UUID && value.eq_ignore_ascii_case("frame") { "raw".to_string() } else { value });
            if let Some(want_key) = want_key {
                for idx in candidates.iter().copied() {
                    if pipelines.get(idx).is_some_and(|p| p.output_key.as_deref().map(str::trim) == Some(want_key.as_str())) {
                        return Some(idx);
                    }
                }
                // Couldn't find instance match: fall back to first instance with this pipeline ID.
                return Some(candidates[0]);
            }

            if endpoint.pipeline_id == RAW_STREAM_PIPELINE_UUID {
                let preferred_key = preferred_source_port.and_then(|value| norm_key(Some(value))).map(|value| if value.eq_ignore_ascii_case("frame") { "raw".to_string() } else { value });
                if let Some(preferred_key) = preferred_key {
                    if matches!(preferred_key.as_str(), "raw" | "undistorted") {
                        for idx in candidates.iter().copied() {
                            let Some(candidate) = pipelines.get(idx) else {
                                continue;
                            };
                            let output_key_matches = candidate.output_key.as_deref().map(str::trim).is_some_and(|v| v.eq_ignore_ascii_case(preferred_key.as_str()));
                            let output_port_matches = candidate.output_port.as_deref().map(str::trim).is_some_and(|v| v.eq_ignore_ascii_case(preferred_key.as_str()));
                            if output_key_matches || output_port_matches {
                                return Some(idx);
                            }
                        }
                    }
                }
            }

            if candidates.len() == 1 {
                return Some(candidates[0]);
            }
            // Ambiguous: choose the active pipeline instance if it matches.
            if pipelines.get(active_pipeline_index).is_some_and(|p| p.pipeline_id == endpoint.pipeline_id) {
                return Some(active_pipeline_index);
            }
            Some(candidates[0])
        };

        let mut frame_sources: BTreeMap<usize, FrameSource> = BTreeMap::new();
        let mut value_sources: BTreeMap<usize, Vec<ValueSource>> = BTreeMap::new();
        let mut full_res_sources: BTreeSet<usize> = BTreeSet::new();

        for wire in &wires {
            let to_port = wire.to.port.as_deref().unwrap_or("frame").trim().to_ascii_lowercase();
            let Some(src_idx) = resolve_endpoint(&wire.from, wire.from.port.as_deref()) else { continue };
            let Some(dst_idx) = resolve_endpoint(&wire.to, None) else { continue };
            used.insert(src_idx);
            used.insert(dst_idx);

            if to_port.eq_ignore_ascii_case("frame") {
                let mut from_port = norm_key(wire.from.port.as_deref()).map(|p| p.to_ascii_lowercase());
                if from_port.is_none() && wire.from.pipeline_id == RAW_STREAM_PIPELINE_UUID {
                    let key_as_port = norm_key(wire.from.output_key.as_deref()).map(|p| p.to_ascii_lowercase());
                    if matches!(key_as_port.as_deref(), Some("raw" | "frame" | "undistorted")) {
                        from_port = key_as_port;
                    }
                }
                frame_sources.insert(dst_idx, FrameSource { src_idx, from_port });
                full_res_sources.insert(src_idx);
            } else {
                let mut from_port = norm_key(wire.from.port.as_deref()).map(|p| p.to_ascii_lowercase());
                if from_port.is_none() && wire.from.pipeline_id == RAW_STREAM_PIPELINE_UUID {
                    let key_as_port = norm_key(wire.from.output_key.as_deref()).map(|p| p.to_ascii_lowercase());
                    if matches!(key_as_port.as_deref(), Some("raw" | "frame" | "undistorted")) {
                        from_port = key_as_port;
                    }
                }
                value_sources.entry(dst_idx).or_default().push(ValueSource { to_port, src_idx, from_port });
                // If a pipeline is feeding values, it must still be processed before the consumer.
                full_res_sources.insert(src_idx);
            }
        }

        let used_indices: Vec<usize> = used.into_iter().collect();

        // Derive an execution order that respects wiring dependencies (best-effort).
        let mut in_used = vec![false; pipelines.len()];
        for idx in used_indices.iter().copied() {
            if idx < in_used.len() {
                in_used[idx] = true;
            }
        }
        let mut indeg: Vec<usize> = vec![0; pipelines.len()];
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); pipelines.len()];
        for wire in &wires {
            let Some(src_idx) = resolve_endpoint(&wire.from, wire.from.port.as_deref()) else { continue };
            let Some(dst_idx) = resolve_endpoint(&wire.to, None) else { continue };
            if src_idx == dst_idx || src_idx >= pipelines.len() || dst_idx >= pipelines.len() {
                continue;
            }
            if !in_used[src_idx] || !in_used[dst_idx] {
                continue;
            }
            adj[src_idx].push(dst_idx);
            indeg[dst_idx] = indeg[dst_idx].saturating_add(1);
        }
        // Kahn: stable-ish (ascending index) processing.
        let mut queue: Vec<usize> = used_indices.iter().copied().filter(|idx| indeg.get(*idx).copied().unwrap_or(0) == 0).collect();
        queue.sort_unstable();
        let mut exec_order: Vec<usize> = Vec::with_capacity(used_indices.len());
        let mut queue_idx = 0usize;
        while queue_idx < queue.len() {
            let n = queue[queue_idx];
            queue_idx += 1;
            exec_order.push(n);
            for &m in adj.get(n).into_iter().flatten() {
                if let Some(v) = indeg.get_mut(m) {
                    *v = v.saturating_sub(1);
                    if *v == 0 {
                        queue.push(m);
                    }
                }
            }
        }
        if exec_order.len() != used_indices.len() {
            // Cycle or disconnected graph; fall back to a deterministic order.
            exec_order = used_indices.clone();
            exec_order.sort_unstable();
        }

        Self {
            pipelines,
            rows: rows.max(1),
            columns: columns.max(1),
            cells,
            active_pipeline_index,
            used_indices,
            frame_sources,
            value_sources,
            exec_order,
            full_res_sources,
            cell_rect_cache: Arc::new(Mutex::new(CellRectCache::default())),
            calibration: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    fn active_pipeline(&self) -> Option<&MultiplexPipeline> {
        self.pipelines.get(self.active_pipeline_index)
    }

    fn cell_extents(total: u32, count: u32, index: u32) -> (u32, u32) {
        let count = count.max(1);
        let base = total / count;
        let rem = total % count;
        let extra = if index < rem { 1 } else { 0 };
        let start = base.saturating_mul(index).saturating_add(rem.min(index));
        let extent = base.saturating_add(extra);
        (start, extent)
    }
}

#[derive(Clone, Copy)]
struct CellRect {
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
}

#[derive(Default)]
struct CellRectCache {
    width: u32,
    height: u32,
    rects: Vec<CellRect>,
}

fn calibration_is_valid(calib: &crate::ipc::StreamCalibration) -> bool {
    calib.fx.is_finite() && calib.fy.is_finite() && calib.fx > 0.0 && calib.fy > 0.0
}

fn undistorted_calibration(calib: Option<crate::ipc::StreamCalibration>) -> Option<crate::ipc::StreamCalibration> {
    let calib = calib?;
    if !calibration_is_valid(&calib) {
        return Some(calib);
    }
    Some(crate::ipc::StreamCalibration {
        fx: calib.fx,
        fy: calib.fy,
        cx: calib.cx,
        cy: calib.cy,
        k1: 0.0,
        k2: 0.0,
        p1: 0.0,
        p2: 0.0,
        k3: 0.0,
        // Used by some CV nodes; keep it low but non-zero.
        undistort_iters: 1,
        // IMPORTANT: "undistorted" images should be treated as pinhole w/ zero distortion.
        lens_model: LensModel::Pinhole,
    })
}

impl GraphExecutor for MultiplexGraphExecutor {
    fn set_calibration(&self, calibration: Option<crate::ipc::StreamCalibration>) {
        if let Ok(mut guard) = self.calibration.write() {
            *guard = calibration.clone();
        }
        for pipeline in &self.pipelines {
            pipeline.graph.set_calibration(calibration.clone());
        }
    }

    fn set_pipeline_inputs(&self, pipeline_id: Option<uuid::Uuid>, inputs: &BTreeMap<String, Option<DaedalusValue>>) {
        if inputs.is_empty() {
            return;
        }
        let target_id = pipeline_id.or_else(|| self.active_pipeline().map(|pipeline| pipeline.pipeline_id));
        if let Some(target_id) = target_id {
            for pipeline in &self.pipelines {
                if pipeline.pipeline_id == target_id {
                    pipeline.graph.set_pipeline_input_values(Some(target_id), inputs);
                }
            }
            return;
        }
        for pipeline in &self.pipelines {
            pipeline.graph.set_pipeline_input_values(Some(pipeline.pipeline_id), inputs);
        }
    }

    fn apply_graph_patch(&self, pipeline_id: Option<uuid::Uuid>, patch: &GraphPatch) -> Option<PatchReport> {
        let target_id = pipeline_id.or_else(|| self.active_pipeline().map(|pipeline| pipeline.pipeline_id))?;
        let mut report: Option<PatchReport> = None;
        for pipeline in &self.pipelines {
            if pipeline.pipeline_id == target_id {
                let applied = pipeline.graph.apply_graph_patch(None, patch);
                if report.is_none() {
                    report = applied;
                }
            }
        }
        report
    }

    fn process(&self, image: DynamicImage) -> Option<DynamicImage> {
        self.process_with_options(image, GraphProcessOptions::default())
    }

    fn process_with_options(&self, image: DynamicImage, options: GraphProcessOptions) -> Option<DynamicImage> {
        let (width, height) = image.dimensions();
        if width == 0 || height == 0 {
            return if options.require_image_output { Some(image) } else { None };
        }

        if self.used_indices.is_empty() {
            // No pipelines selected: emit an empty (black) canvas.
            return if options.require_image_output { Some(DynamicImage::ImageRgba8(RgbaImage::new(width, height))) } else { None };
        }

        const INPUT_FILTER: FilterType = FilterType::Nearest;

        let mut outputs: Vec<Option<DynamicImage>> = vec![None; self.pipelines.len()];

        let raw = image;

        let rows = self.rows.max(1);
        let cols = self.columns.max(1);

        let cell_rects = {
            let mut cell_rects_guard = self.cell_rect_cache.lock().unwrap();
            if cell_rects_guard.width != width || cell_rects_guard.height != height || cell_rects_guard.rects.len() != (rows * cols) as usize {
                cell_rects_guard.width = width;
                cell_rects_guard.height = height;
                cell_rects_guard.rects.clear();
                cell_rects_guard.rects.reserve((rows * cols) as usize);
                for row in 0..rows {
                    let (y0, cell_h) = Self::cell_extents(height, rows, row);
                    for col in 0..cols {
                        let (x0, cell_w) = Self::cell_extents(width, cols, col);
                        cell_rects_guard.rects.push(CellRect { x0, y0, w: cell_w, h: cell_h });
                    }
                }
            }
            cell_rects_guard.rects.clone()
        };

        let base_calib = self.calibration.read().ok().map(|g| (*g).clone()).unwrap_or(None);
        let undistorted_calib = undistorted_calibration(base_calib.clone());

        // Determine desired input thumbnail size per pipeline. Active pipeline keeps full res;
        // other pipelines get downscaled to their largest cell for speed.
        let mut desired_input: Vec<(u32, u32)> = vec![(width, height); self.pipelines.len()];
        for (cell_index, pipeline_idx) in self.cells.iter().copied().flatten().enumerate() {
            if pipeline_idx >= self.pipelines.len() {
                continue;
            }
            if pipeline_idx == self.active_pipeline_index {
                continue;
            }
            if self.full_res_sources.contains(&pipeline_idx) {
                continue;
            }
            let Some(rect) = cell_rects.get(cell_index) else {
                continue;
            };
            let cell_w = rect.w;
            let cell_h = rect.h;
            if cell_w == 0 || cell_h == 0 {
                continue;
            }
            let (w0, h0) = desired_input[pipeline_idx];
            desired_input[pipeline_idx] = (w0.max(cell_w), h0.max(cell_h));
        }

        let mut raw_rgba_full: Option<RgbaImage> = None;

        const RAW_STREAM_PIPELINE_UUID: Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_0000000000aa);

        for idx in self.exec_order.iter().copied() {
            let Some(pipeline) = self.pipelines.get(idx) else { continue };

            // Resolve pipeline frame input.
            let mut input_image: DynamicImage = raw.clone();
            let mut wants_undistorted_calib = false;

            if let Some(source) = self.frame_sources.get(&idx) {
                let src_idx = source.src_idx;
                if let Some(src_pipeline) = self.pipelines.get(src_idx) {
                    let port = source.from_port.as_deref().or(src_pipeline.output_port.as_deref()).unwrap_or("frame");

                    // Prefer sampling the declared port; fall back to the source preview output.
                    let sampled = src_pipeline.graph.sample_image_output(port).or_else(|| outputs.get(src_idx).and_then(|v| v.clone()));
                    if let Some(sampled) = sampled {
                        input_image = sampled;
                    }
                    if src_pipeline.pipeline_id == RAW_STREAM_PIPELINE_UUID && port.eq_ignore_ascii_case("undistorted") {
                        wants_undistorted_calib = true;
                    }
                }
            }

            // Apply calibration: if we're feeding an undistorted RAW view into a downstream pipeline,
            // treat it as pinhole/zero-distortion to avoid double undistort.
            if pipeline.pipeline_id == RAW_STREAM_PIPELINE_UUID {
                pipeline.graph.set_calibration(base_calib.clone());
            } else if wants_undistorted_calib {
                pipeline.graph.set_calibration(undistorted_calib.clone());
            } else {
                pipeline.graph.set_calibration(base_calib.clone());
            }

            // Wire value-like sources into downstream pipeline ports (best-effort).
            if let Some(values) = self.value_sources.get(&idx) {
                let mut input_values: BTreeMap<String, Option<DaedalusValue>> = BTreeMap::new();
                for wire in values {
                    let Some(src_pipeline) = self.pipelines.get(wire.src_idx) else {
                        input_values.insert(wire.to_port.clone(), None);
                        continue;
                    };
                    let port = wire.from_port.as_deref().or(src_pipeline.output_port.as_deref()).unwrap_or("frame");
                    let sampled = src_pipeline.graph.sample_value_output(port);
                    input_values.insert(wire.to_port.clone(), sampled);
                }
                if !input_values.is_empty() {
                    pipeline.graph.set_pipeline_input_values(None, &input_values);
                }
            }

            // Downscale inputs for non-active pipelines based on their cell size (or wiring rules).
            let (tw, th) = desired_input.get(idx).copied().unwrap_or((width, height));
            if idx != self.active_pipeline_index && (tw < width || th < height) {
                // Downscale from camera raw efficiently when possible.
                if !self.frame_sources.contains_key(&idx) {
                    if tw < width || th < height {
                        let rgba = raw_rgba_full.get_or_insert_with(|| raw.to_rgba8());
                        let resized = image::imageops::resize(rgba, tw.max(1), th.max(1), INPUT_FILTER);
                        input_image = DynamicImage::ImageRgba8(resized);
                    }
                } else {
                    let (iw, ih) = input_image.dimensions();
                    if iw != tw || ih != th {
                        let resized = image::imageops::resize(&input_image.to_rgba8(), tw.max(1), th.max(1), INPUT_FILTER);
                        input_image = DynamicImage::ImageRgba8(resized);
                    }
                }
            }

            outputs[idx] = pipeline.graph.process_with_options(input_image, options);
        }

        if !options.require_image_output {
            return None;
        }

        let outputs_rgba: Vec<Option<RgbaImage>> = outputs.into_iter().map(|img| img.map(|img| img.into_rgba8())).collect();
        let mut canvas_raw = vec![0u8; (width as usize).saturating_mul(height as usize).saturating_mul(4)];
        let canvas_stride = (width as usize).saturating_mul(4);
        let blit = |dst: &mut [u8], src: &RgbaImage, x0: u32, y0: u32, dst_stride: usize| {
            let src_w = src.width() as usize;
            let src_h = src.height() as usize;
            let x0 = x0 as usize;
            let y0 = y0 as usize;
            let src_stride = src_w.saturating_mul(4);
            let src_raw = src.as_raw();
            for row in 0..src_h {
                let dst_off = (y0 + row).saturating_mul(dst_stride).saturating_add(x0.saturating_mul(4));
                let src_off = row.saturating_mul(src_stride);
                if dst_off + src_stride <= dst.len() && src_off + src_stride <= src_raw.len() {
                    dst[dst_off..dst_off + src_stride].copy_from_slice(&src_raw[src_off..src_off + src_stride]);
                }
            }
        };
        let blit_scaled_nearest = |dst: &mut [u8], src: &RgbaImage, x0: u32, y0: u32, dst_stride: usize, cell_w: u32, cell_h: u32| {
            let src_w = src.width() as usize;
            let src_h = src.height() as usize;
            let x0 = x0 as usize;
            let y0 = y0 as usize;
            let cell_w = cell_w as usize;
            let cell_h = cell_h as usize;
            if src_w == 0 || src_h == 0 || cell_w == 0 || cell_h == 0 {
                return;
            }
            let src_raw = src.as_raw();
            for y in 0..cell_h {
                let src_y = y.saturating_mul(src_h) / cell_h;
                let dst_row = (y0 + y).saturating_mul(dst_stride).saturating_add(x0.saturating_mul(4));
                let src_row = src_y.saturating_mul(src_w).saturating_mul(4);
                for x in 0..cell_w {
                    let src_x = x.saturating_mul(src_w) / cell_w;
                    let dst_off = dst_row.saturating_add(x.saturating_mul(4));
                    let src_off = src_row.saturating_add(src_x.saturating_mul(4));
                    if dst_off + 4 <= dst.len() && src_off + 4 <= src_raw.len() {
                        dst[dst_off..dst_off + 4].copy_from_slice(&src_raw[src_off..src_off + 4]);
                    }
                }
            }
        };

        let dst = canvas_raw.as_mut_slice();
        for (cell_index, rect) in cell_rects.iter().enumerate() {
            let cell_w = rect.w;
            let cell_h = rect.h;
            if cell_w == 0 || cell_h == 0 {
                continue;
            }
            let Some(pipeline_idx) = self.cells.get(cell_index).copied().flatten() else {
                // Empty cell: leave blank (black).
                continue;
            };
            let Some(img) = outputs_rgba.get(pipeline_idx).and_then(|img| img.as_ref()) else {
                continue;
            };

            let (src_w, src_h) = img.dimensions();
            if src_w == cell_w && src_h == cell_h {
                blit(dst, img, rect.x0, rect.y0, canvas_stride);
                continue;
            }

            blit_scaled_nearest(dst, img, rect.x0, rect.y0, canvas_stride, cell_w, cell_h);
        }

        let canvas = RgbaImage::from_raw(width, height, canvas_raw).unwrap_or_else(|| RgbaImage::new(width, height));
        Some(DynamicImage::ImageRgba8(canvas))
    }

    fn host_output_ports(&self) -> Option<Vec<String>> {
        self.active_pipeline().and_then(|pipeline| pipeline.graph.host_output_ports())
    }

    fn host_output_port_types(&self) -> Option<BTreeMap<String, daedalus::data::model::TypeExpr>> {
        self.active_pipeline().and_then(|pipeline| pipeline.graph.host_output_port_types())
    }

    fn sample_json_output(&self, port: &str) -> Option<Value> {
        self.active_pipeline().and_then(|pipeline| pipeline.graph.sample_json_output(port))
    }

    fn sample_value_output(&self, port: &str) -> Option<daedalus::data::model::Value> {
        self.active_pipeline().and_then(|pipeline| pipeline.graph.sample_value_output(port))
    }

    fn pipeline_metrics(&self) -> Option<PipelineGraphMetrics> {
        self.active_pipeline().and_then(|pipeline| pipeline.graph.pipeline_metrics())
    }

    fn pipeline_metrics_by_pipeline(&self) -> Option<BTreeMap<String, PipelineGraphMetrics>> {
        let mut out = BTreeMap::new();
        for pipeline in &self.pipelines {
            if let Some(metrics) = pipeline.graph.pipeline_metrics() {
                out.insert(pipeline.pipeline_id.to_string(), metrics);
            }
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    fn set_perf_enabled(&self, pipeline_id: Option<uuid::Uuid>, enabled: bool) {
        let target_id = pipeline_id.or_else(|| self.active_pipeline().map(|pipeline| pipeline.pipeline_id));
        if let Some(target_id) = target_id {
            for pipeline in &self.pipelines {
                if pipeline.pipeline_id == target_id {
                    pipeline.graph.set_perf_enabled(Some(target_id), enabled);
                }
            }
            return;
        }
        for pipeline in &self.pipelines {
            pipeline.graph.set_perf_enabled(Some(pipeline.pipeline_id), enabled);
        }
    }

    fn reset_pipeline_metrics(&self, pipeline_id: Option<uuid::Uuid>) {
        let target_id = pipeline_id.or_else(|| self.active_pipeline().map(|pipeline| pipeline.pipeline_id));
        if let Some(target_id) = target_id {
            for pipeline in &self.pipelines {
                if pipeline.pipeline_id == target_id {
                    pipeline.graph.reset_pipeline_metrics(Some(target_id));
                }
            }
            return;
        }
        for pipeline in &self.pipelines {
            pipeline.graph.reset_pipeline_metrics(Some(pipeline.pipeline_id));
        }
    }

    fn capture_flamegraph(&self, pipeline_id: Option<uuid::Uuid>, duration_ms: u64) -> Result<(), String> {
        let target_id = pipeline_id.or_else(|| self.active_pipeline().map(|pipeline| pipeline.pipeline_id)).ok_or_else(|| "no pipeline selected".to_string())?;
        for pipeline in &self.pipelines {
            if pipeline.pipeline_id == target_id {
                return pipeline.graph.capture_flamegraph(Some(target_id), duration_ms);
            }
        }
        Err("pipeline not found".into())
    }
}
