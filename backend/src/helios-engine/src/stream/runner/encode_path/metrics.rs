use super::*;

impl StreamRunner {
    pub(in super::super) fn encoded_consumer_stale_ms() -> u64 {
        HELIOS_ENGINE_STREAM_RUNTIME_POLICY.resolve().encoded_consumer_stale_ms
    }

    pub(super) fn parse_proc_key_bytes(text: &str, key: &str) -> Option<u64> {
        text.lines().find_map(|line| {
            let trimmed = line.trim_start();
            if !trimmed.starts_with(key) {
                return None;
            }
            let value = trimmed[key.len()..].trim();
            let number = value.split_whitespace().next().and_then(|raw| raw.parse::<u64>().ok())?;
            if value.contains("kB") {
                Some(number.saturating_mul(1024))
            } else {
                Some(number)
            }
        })
    }

    fn process_memory_metrics() -> Option<StreamProcessMemoryMetrics> {
        let status = fs::read_to_string("/proc/self/status").ok()?;
        let rss_bytes = Self::parse_proc_key_bytes(&status, "VmRSS:")?;

        Some(StreamProcessMemoryMetrics {
            sampled_at_ms: Self::unix_now_ms(),
            rss_bytes,
            pss_bytes: fs::read_to_string("/proc/self/smaps_rollup").ok().and_then(|text| Self::parse_proc_key_bytes(&text, "Pss:")),
            rss_anon_bytes: Self::parse_proc_key_bytes(&status, "RssAnon:").unwrap_or(0),
            rss_file_bytes: Self::parse_proc_key_bytes(&status, "RssFile:").unwrap_or(0),
            rss_shmem_bytes: Self::parse_proc_key_bytes(&status, "RssShmem:").unwrap_or(0),
            vm_data_bytes: Self::parse_proc_key_bytes(&status, "VmData:").unwrap_or(0),
            swap_bytes: Self::parse_proc_key_bytes(&status, "VmSwap:").unwrap_or(0),
        })
    }

    fn styx_pool_metrics(stats: styx::core::buffer::BufferPoolStats) -> StreamBufferPoolMetrics {
        StreamBufferPoolMetrics {
            chunk_size_bytes: stats.chunk_size as u64,
            free_buffers: stats.free as u64,
            free_bytes: stats.free_bytes as u64,
            max_free_buffers: stats.max_free as u64,
            retained_buffers: stats.retained as u64,
            retained_bytes: stats.retained_bytes as u64,
            in_use_buffers: stats.in_use as u64,
            in_use_bytes: stats.in_use_bytes as u64,
            peak_in_use_buffers: stats.peak_in_use as u64,
            peak_in_use_bytes: stats.peak_in_use_bytes as u64,
            hits: stats.hits,
            misses: stats.misses,
            allocations: stats.allocations,
        }
    }

