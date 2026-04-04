use super::*;

#[derive(Debug, Default)]
pub(super) struct GraphImageWorkingSetTracker {
    pub(super) input_image_bytes: AtomicU64,
    pub(super) host_output_image_bytes: AtomicU64,
    pub(super) preview_image_bytes: AtomicU64,
    pub(super) total_materialized_image_bytes: AtomicU64,
    pub(super) peak_total_materialized_image_bytes: AtomicU64,
}

#[derive(Debug, Clone)]
pub(super) enum TypedHostOutputSample {
    ArucoDetections(Arc<Vec<ArucoDetection2D>>),
}

impl TypedHostOutputSample {
    pub(super) fn to_json(&self) -> Option<Value> {
        match self {
            Self::ArucoDetections(detections) => serde_json::to_value(detections.as_ref()).ok(),
        }
    }

    pub(super) fn to_daedalus_value(&self) -> Option<DaedalusValue> {
        self.to_json().map(|json| json_to_daedalus_value(&json))
    }

    pub(super) fn size_bytes(&self) -> u64 {
        match self {
            // Sample-cache metrics are retained working-set numbers, not wire-format bytes. Use a
            // direct in-memory estimate here so pipeline metrics do not serialize detections to
            // JSON on every snapshot just to approximate cache pressure.
            Self::ArucoDetections(detections) => aruco_detections_retained_bytes(detections),
        }
    }
}

fn aruco_detections_retained_bytes(detections: &Arc<Vec<ArucoDetection2D>>) -> u64 {
    let base = std::mem::size_of::<Vec<ArucoDetection2D>>() as u64 + detections.capacity() as u64 * std::mem::size_of::<ArucoDetection2D>() as u64;
    base + detections.iter().map(aruco_detection_nested_bytes).sum::<u64>()
}

fn aruco_detection_nested_bytes(detection: &ArucoDetection2D) -> u64 {
    detection.bits.as_ref().map(aruco_bit_grid_bytes).unwrap_or(0)
}

fn aruco_bit_grid_bytes(bits: &lib_cv::modules::aruco::ArucoBitGrid) -> u64 {
    bits.rows.capacity() as u64 * std::mem::size_of::<String>() as u64 + bits.rows.iter().map(|row| row.capacity() as u64).sum::<u64>()
}

impl GraphImageWorkingSetTracker {
    pub(super) fn update_peak(slot: &AtomicU64, value: u64) {
        let mut current = slot.load(Ordering::Relaxed);
        while value > current {
            match slot.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(next) => current = next,
            }
        }
    }

    pub(super) fn record(&self, input: u64, host_output: u64, preview: u64) {
        let total = input.saturating_add(host_output).saturating_add(preview);
        self.input_image_bytes.store(input, Ordering::Relaxed);
        self.host_output_image_bytes.store(host_output, Ordering::Relaxed);
        self.preview_image_bytes.store(preview, Ordering::Relaxed);
        self.total_materialized_image_bytes.store(total, Ordering::Relaxed);
        Self::update_peak(&self.peak_total_materialized_image_bytes, total);
    }

    pub(super) fn clear_current(&self) {
        self.input_image_bytes.store(0, Ordering::Relaxed);
        self.host_output_image_bytes.store(0, Ordering::Relaxed);
        self.preview_image_bytes.store(0, Ordering::Relaxed);
        self.total_materialized_image_bytes.store(0, Ordering::Relaxed);
    }

    pub(super) fn snapshot(&self) -> Option<PipelineImageWorkingSetMetrics> {
        let input = self.input_image_bytes.load(Ordering::Relaxed);
        let host_output = self.host_output_image_bytes.load(Ordering::Relaxed);
        let preview = self.preview_image_bytes.load(Ordering::Relaxed);
        let total = self.total_materialized_image_bytes.load(Ordering::Relaxed);
        let peak = self.peak_total_materialized_image_bytes.load(Ordering::Relaxed);
        if input == 0 && host_output == 0 && preview == 0 && total == 0 && peak == 0 {
            None
        } else {
            Some(PipelineImageWorkingSetMetrics {
                input_image_bytes: input,
                host_output_image_bytes: host_output,
                preview_image_bytes: preview,
                total_materialized_image_bytes: total,
                peak_total_materialized_image_bytes: peak,
            })
        }
    }
}

const NODE_METRICS_WINDOW_OFF: usize = 16;
const NODE_METRICS_WINDOW_BASIC: usize = 32;
const NODE_METRICS_WINDOW_DETAILED: usize = 64;
const NODE_METRICS_WINDOW_PROFILE: usize = 100;

pub(super) fn node_metrics_window(level: DaedalusMetricsLevel) -> usize {
    match level {
        DaedalusMetricsLevel::Off => NODE_METRICS_WINDOW_OFF,
        DaedalusMetricsLevel::Basic => NODE_METRICS_WINDOW_BASIC,
        DaedalusMetricsLevel::Detailed => NODE_METRICS_WINDOW_DETAILED,
        DaedalusMetricsLevel::Profile => NODE_METRICS_WINDOW_PROFILE,
    }
}
pub(super) const GRAPH_ERROR_DISABLE_THRESHOLD: u64 = 5;