    fn styx_memory_metrics(&self) -> Option<StreamMemoryMetrics> {
        let process = Self::process_memory_metrics();
        let runner = Some(self.runner_memory.snapshot()).filter(|metrics| {
            metrics.current_decoded_frame_bytes > 0
                || metrics.peak_decoded_frame_bytes > 0
                || metrics.current_raw_clone_bytes > 0
                || metrics.peak_raw_clone_bytes > 0
                || metrics.current_processed_frame_bytes > 0
                || metrics.peak_processed_frame_bytes > 0
                || metrics.current_frame_working_set_bytes > 0
                || metrics.peak_frame_working_set_bytes > 0
        });
        let pool_stats = self.session.as_ref().and_then(|session| session.handle()).map(|handle| handle.memory_stats());
        let capture_queue = pool_stats.as_ref().and_then(|stats| stats.capture_queue.as_ref().map(|queue| StreamQueueMemoryMetrics { depth: queue.depth, capacity: queue.capacity }));
        let external_backings: Vec<StreamExternalBackingMetrics> = pool_stats
            .as_ref()
            .map(|stats| {
                stats
                    .external_backings
                    .iter()
                    .map(|backing| StreamExternalBackingMetrics {
                        label: backing.label.clone(),
                        current_buffers: backing.current_buffers,
                        current_bytes: backing.current_bytes,
                        peak_buffers: backing.peak_buffers,
                        peak_bytes: backing.peak_bytes,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let transform_pool = pool_stats.as_ref().and_then(|stats| stats.transform_pool.clone()).map(Self::styx_pool_metrics);
        let image_pool = pool_stats.as_ref().and_then(|stats| stats.image_pool.clone()).map(Self::styx_pool_metrics);
        let packed_pools: Vec<StreamPackedPoolMetrics> = pool_stats
            .as_ref()
            .map(|stats| stats.packed_pools.iter().map(|pool| StreamPackedPoolMetrics { min_len_bytes: pool.min_len as u64, pool: Self::styx_pool_metrics(pool.stats.clone()) }).collect())
            .unwrap_or_default();
        let staging_copy = pool_stats
            .as_ref()
            .and_then(|stats| stats.staging_copy.as_ref().map(|staging| StreamStagingCopyMetrics { copies: staging.copies, bytes: staging.bytes, peak_copy_bytes: staging.peak_copy_bytes }));

        if process.is_none()
            && runner.is_none()
            && capture_queue.is_none()
            && external_backings.is_empty()
            && transform_pool.is_none()
            && image_pool.is_none()
            && packed_pools.is_empty()
            && staging_copy.is_none()
        {
            return None;
        }

        Some(StreamMemoryMetrics { process, runner, capture_queue, external_backings, transform_pool, image_pool, packed_pools, staging_copy })
    }

    fn metrics_stale_base_ms() -> u64 {
        HELIOS_ENGINE_STREAM_RUNTIME_POLICY.resolve().metrics_stale_base_ms
    }

    fn stale_threshold_for_fps(fps: f64) -> Duration {
        let base = Self::metrics_stale_base_ms();
        let cadence_ms = if fps.is_finite() && fps > 0.0 { (4_000.0 / fps).round() as u64 } else { 0 };
        Duration::from_millis(base.max(cadence_ms).min(60_000))
    }

    fn mark_capture_metrics_stale(metrics: &mut crate::capture::CaptureStageMetrics, age: Duration) {
        metrics.fps = 0.0;
        metrics.average_time_ms = 0.0;
        metrics.last_time_ms = metrics.last_time_ms.max(age.as_secs_f64() * 1000.0);
    }

    fn mark_codec_metrics_stale(metrics: &mut CodecMetrics, age: Duration) {
        metrics.fps = 0.0;
        metrics.average_time_ms = 0.0;
        metrics.last_time_ms = metrics.last_time_ms.max(age.as_secs_f64() * 1000.0);
    }

    pub(in super::super) fn fourcc_code(fourcc: FourCc) -> String {
        String::from_utf8_lossy(&fourcc.to_u32().to_le_bytes()).to_string()
    }

    pub(in super::super) fn unix_now_ms() -> u64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_millis().min(u64::MAX as u128) as u64,
            Err(_) => 0,
        }
    }

    pub fn runtime_state(&self) -> crate::ipc::StreamRuntimeState {
        crate::ipc::StreamRuntimeState {
            capture: crate::ipc::StreamCaptureRuntimeState {
                state: if self.session.is_some() { crate::ipc::StreamCaptureState::Running } else { crate::ipc::StreamCaptureState::Stopped },
                started_at_ms: None,
                capture_fourcc: self.capture_fourcc.map(Self::fourcc_code),
                disabled_since_ms: None,
                disabled_reason: None,
            },
            codecs: crate::ipc::StreamCodecChainRuntimeState {
                capture_input_fourcc: self.capture_fourcc.map(Self::fourcc_code),
                decoder_impl: self.decoder_id.clone().and_then(|value| (!value.trim().is_empty()).then_some(value)),
                encoder_input_fourcc: self.encoder_input_fourcc.map(Self::fourcc_code),
                encoder_impl: self.encoder_impl.clone().and_then(|value| (!value.trim().is_empty()).then_some(value)),
                encoder_output_fourcc: self.encode_fourcc.map(Self::fourcc_code),
            },
            demand: self.last_demand_state_snapshot(),
            recording: crate::ipc::StreamRecordingRuntimeState::default(),
            pipeline: crate::ipc::StreamPipelineRuntimeState::default(),
        }
    }

    pub fn metrics(&self) -> StreamMetrics {
        let now = Instant::now();
        let mut capture = cadence_stage_to_capture_metrics(&self.capture_stats);
        let mut host = stage_to_capture_metrics(&self.graph.host().metrics());
        let mut graph_stage = stage_to_capture_metrics(&self.graph_stage_stats);

        if let Some(last_capture) = self.last_capture_wall {
            let age = now.saturating_duration_since(last_capture);
            if age >= Self::stale_threshold_for_fps(capture.fps) {
                Self::mark_capture_metrics_stale(&mut capture, age);
            }
            if age >= Self::stale_threshold_for_fps(host.fps) {
                Self::mark_capture_metrics_stale(&mut host, age);
            }
        }
        if let Some(last_graph) = self.last_graph_wall {
            let age = now.saturating_duration_since(last_graph);
            if age >= Self::stale_threshold_for_fps(graph_stage.fps) {
                Self::mark_capture_metrics_stale(&mut graph_stage, age);
            }
        }

        let pipeline = self.graph.pipeline_metrics();
        let pipeline_instances = self.graph.pipeline_metrics_by_pipeline();
        let memory = self.styx_memory_metrics();
        let demand_snapshot = self.last_demand_state_snapshot();
        let frame_demand = Some(demand_snapshot.frame.clone());
        let encoder_demand = Some(demand_snapshot.encoder.clone());
        let preview_transport = {
            let stats = match self.preview_transport_stats.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            stats.has_samples().then(|| stats.snapshot())
        };
        let mut encoder = if self.encode_fourcc.is_some() { Some(to_codec_metrics(&self.encoder_stats)) } else { None };
        if let Some(metrics) = encoder.as_mut() {
            let activity_ms = self.encoder_last_activity_ms.load(Ordering::Relaxed);
            if activity_ms > 0 {
                let age = Duration::from_millis(Self::unix_now_ms().saturating_sub(activity_ms));
                if age >= Self::stale_threshold_for_fps(metrics.fps) {
                    Self::mark_codec_metrics_stale(metrics, age);
                }
            } else if let Some(last_encode) = self.last_encode_wall {
                let age = now.saturating_duration_since(last_encode);
                if age >= Self::stale_threshold_for_fps(metrics.fps) {
                    Self::mark_codec_metrics_stale(metrics, age);
                }
            }
        }
        let mut decoder = Some(to_codec_metrics(&self.decoder_stats));
        if let Some(metrics) = decoder.as_mut() {
            if let Some(last_decode) = self.last_decode_wall {
                let age = now.saturating_duration_since(last_decode);
                if age >= Self::stale_threshold_for_fps(metrics.fps) {
                    Self::mark_codec_metrics_stale(metrics, age);
                }
            }
        }
        StreamMetrics { capture, host, graph_stage, encoder, preview_transport, encoder_demand, frame_demand, decoder, memory, pipeline, pipeline_instances }
    }

    pub(in super::super) fn stream_label(&self) -> &str {
        self.stream_label.as_ref()
    }

    pub(in super::super) fn is_encoded_preview_fourcc(fourcc: FourCc) -> bool {
        matches!(&fourcc.to_u32().to_le_bytes(), b"MJPG" | b"JPEG" | b"H264" | b"H265" | b"HEVC")
    }

    pub(in super::super) fn record_decode_ms(&self, decode_start: Instant) {
        self.decoder_stats.record_duration(decode_start.elapsed());
        let decode_ms = decode_start.elapsed().as_secs_f64() * 1000.0;
        histogram!("helios.stream.decode_ms", "stream" => self.stream_label.clone()).record(decode_ms);
    }
}