#[derive(Debug, Clone)]
pub(super) struct NodeInfo {
    pub(super) type_id: String,
    pub(super) label: Option<String>,
    pub(super) group: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct EdgeInfo {
    pub(super) from_node_index: usize,
    pub(super) from_node_label: Option<String>,
    pub(super) from_port: String,
    pub(super) to_node_index: usize,
    pub(super) to_node_label: Option<String>,
    pub(super) to_port: String,
    pub(super) queue_capacity: Option<u64>,
    pub(super) policy: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct NodePerfSample {
    cache_misses: f64,
    branch_instructions: f64,
    branch_misses: f64,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct NodePayloadSample {
    average_input_payload_bytes: f64,
    average_output_payload_bytes: f64,
    peak_input_payload_bytes: u64,
    peak_output_payload_bytes: u64,
    peak_payload_working_set_bytes: u64,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct EdgeMetricSample {
    pub(super) total_wait: Duration,
    pub(super) samples: usize,
    pub(super) max_depth: u64,
    pub(super) current_depth: u64,
    pub(super) peak_queue_bytes: u64,
    pub(super) current_queue_bytes: u64,
    pub(super) capacity: Option<u64>,
    pub(super) drops: u64,
    pub(super) transport_bytes: u64,
    pub(super) transport_count: u64,
    pub(super) gpu_uploads: u64,
    pub(super) gpu_downloads: u64,
}

#[derive(Debug, Default)]
pub(super) struct RollingGraphMetrics {
    window: usize,
    node_info: Vec<NodeInfo>,
    edge_info: Vec<EdgeInfo>,
    pub(super) samples: BTreeMap<usize, VecDeque<(Instant, f64)>>,
    node_perf_samples: BTreeMap<usize, VecDeque<NodePerfSample>>,
    node_perf_last_at: BTreeMap<usize, Instant>,
    node_payload_samples: BTreeMap<usize, VecDeque<NodePayloadSample>>,
    node_payload_last_at: BTreeMap<usize, Instant>,
    pub(super) edge_samples: BTreeMap<usize, VecDeque<EdgeMetricSample>>,
    edge_last_at: BTreeMap<usize, Instant>,
    group_samples: BTreeMap<String, VecDeque<(Instant, f64)>>,
    group_perf_samples: BTreeMap<String, VecDeque<NodePerfSample>>,
    group_perf_last_at: BTreeMap<String, Instant>,
    group_payload_samples: BTreeMap<String, VecDeque<NodePayloadSample>>,
    group_payload_last_at: BTreeMap<String, Instant>,
    pub(super) graph_samples: VecDeque<(Instant, f64)>,
    pub(super) wrapper_samples: VecDeque<f64>,
    wrapper_last_at: Option<Instant>,
    lock_samples: VecDeque<f64>,
    lock_last_at: Option<Instant>,
    output_materialization_samples: VecDeque<f64>,
    output_materialization_last_at: Option<Instant>,
    perf_samples: VecDeque<perf::PerfSample>,
    perf_last_at: Option<Instant>,
    last_flamegraph: Option<flamegraph::FlamegraphCapture>,
    pub(super) warnings: VecDeque<(Instant, String)>,
    last_errors: BTreeMap<String, (Instant, String)>,
}

impl RollingGraphMetrics {
    pub(super) fn new(window: usize, node_info: Vec<NodeInfo>, edge_info: Vec<EdgeInfo>) -> Self {
        Self {
            window,
            node_info,
            edge_info,
            samples: BTreeMap::new(),
            node_perf_samples: BTreeMap::new(),
            node_perf_last_at: BTreeMap::new(),
            node_payload_samples: BTreeMap::new(),
            node_payload_last_at: BTreeMap::new(),
            edge_samples: BTreeMap::new(),
            edge_last_at: BTreeMap::new(),
            group_samples: BTreeMap::new(),
            group_perf_samples: BTreeMap::new(),
            group_perf_last_at: BTreeMap::new(),
            group_payload_samples: BTreeMap::new(),
            group_payload_last_at: BTreeMap::new(),
            graph_samples: VecDeque::new(),
            wrapper_samples: VecDeque::new(),
            wrapper_last_at: None,
            lock_samples: VecDeque::new(),
            lock_last_at: None,
            output_materialization_samples: VecDeque::new(),
            output_materialization_last_at: None,
            perf_samples: VecDeque::new(),
            perf_last_at: None,
            last_flamegraph: None,
            warnings: VecDeque::new(),
            last_errors: BTreeMap::new(),
        }
    }

    pub(super) fn record_telemetry(&mut self, telemetry: &DaedalusExecutionTelemetry) {
        let now = Instant::now();
        for warning in telemetry.warnings.iter().cloned() {
            if is_internal_runtime_warning(&warning) {
                continue;
            }
            self.record_warning(warning);
        }
        // Surface runtime node failures in the same warning/error surface the UI already shows.
        // These used to be invisible unless the whole graph run failed.
        for failure in telemetry.errors.iter() {
            let node_type = failure.node_id.as_str();
            let msg = format!("{}: {}", failure.code, failure.message);
            let prev = self.last_errors.get(node_type).map(|(_, m)| m.as_str()).unwrap_or("");
            if prev != msg {
                self.last_errors.insert(node_type.to_string(), (now, msg.clone()));
                // Keep a human-visible entry in the rolling warnings list too.
                self.warnings.push_back((now, format!("[error] {node_type}: {msg}")));
                while self.warnings.len() > self.window {
                    self.warnings.pop_front();
                }
            }
        }
        for (node_idx, node_metrics) in &telemetry.node_metrics {
            let calls = node_metrics.calls.max(1) as f64;
            let avg_ms = node_metrics.total_duration.as_secs_f64() * 1000.0 / calls;
            let deque = self.samples.entry(*node_idx).or_default();
            deque.push_back((now, avg_ms));
            while deque.len() > self.window {
                deque.pop_front();
            }
            if let Some(perf) = node_metrics.perf.as_ref() {
                let sample =
                    NodePerfSample { cache_misses: perf.cache_misses as f64 / calls, branch_instructions: perf.branch_instructions as f64 / calls, branch_misses: perf.branch_misses as f64 / calls };
                let perf_deque = self.node_perf_samples.entry(*node_idx).or_default();
                perf_deque.push_back(sample);
                while perf_deque.len() > self.window {
                    perf_deque.pop_front();
                }
                self.node_perf_last_at.insert(*node_idx, now);
            }
            if let Some(payload) = node_metrics.transport.as_ref() {
                let sample = NodePayloadSample {
                    average_input_payload_bytes: payload.in_bytes as f64 / calls,
                    average_output_payload_bytes: payload.out_bytes as f64 / calls,
                    peak_input_payload_bytes: payload.peak_input_bytes,
                    peak_output_payload_bytes: payload.peak_output_bytes,
                    peak_payload_working_set_bytes: payload.peak_working_set_bytes,
                };
                let payload_deque = self.node_payload_samples.entry(*node_idx).or_default();
                payload_deque.push_back(sample);
                while payload_deque.len() > self.window {
                    payload_deque.pop_front();
                }
                self.node_payload_last_at.insert(*node_idx, now);
            }
        }
        for (group_id, group_metrics) in &telemetry.group_metrics {
            let total_ms = group_metrics.total_duration.as_secs_f64() * 1000.0;
            let deque = self.group_samples.entry(group_id.clone()).or_default();
            deque.push_back((now, total_ms));
            while deque.len() > self.window {
                deque.pop_front();
            }
            if let Some(perf) = group_metrics.perf.as_ref() {
                let sample = NodePerfSample { cache_misses: perf.cache_misses as f64, branch_instructions: perf.branch_instructions as f64, branch_misses: perf.branch_misses as f64 };
                let perf_deque = self.group_perf_samples.entry(group_id.clone()).or_default();
                perf_deque.push_back(sample);
                while perf_deque.len() > self.window {
                    perf_deque.pop_front();
                }
                self.group_perf_last_at.insert(group_id.clone(), now);
            }
            if let Some(payload) = group_metrics.transport.as_ref() {
                let calls = group_metrics.calls.max(1) as f64;
                let sample = NodePayloadSample {
                    average_input_payload_bytes: payload.in_bytes as f64 / calls,
                    average_output_payload_bytes: payload.out_bytes as f64 / calls,
                    peak_input_payload_bytes: payload.peak_input_bytes,
                    peak_output_payload_bytes: payload.peak_output_bytes,
                    peak_payload_working_set_bytes: payload.peak_working_set_bytes,
                };
                let payload_deque = self.group_payload_samples.entry(group_id.clone()).or_default();
                payload_deque.push_back(sample);
                while payload_deque.len() > self.window {
                    payload_deque.pop_front();
                }
                self.group_payload_last_at.insert(group_id.clone(), now);
            }
        }
        for (edge_idx, edge_metrics) in &telemetry.edge_metrics {
            let deque = self.edge_samples.entry(*edge_idx).or_default();
            deque.push_back(EdgeMetricSample {
                total_wait: edge_metrics.total_wait,
                samples: edge_metrics.samples,
                max_depth: edge_metrics.max_depth,
                current_depth: edge_metrics.current_depth,
                peak_queue_bytes: edge_metrics.peak_queue_bytes,
                current_queue_bytes: edge_metrics.current_queue_bytes,
                capacity: edge_metrics.capacity,
                drops: edge_metrics.drops,
                transport_bytes: edge_metrics.transport_bytes,
                transport_count: edge_metrics.transport_count,
                gpu_uploads: edge_metrics.gpu_uploads,
                gpu_downloads: edge_metrics.gpu_downloads,
            });
            while deque.len() > self.window {
                deque.pop_front();
            }
            self.edge_last_at.insert(*edge_idx, now);
        }
    }

    pub(super) fn record_graph_duration(&mut self, duration: Duration) {
        let now = Instant::now();
        let ms = duration.as_secs_f64() * 1000.0;
        self.graph_samples.push_back((now, ms));
        while self.graph_samples.len() > self.window {
            self.graph_samples.pop_front();
        }
    }

    pub(super) fn record_wrapper_duration(&mut self, duration: Duration) {
        let now = Instant::now();
        let ms = duration.as_secs_f64() * 1000.0;
        self.wrapper_samples.push_back(ms);
        while self.wrapper_samples.len() > self.window {
            self.wrapper_samples.pop_front();
        }
        self.wrapper_last_at = Some(now);
    }

    pub(super) fn record_lock_duration(&mut self, duration: Duration) {
        let now = Instant::now();
        let ms = duration.as_secs_f64() * 1000.0;
        self.lock_samples.push_back(ms);
        while self.lock_samples.len() > self.window {
            self.lock_samples.pop_front();
        }
        self.lock_last_at = Some(now);
    }

    pub(super) fn record_output_materialization_duration(&mut self, duration: Duration) {
        let now = Instant::now();
        let ms = duration.as_secs_f64() * 1000.0;
        self.output_materialization_samples.push_back(ms);
        while self.output_materialization_samples.len() > self.window {
            self.output_materialization_samples.pop_front();
        }
        self.output_materialization_last_at = Some(now);
    }

    pub(super) fn record_perf_sample(&mut self, sample: perf::PerfSample) {
        let now = Instant::now();
        self.perf_samples.push_back(sample);
        while self.perf_samples.len() > self.window {
            self.perf_samples.pop_front();
        }
        self.perf_last_at = Some(now);
    }

    pub(super) fn record_flamegraph(&mut self, capture: flamegraph::FlamegraphCapture) {
        self.last_flamegraph = Some(capture);
    }

    pub(super) fn record_error(&mut self, node_type: Option<&str>, error: String) {
        let now = Instant::now();
        if let Some(node_type) = node_type {
            self.last_errors.insert(node_type.to_string(), (now, error));
        } else {
            self.record_warning(error);
        }
    }

    pub(super) fn record_warning(&mut self, warning: String) {
        let now = Instant::now();
        self.warnings.push_back((now, warning));
        while self.warnings.len() > self.window {
            self.warnings.pop_front();
        }
    }

    pub(super) fn reset(&mut self) {
        self.samples.clear();
        self.node_perf_samples.clear();
        self.node_perf_last_at.clear();
        self.node_payload_samples.clear();
        self.node_payload_last_at.clear();
        self.edge_samples.clear();
        self.edge_last_at.clear();
        self.group_samples.clear();
        self.group_perf_samples.clear();
        self.group_perf_last_at.clear();
        self.group_payload_samples.clear();
        self.group_payload_last_at.clear();
        self.graph_samples.clear();
        self.wrapper_samples.clear();
        self.wrapper_last_at = None;
        self.lock_samples.clear();
        self.lock_last_at = None;
        self.output_materialization_samples.clear();
        self.output_materialization_last_at = None;
        self.perf_samples.clear();
        self.perf_last_at = None;
        self.last_flamegraph = None;
        self.warnings.clear();
        self.last_errors.clear();
    }

    pub(super) fn release_idle_retention(&mut self) {
        self.samples.clear();
        self.node_perf_samples.clear();
        self.node_perf_last_at.clear();
        self.node_payload_samples.clear();
        self.node_payload_last_at.clear();
        self.edge_samples.clear();
        self.edge_last_at.clear();
        self.group_samples.clear();
        self.group_perf_samples.clear();
        self.group_perf_last_at.clear();
        self.group_payload_samples.clear();
        self.group_payload_last_at.clear();
        self.graph_samples.clear();
        self.wrapper_samples.clear();
        self.wrapper_last_at = None;
        self.lock_samples.clear();
        self.lock_last_at = None;
        self.output_materialization_samples.clear();
        self.output_materialization_last_at = None;
        self.perf_samples.clear();
        self.perf_last_at = None;
        self.last_flamegraph = None;
    }

    pub(super) fn snapshot(&self) -> PipelineGraphMetrics {
        let now = Instant::now();
        let summarize_payload = |payload_deque: &VecDeque<NodePayloadSample>| {
            let sample_count = payload_deque.len() as f64;
            let average_input_payload_bytes = payload_deque.iter().map(|sample| sample.average_input_payload_bytes).sum::<f64>() / sample_count.max(1.0);
            let average_output_payload_bytes = payload_deque.iter().map(|sample| sample.average_output_payload_bytes).sum::<f64>() / sample_count.max(1.0);
            let peak_input_payload_bytes = payload_deque.iter().map(|sample| sample.peak_input_payload_bytes).max().unwrap_or(0);
            let peak_output_payload_bytes = payload_deque.iter().map(|sample| sample.peak_output_payload_bytes).max().unwrap_or(0);
            let peak_payload_working_set_bytes = payload_deque.iter().map(|sample| sample.peak_payload_working_set_bytes).max().unwrap_or(0);
            (average_input_payload_bytes, average_output_payload_bytes, peak_input_payload_bytes, peak_output_payload_bytes, peak_payload_working_set_bytes)
        };
        let summarize_timing = |deque: &VecDeque<f64>, last_at: Option<Instant>| -> Option<crate::stream::PipelineTimingMetrics> {
            if deque.is_empty() {
                return None;
            }
            let sample_count = deque.len() as u64;
            let sum_ms: f64 = deque.iter().copied().sum();
            let average_time_ms = sum_ms / sample_count.max(1) as f64;
            let last_time_ms = deque.back().copied().unwrap_or(0.0);
            Some(crate::stream::PipelineTimingMetrics {
                average_time_ms,
                last_time_ms,
                sample_count,
                window_size: self.window as u64,
                last_sample_age_ms: last_at.map(|instant| now.saturating_duration_since(instant).as_millis() as u64),
            })
        };
        let mut out = BTreeMap::new();
        let mut type_counts: BTreeMap<&str, usize> = BTreeMap::new();
        for node_idx in self.samples.keys().copied() {
            if let Some(info) = self.node_info.get(node_idx) {
                *type_counts.entry(info.type_id.as_str()).or_insert(0) += 1;
            }
        }
        let mut type_instance: BTreeMap<&str, usize> = BTreeMap::new();

        for (node_idx, deque) in &self.samples {
            if deque.is_empty() {
                continue;
            }
            let info = self.node_info.get(*node_idx);
            let type_id = info.map(|info| info.type_id.as_str()).unwrap_or("unknown");
            let count = type_counts.get(type_id).copied().unwrap_or(0);
            let key = if count <= 1 {
                type_id.to_string()
            } else {
                let idx = type_instance.entry(type_id).or_insert(0);
                *idx += 1;
                format!("{type_id}#{idx}")
            };

            let sample_count = deque.len() as u64;
            let sum_ms: f64 = deque.iter().map(|(_, ms)| *ms).sum();
            let average_time_ms = sum_ms / sample_count.max(1) as f64;
            let (first_t, _) = deque.front().copied().unwrap_or((now, 0.0));
            let (last_t, _) = deque.back().copied().unwrap_or((now, 0.0));
            let elapsed = last_t.saturating_duration_since(first_t).as_secs_f64();
            let average_fps = if elapsed > 0.000_001 && sample_count > 1 { (sample_count as f64 - 1.0) / elapsed } else { 0.0 };
            let last_sample_age_ms = Some(now.saturating_duration_since(last_t).as_millis() as u64);
            let perf = self.node_perf_samples.get(node_idx).and_then(|perf_deque| {
                if perf_deque.is_empty() {
                    return None;
                }
                let perf_sample_count = perf_deque.len() as u64;
                let sum_cache: f64 = perf_deque.iter().map(|sample| sample.cache_misses).sum();
                let sum_branch_inst: f64 = perf_deque.iter().map(|sample| sample.branch_instructions).sum();
                let sum_branch_miss: f64 = perf_deque.iter().map(|sample| sample.branch_misses).sum();
                let last_age_ms = self.node_perf_last_at.get(node_idx).map(|instant| now.saturating_duration_since(*instant).as_millis() as u64);
                Some(PipelineNodePerfMetrics {
                    average_cache_misses: sum_cache / perf_sample_count.max(1) as f64,
                    average_branch_instructions: sum_branch_inst / perf_sample_count.max(1) as f64,
                    average_branch_misses: sum_branch_miss / perf_sample_count.max(1) as f64,
                    sample_count: perf_sample_count,
                    window_size: self.window as u64,
                    last_sample_age_ms: last_age_ms,
                })
            });
            let payload = self.node_payload_samples.get(node_idx).map(&summarize_payload);
            out.insert(
                key,
                PipelineNodeRuntimeMetrics {
                    metrics: PipelineNodeMetrics { average_time_ms, average_fps, sample_count, window_size: self.window as u64, last_sample_age_ms },
                    perf,
                    average_input_payload_bytes: payload.map(|p| p.0).unwrap_or(0.0),
                    average_output_payload_bytes: payload.map(|p| p.1).unwrap_or(0.0),
                    peak_input_payload_bytes: payload.map(|p| p.2).unwrap_or(0),
                    peak_output_payload_bytes: payload.map(|p| p.3).unwrap_or(0),
                    peak_payload_working_set_bytes: payload.map(|p| p.4).unwrap_or(0),
                    children: None,
                    node_type: info.map(|info| info.type_id.clone()),
                    node_label: info.and_then(|info| info.label.clone()),
                    node_index: Some(*node_idx as u64),
                    retained_output_sample_count: 0,
                    retained_output_sample_bytes: 0,
                    retained_output_ports: None,
                    last_error: info.and_then(|info| self.last_errors.get(&info.type_id)).map(|(_, message)| message.clone()),
                    last_error_at: info.and_then(|info| self.last_errors.get(&info.type_id)).map(|(instant, _)| now.saturating_duration_since(*instant).as_millis() as u64),
                },
            );
        }
        if !self.graph_samples.is_empty() {
            let deque = &self.graph_samples;
            let sample_count = deque.len() as u64;
            let sum_ms: f64 = deque.iter().map(|(_, ms)| *ms).sum();
            let average_time_ms = sum_ms / sample_count.max(1) as f64;
            let (first_t, _) = deque.front().copied().unwrap_or((now, 0.0));
            let (last_t, _) = deque.back().copied().unwrap_or((now, 0.0));
            let elapsed = last_t.saturating_duration_since(first_t).as_secs_f64();
            let average_fps = if elapsed > 0.000_001 && sample_count > 1 { (sample_count as f64 - 1.0) / elapsed } else { 0.0 };
            let last_sample_age_ms = Some(now.saturating_duration_since(last_t).as_millis() as u64);
            out.insert(
                "graph".to_string(),
                PipelineNodeRuntimeMetrics {
                    metrics: PipelineNodeMetrics { average_time_ms, average_fps, sample_count, window_size: self.window as u64, last_sample_age_ms },
                    perf: None,
                    average_input_payload_bytes: 0.0,
                    average_output_payload_bytes: 0.0,
                    peak_input_payload_bytes: 0,
                    peak_output_payload_bytes: 0,
                    peak_payload_working_set_bytes: 0,
                    children: None,
                    node_type: Some("graph".to_string()),
                    node_label: Some("graph".to_string()),
                    node_index: None,
                    retained_output_sample_count: 0,
                    retained_output_sample_bytes: 0,
                    retained_output_ports: None,
                    last_error: None,
                    last_error_at: None,
                },
            );
        }
        for (node_type, (instant, message)) in &self.last_errors {
            if out.contains_key(node_type) {
                continue;
            }
            let label = self.node_info.iter().find(|info| info.type_id == *node_type).and_then(|info| info.label.clone());
            out.insert(
                node_type.clone(),
                PipelineNodeRuntimeMetrics {
                    metrics: PipelineNodeMetrics { average_time_ms: 0.0, average_fps: 0.0, sample_count: 0, window_size: self.window as u64, last_sample_age_ms: None },
                    perf: None,
                    average_input_payload_bytes: 0.0,
                    average_output_payload_bytes: 0.0,
                    peak_input_payload_bytes: 0,
                    peak_output_payload_bytes: 0,
                    peak_payload_working_set_bytes: 0,
                    children: None,
                    node_type: Some(node_type.clone()),
                    node_label: label,
                    node_index: None,
                    retained_output_sample_count: 0,
                    retained_output_sample_bytes: 0,
                    retained_output_ports: None,
                    last_error: Some(message.clone()),
                    last_error_at: Some(now.saturating_duration_since(*instant).as_millis() as u64),
                },
            );
        }

        if let Some((instant, message)) = self.warnings.back() {
            let entry = out.entry("graph".to_string()).or_insert(PipelineNodeRuntimeMetrics {
                metrics: PipelineNodeMetrics { average_time_ms: 0.0, average_fps: 0.0, sample_count: 0, window_size: self.window as u64, last_sample_age_ms: None },
                perf: None,
                average_input_payload_bytes: 0.0,
                average_output_payload_bytes: 0.0,
                peak_input_payload_bytes: 0,
                peak_output_payload_bytes: 0,
                peak_payload_working_set_bytes: 0,
                children: None,
                node_type: Some("graph".to_string()),
                node_label: Some("graph".to_string()),
                node_index: None,
                retained_output_sample_count: 0,
                retained_output_sample_bytes: 0,
                retained_output_ports: None,
                last_error: None,
                last_error_at: None,
            });
            entry.last_error = Some(message.clone());
            entry.last_error_at = Some(now.saturating_duration_since(*instant).as_millis() as u64);
        }

        let mut group_entries: BTreeMap<String, PipelineNodeRuntimeMetrics> = BTreeMap::new();
        for (group_id, deque) in &self.group_samples {
            if deque.is_empty() {
                continue;
            }
            let sample_count = deque.len() as u64;
            let sum_ms: f64 = deque.iter().map(|(_, ms)| *ms).sum();
            let average_time_ms = sum_ms / sample_count.max(1) as f64;
            let (first_t, _) = deque.front().copied().unwrap_or((now, 0.0));
            let (last_t, _) = deque.back().copied().unwrap_or((now, 0.0));
            let elapsed = last_t.saturating_duration_since(first_t).as_secs_f64();
            let average_fps = if elapsed > 0.000_001 && sample_count > 1 { (sample_count as f64 - 1.0) / elapsed } else { 0.0 };
            let last_sample_age_ms = Some(now.saturating_duration_since(last_t).as_millis() as u64);
            let perf = self.group_perf_samples.get(group_id).and_then(|perf_deque| {
                if perf_deque.is_empty() {
                    return None;
                }
                let perf_sample_count = perf_deque.len() as u64;
                let sum_cache: f64 = perf_deque.iter().map(|sample| sample.cache_misses).sum();
                let sum_branch_inst: f64 = perf_deque.iter().map(|sample| sample.branch_instructions).sum();
                let sum_branch_miss: f64 = perf_deque.iter().map(|sample| sample.branch_misses).sum();
                let last_age_ms = self.group_perf_last_at.get(group_id).map(|instant| now.saturating_duration_since(*instant).as_millis() as u64);
                Some(PipelineNodePerfMetrics {
                    average_cache_misses: sum_cache / perf_sample_count.max(1) as f64,
                    average_branch_instructions: sum_branch_inst / perf_sample_count.max(1) as f64,
                    average_branch_misses: sum_branch_miss / perf_sample_count.max(1) as f64,
                    sample_count: perf_sample_count,
                    window_size: self.window as u64,
                    last_sample_age_ms: last_age_ms,
                })
            });
            let payload = self.group_payload_samples.get(group_id).map(&summarize_payload);
            group_entries.insert(
                group_id.clone(),
                PipelineNodeRuntimeMetrics {
                    metrics: PipelineNodeMetrics { average_time_ms, average_fps, sample_count, window_size: self.window as u64, last_sample_age_ms },
                    perf,
                    average_input_payload_bytes: payload.map(|p| p.0).unwrap_or(0.0),
                    average_output_payload_bytes: payload.map(|p| p.1).unwrap_or(0.0),
                    peak_input_payload_bytes: payload.map(|p| p.2).unwrap_or(0),
                    peak_output_payload_bytes: payload.map(|p| p.3).unwrap_or(0),
                    peak_payload_working_set_bytes: payload.map(|p| p.4).unwrap_or(0),
                    children: None,
                    node_type: Some("group".to_string()),
                    node_label: Some(group_id.clone()),
                    node_index: None,
                    retained_output_sample_count: 0,
                    retained_output_sample_bytes: 0,
                    retained_output_ports: None,
                    last_error: None,
                    last_error_at: None,
                },
            );
        }

        let mut grouped_nodes: BTreeMap<String, BTreeMap<String, PipelineNodeRuntimeMetrics>> = BTreeMap::new();
        for (node_key, metrics) in &out {
            let Some(node_index) = metrics.node_index else {
                continue;
            };
            let Some(info) = self.node_info.get(node_index as usize) else {
                continue;
            };
            let Some(group) = info.group.as_ref() else {
                continue;
            };
            grouped_nodes.entry(group.clone()).or_default().insert(node_key.clone(), metrics.clone());
        }

        for (group_id, children) in grouped_nodes {
            let entry = group_entries.entry(group_id.clone()).or_insert(PipelineNodeRuntimeMetrics {
                metrics: PipelineNodeMetrics { average_time_ms: 0.0, average_fps: 0.0, sample_count: 0, window_size: self.window as u64, last_sample_age_ms: None },
                perf: None,
                average_input_payload_bytes: 0.0,
                average_output_payload_bytes: 0.0,
                peak_input_payload_bytes: 0,
                peak_output_payload_bytes: 0,
                peak_payload_working_set_bytes: 0,
                children: None,
                node_type: Some("group".to_string()),
                node_label: Some(group_id.clone()),
                node_index: None,
                retained_output_sample_count: 0,
                retained_output_sample_bytes: 0,
                retained_output_ports: None,
                last_error: None,
                last_error_at: None,
            });
            entry.children = Some(children);
        }

        let mut root_groups: BTreeMap<String, PipelineNodeRuntimeMetrics> = BTreeMap::new();
        if !group_entries.is_empty() {
            let group_ids: BTreeSet<String> = group_entries.keys().cloned().collect();
            let mut child_groups: BTreeSet<String> = BTreeSet::new();
            for group_id in &group_ids {
                if let Some((parent, _)) = group_id.rsplit_once("::") {
                    if group_ids.contains(parent) {
                        child_groups.insert(group_id.clone());
                    }
                }
            }
            for child_id in &child_groups {
                if let Some((parent, _)) = child_id.rsplit_once("::") {
                    if let Some(child_entry) = group_entries.get(child_id).cloned() {
                        if let Some(parent_entry) = group_entries.get_mut(parent) {
                            parent_entry.children.get_or_insert_with(BTreeMap::new).insert(child_id.clone(), child_entry);
                        }
                    }
                }
            }
            for group_id in &group_ids {
                if !child_groups.contains(group_id) {
                    if let Some(entry) = group_entries.get(group_id).cloned() {
                        root_groups.insert(group_id.clone(), entry);
                    }
                }
            }
        }

        let mut edge_entries: BTreeMap<String, crate::stream::PipelineEdgeRuntimeMetrics> = BTreeMap::new();
        for (edge_idx, deque) in &self.edge_samples {
            if deque.is_empty() {
                continue;
            }

            let wait_sample_count: u64 = deque.iter().map(|metrics| metrics.samples as u64).sum();
            let total_wait_ms: f64 = deque.iter().map(|metrics| metrics.total_wait.as_secs_f64() * 1000.0).sum();
            let payload_count: u64 = deque.iter().map(|metrics| metrics.transport_count).sum();
            let payload_bytes: u64 = deque.iter().map(|metrics| metrics.transport_bytes).sum();
            let max_depth = deque.iter().map(|metrics| metrics.max_depth).max().unwrap_or(0);
            let dropped = deque.iter().map(|metrics| metrics.drops).sum();
            let gpu_uploads = deque.iter().map(|metrics| metrics.gpu_uploads).sum();
            let gpu_downloads = deque.iter().map(|metrics| metrics.gpu_downloads).sum();
            let current_depth = deque.back().map(|metrics| metrics.current_depth).unwrap_or(0);
            let average_wait_ms = if wait_sample_count > 0 { total_wait_ms / wait_sample_count as f64 } else { 0.0 };
            let average_payload_bytes = if payload_count > 0 { payload_bytes as f64 / payload_count as f64 } else { 0.0 };
            let last_sample_age_ms = self.edge_last_at.get(edge_idx).map(|instant| now.saturating_duration_since(*instant).as_millis() as u64);
            let info = self.edge_info.get(*edge_idx);
            let capacity = deque.iter().filter_map(|metrics| metrics.capacity).max().or_else(|| info.and_then(|edge| edge.queue_capacity));

            edge_entries.insert(
                format!("edge_{edge_idx}"),
                crate::stream::PipelineEdgeRuntimeMetrics {
                    average_wait_ms,
                    wait_sample_count,
                    window_size: self.window as u64,
                    last_sample_age_ms,
                    max_depth,
                    current_depth,
                    current_queue_bytes: deque.back().map(|metrics| metrics.current_queue_bytes).unwrap_or(0),
                    peak_queue_bytes: deque.iter().map(|metrics| metrics.peak_queue_bytes).max().unwrap_or(0),
                    capacity,
                    dropped,
                    payload_bytes,
                    payload_count,
                    average_payload_bytes,
                    gpu_uploads,
                    gpu_downloads,
                    edge_index: *edge_idx as u64,
                    from_node_index: info.map(|edge| edge.from_node_index as u64),
                    from_node_label: info.and_then(|edge| edge.from_node_label.clone()),
                    from_port: info.map(|edge| edge.from_port.clone()),
                    to_node_index: info.map(|edge| edge.to_node_index as u64),
                    to_node_label: info.and_then(|edge| edge.to_node_label.clone()),
                    to_port: info.map(|edge| edge.to_port.clone()),
                    policy: info.map(|edge| edge.policy.clone()),
                },
            );
        }

        let perf = if self.perf_samples.is_empty() {
            None
        } else {
            let sample_count = self.perf_samples.len() as u64;
            let sum_cache: f64 = self.perf_samples.iter().map(|sample| sample.cache_misses as f64).sum();
            let sum_branch_inst: f64 = self.perf_samples.iter().map(|sample| sample.branch_instructions as f64).sum();
            let sum_branch_miss: f64 = self.perf_samples.iter().map(|sample| sample.branch_misses as f64).sum();
            let last_age_ms = self.perf_last_at.map(|instant| now.saturating_duration_since(instant).as_millis() as u64);
            Some(PipelinePerfMetrics {
                average_cache_misses: sum_cache / sample_count.max(1) as f64,
                average_branch_instructions: sum_branch_inst / sample_count.max(1) as f64,
                average_branch_misses: sum_branch_miss / sample_count.max(1) as f64,
                sample_count,
                window_size: self.window as u64,
                last_sample_age_ms: last_age_ms,
            })
        };

        let flamegraph = self.last_flamegraph.as_ref().map(|capture| PipelineFlamegraphMetrics { path: capture.path.clone(), size_bytes: capture.size_bytes, captured_at_ms: capture.captured_at_ms });
        let warnings =
            self.warnings.iter().rev().filter(|(instant, _)| now.saturating_duration_since(*instant) <= Duration::from_secs(4)).map(|(_, warning)| warning.clone()).take(4).collect::<Vec<_>>();

        PipelineGraphMetrics {
            nodes: out,
            groups: if root_groups.is_empty() { None } else { Some(root_groups) },
            edges: if edge_entries.is_empty() { None } else { Some(edge_entries) },
            sample_cache: None,
            image_working_set: None,
            executor: {
                if self.graph_samples.is_empty() {
                    None
                } else {
                    let sample_count = self.graph_samples.len() as u64;
                    let sum_ms: f64 = self.graph_samples.iter().map(|(_, ms)| *ms).sum();
                    let average_time_ms = sum_ms / sample_count.max(1) as f64;
                    let (last_t, last_time_ms) = self.graph_samples.back().copied().unwrap_or((now, 0.0));
                    Some(crate::stream::PipelineTimingMetrics {
                        average_time_ms,
                        last_time_ms,
                        sample_count,
                        window_size: self.window as u64,
                        last_sample_age_ms: Some(now.saturating_duration_since(last_t).as_millis() as u64),
                    })
                }
            },
            wrapper: summarize_timing(&self.wrapper_samples, self.wrapper_last_at),
            executor_lock: summarize_timing(&self.lock_samples, self.lock_last_at),
            output_materialization: summarize_timing(&self.output_materialization_samples, self.output_materialization_last_at),
            perf,
            flamegraph,
            warnings,
        }
    }
}

pub(super) fn is_internal_runtime_warning(warning: &str) -> bool {
    let trimmed = warning.trim();
    // Daedalus may emit internal segment identifiers for GPU-preferred paths that
    // safely fallback to CPU. These are implementation details and should not be
    // surfaced as user-facing "Pipeline warning" banners.
    trimmed.starts_with("gpu_preferred_fallback_cpu_seg_")
}
