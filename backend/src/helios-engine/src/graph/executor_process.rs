use super::*;

mod input_push;
mod state;

impl GraphExecutor for DaedalusGraphExecutor {
    fn set_calibration(&self, calibration: Option<crate::ipc::StreamCalibration>) {
        if let Ok(mut guard) = self.calibration_payload.write() {
            *guard = calibration_to_daedalus_value(calibration);
        }
    }

    fn set_pipeline_inputs(&self, _pipeline_id: Option<uuid::Uuid>, inputs: &BTreeMap<String, Option<DaedalusValue>>) {
        if inputs.is_empty() {
            return;
        }
        if let Ok(mut guard) = self.input_values.write() {
            for (port, value) in inputs {
                let key = port.trim().to_ascii_lowercase();
                if key.is_empty() {
                    continue;
                }
                if let Some(value) = value {
                    guard.insert(key, value.clone());
                } else if let Some(default_value) = self.default_input_values.get(&key) {
                    guard.insert(key, default_value.clone());
                } else {
                    guard.remove(&key);
                }
            }
        }
    }

    fn apply_graph_patch(&self, _pipeline_id: Option<uuid::Uuid>, patch: &GraphPatch) -> Option<PatchReport> {
        let guard = self.executor.lock().unwrap_or_else(PoisonError::into_inner);
        Some(guard.apply_patch(patch))
    }

    fn process(&self, image: DynamicImage) -> Option<DynamicImage> {
        self.process_with_options(image, GraphProcessOptions::default())
    }

    fn process_with_options(&self, image: DynamicImage, options: GraphProcessOptions) -> Option<DynamicImage> {
        self.process_preview_with_options(image, options).map(GraphPreviewOutput::into_dynamic_image)
    }

    fn process_preview_with_options(&self, image: DynamicImage, options: GraphProcessOptions) -> Option<GraphPreviewOutput> {
        let total_start = Instant::now();
        let call_idx = self.process_calls.fetch_add(1, Ordering::Relaxed);
        let requested_sample_ports = self.active_requested_sample_ports();
        if call_idx < 3 {
            tracing::debug!(call_idx, "daedalus graph: processing frame");
        }
        if self.disabled.load(Ordering::Relaxed) {
            let disabled_since = self.disabled_since_ms.load(Ordering::Relaxed);
            if call_idx < 3 || call_idx.is_multiple_of(120) {
                tracing::warn!(call_idx, disabled_since, "graph disabled after repeated errors; emitting error frame");
            }
            let detail = self.last_error_detail.read().ok().map(|guard| guard.trim().to_string()).filter(|text| !text.is_empty());
            self.image_working_set.record(dynamic_image_size_bytes(&image), 0, 0);
            return Some(GraphPreviewOutput::Image(error_frame_like(&image, "GRAPH DISABLED", detail.as_deref())));
        }
        if !self.dedicated_executor && self.rebuild_requested.swap(false, Ordering::Relaxed) {
            match self.rebuild_shared_executor() {
                Ok(()) => tracing::debug!(call_idx, "daedalus graph: rebuilt shared executor after failure"),
                Err(err) => {
                    self.rebuild_requested.store(true, Ordering::Relaxed);
                    tracing::warn!(call_idx, error = %err, "daedalus graph: failed to rebuild executor");
                }
            }
        }
        let gpu_plan_active = self.gpu.is_some() && plan_uses_gpu(self.plan.as_ref());
        let input_image_bytes = dynamic_image_size_bytes(&image);
        let input_dims = (image.width(), image.height());

        let perf_guard = if self.perf_enabled.load(Ordering::Relaxed) {
            match perf::PerfCounterGuard::start() {
                Ok(guard) => Some(guard),
                Err(err) => {
                    self.perf_enabled.store(false, Ordering::Relaxed);
                    tracing::warn!(error = %err, "perf counters unavailable; disabling");
                    None
                }
            }
        } else {
            None
        };

        if self.pprof_pending.load(Ordering::Relaxed) {
            if let Ok(mut guard_slot) = self.pprof_guard.lock() {
                if guard_slot.is_none() {
                    let path = flamegraph::build_flamegraph_path(call_idx);
                    match flamegraph::FlamegraphGuard::start(path) {
                        Ok(guard) => {
                            *guard_slot = Some(guard);
                        }
                        Err(err) => {
                            self.pprof_pending.store(false, Ordering::Relaxed);
                            tracing::warn!(error = %err, "flamegraph capture failed");
                        }
                    }
                }
            }
        }

        let mut lock_duration = Duration::default();
        let run_result: Result<(DaedalusExecutionTelemetry, Duration), String> = if self.dedicated_executor {
            self.push_process_host_inputs(image, options, &requested_sample_ports, gpu_plan_active, input_dims, call_idx)?;
            let host_outputs_in_graph = policy::host_outputs_in_graph_enabled(Some(self.plan.as_ref()), gpu_plan_active);
            let active_nodes = self.active_nodes_for_process_options(options);
            let mut exec = DaedalusOwnedExecutor::new(self.plan.clone(), self.handlers.clone_arc())
                .with_host_bridges(self.host_mgr.clone())
                .with_const_coercers(self.const_coercers.clone())
                .with_output_movers(self.output_movers.clone())
                .with_fail_fast(false)
                .with_metrics_level(self.run_metrics_level)
                .with_host_outputs_in_graph(host_outputs_in_graph)
                .with_active_nodes_mask(active_nodes);
            if let Some(handle) = self.gpu.clone() {
                exec = exec.with_gpu(handle);
            }
            if let Some(size) = self.pool_size {
                exec = exec.with_pool_size(Some(size));
            }
            let run_start = Instant::now();
            let result = catch_unwind(AssertUnwindSafe(|| match self.run_mode {
                RuntimeMode::Serial => exec.run_in_place().map(|telemetry| (telemetry, run_start.elapsed())),
                _ => exec.run_parallel_in_place().map(|telemetry| (telemetry, run_start.elapsed())),
            }));
            match result {
                Ok(result) => result.map_err(|err| format!("{err:?}")),
                Err(panic) => Err(format!("panic: {}", format_panic_message(&panic))),
            }
        } else {
            let lock_start = Instant::now();
            let mut exec = match self.busy_behavior {
                ExecutorBusyBehavior::Drop => match self.executor.try_lock() {
                    Ok(lock) => Some(lock),
                    Err(TryLockError::WouldBlock) => None,
                    Err(TryLockError::Poisoned(err)) => Some(err.into_inner()),
                },
                ExecutorBusyBehavior::Block => {
                    if let Some(timeout) = self.busy_timeout {
                        let deadline = Instant::now() + timeout;
                        loop {
                            match self.executor.try_lock() {
                                Ok(lock) => break Some(lock),
                                Err(TryLockError::WouldBlock) => {}
                                Err(TryLockError::Poisoned(err)) => break Some(err.into_inner()),
                            }
                            if Instant::now() >= deadline {
                                break None;
                            }
                            std::thread::sleep(Duration::from_millis(1));
                        }
                    } else {
                        // If the executor previously panicked while holding the lock, recover the
                        // inner executor and keep the stream alive (otherwise the preview freezes).
                        Some(self.executor.lock().unwrap_or_else(PoisonError::into_inner))
                    }
                }
            };

            lock_duration = lock_start.elapsed();
            let lock_ms = lock_duration.as_secs_f64() * 1000.0;
            histogram!("helios.stream.executor_lock_ms").record(lock_ms);

            let exec = exec.as_deref_mut()?;
            self.push_process_host_inputs(image, options, &requested_sample_ports, gpu_plan_active, input_dims, call_idx)?;
            exec.set_active_nodes_mask(self.active_nodes_for_process_options(options));

            let run_start = Instant::now();
            let result = catch_unwind(AssertUnwindSafe(|| match self.run_mode {
                RuntimeMode::Serial => exec.run_in_place().map(|telemetry| (telemetry, run_start.elapsed())),
                _ => exec.run_parallel_in_place().map(|telemetry| (telemetry, run_start.elapsed())),
            }));
            match result {
                Ok(result) => result.map_err(|err| format!("{err:?}")),
                Err(panic) => Err(format!("panic: {}", format_panic_message(&panic))),
            }
        };
        let (telemetry, run_duration) = match run_result {
            Ok(result) => result,
            Err(err_text) => {
                if call_idx < 3 || call_idx.is_multiple_of(120) {
                    tracing::warn!(call_idx, error = %err_text, "graph execution failed; emitting error frame");
                }
                let node_type = extract_node_type(&err_text);
                if let Ok(mut metrics) = self.metrics.lock() {
                    metrics.record_error(node_type.as_deref(), err_text.clone());
                }
                if !self.dedicated_executor {
                    self.rebuild_requested.store(true, Ordering::Relaxed);
                }
                let failures = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
                if failures >= GRAPH_ERROR_DISABLE_THRESHOLD {
                    self.failure_count.store(0, Ordering::Relaxed);
                    self.disabled.store(true, Ordering::Relaxed);
                    self.disabled_since_ms.store(now_ms(), Ordering::Relaxed);
                    tracing::warn!(call_idx, failures, "graph disabled after repeated errors; awaiting stream/graph update");
                }
                let detail = format_error_detail(&err_text, node_type.as_deref());
                if let Ok(mut guard) = self.last_error_detail.write() {
                    *guard = detail.clone();
                }
                self.image_working_set.record(input_image_bytes, 0, 0);
                return Some(GraphPreviewOutput::Image(error_frame(input_dims, "GRAPH ERROR", Some(&detail))));
            }
        };
        let perf_sample = perf_guard.and_then(|guard| guard.finish().ok());
        let flamegraph_capture = if self.pprof_pending.load(Ordering::Relaxed) {
            let mut capture = None;
            let now_wall_ms = now_ms();
            if let Ok(mut guard_slot) = self.pprof_guard.lock() {
                if guard_slot.is_some() {
                    let until_ms = self.pprof_until_ms.load(Ordering::Relaxed);
                    if until_ms > 0 {
                        if now_wall_ms >= until_ms {
                            if let Some(guard) = guard_slot.take() {
                                capture = guard.finish().ok();
                            }
                            self.pprof_pending.store(false, Ordering::Relaxed);
                            self.pprof_until_ms.store(0, Ordering::Relaxed);
                            self.pprof_remaining.store(0, Ordering::Relaxed);
                        }
                    } else {
                        let remaining = self.pprof_remaining.load(Ordering::Relaxed);
                        if remaining > 0 {
                            let next = remaining.saturating_sub(1);
                            self.pprof_remaining.store(next, Ordering::Relaxed);
                            if next == 0 {
                                if let Some(guard) = guard_slot.take() {
                                    capture = guard.finish().ok();
                                }
                                self.pprof_pending.store(false, Ordering::Relaxed);
                                self.pprof_until_ms.store(0, Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
            capture
        } else {
            None
        };
        self.failure_count.store(0, Ordering::Relaxed);
        if call_idx < 3 {
            tracing::debug!(call_idx, warnings = telemetry.warnings.len(), nodes = telemetry.node_metrics.len(), "daedalus graph: ran");
        }

        let output_materialization_start = Instant::now();
        let mut preview_output: Option<GraphPreviewOutput> = None;
        let mut preview_key: Option<String> = None;
        let mut image_updates: Vec<(String, DynamicImage)> = Vec::new();
        let mut value_updates: Vec<(String, DaedalusValue)> = Vec::new();
        let mut typed_updates: Vec<(String, TypedHostOutputSample)> = Vec::new();
        let mut popped_outputs = 0usize;
        let image_output_requested = options.require_image_output;
        let preview_only_port_reads = options.preview_only && requested_sample_ports.is_empty();

        for alias in &self.output_hosts {
            let Some(output_host) = self.host_mgr.handle(alias) else { continue };
            if call_idx < 3 {
                tracing::debug!(call_idx, host = %alias, ports = ?output_host.incoming_port_names(), "daedalus graph: output host ports");
            }
            let port_names: Vec<String> = if preview_only_port_reads {
                self.preview_ports.iter().chain(self.preview_aux_ports.iter()).cloned().collect::<BTreeSet<_>>().into_iter().collect()
            } else {
                output_host.incoming_port_names()
            };
            for port_name_owned in port_names {
                let mut ports = output_host.iter_ports(std::slice::from_ref(&port_name_owned));
                let Some(port) = ports.next() else { continue };
                let port_name = port.name();
                let port_lc = port_name.to_ascii_lowercase();
                if !self.host_output_ports_lc.contains(&port_lc) {
                    // The graph JSON contract is authoritative. If the runtime exposes extra
                    // ports (e.g. due to stale persisted graphs or dynamic nodes), drain+ignore.
                    let _ = output_host.clear(port_name);
                    continue;
                }
                let wants_preview = image_output_requested && self.preview_ports_lc.contains(&port_lc);
                let wants_retained_sample = requested_sample_ports.contains(&port_lc);
                if preview_only_port_reads && !wants_preview {
                    continue;
                }
                let wants_image_sample = wants_preview || wants_retained_sample;
                let direct_preview = wants_preview && !wants_retained_sample;
                let port_type = port.resolved_type();
                let prefers_grayscale_preview = direct_preview && preview_port_accepts_grayscale_input(&port_lc, &self.host_output_port_types);
                let is_image_type = port_type.map(is_image_payload).unwrap_or(false);
                let typed_image = is_image_type;
                if policy::host_output_debug_enabled() && (call_idx < 3 || call_idx.is_multiple_of(120)) {
                    tracing::info!(
                        target: "helios_engine::graph",
                        port = %port_name,
                        wants_preview,
                        wants_retained_sample,
                        typed_image,
                        resolved_type = ?port_type,
                        "host output port state"
                    );
                }
                if typed_image && !wants_image_sample {
                    popped_outputs += output_host.clear(port_name);
                    continue;
                }
                // Keep image samples only for the active preview/output path. Additional image
                // outputs can be surprisingly expensive because they force CPU materialization and
                // then stay resident in `image_samples`.
                //
                // Unresolved ports are common while type inference is catching up, but they should
                // not cause us to eagerly materialize large image payloads unless the caller
                // explicitly asked for that image.
                let unresolved_type = port_type.is_none();
                if typed_image || (unresolved_type && wants_image_sample) {
                    let mut image_popped = false;

                    if prefers_grayscale_preview {
                        match port.try_pop::<Compute<GrayImage>>() {
                            Ok(Some((_corr, payload))) => match payload {
                                Compute::Cpu(gray) => {
                                    popped_outputs += 1;
                                    if preview_key.is_none() && wants_preview {
                                        preview_key = Some(port_lc.clone());
                                    }
                                    preview_output = Some(GraphPreviewOutput::Gray(Arc::new(gray)));
                                    image_popped = true;
                                    if call_idx < 3 && wants_preview {
                                        tracing::debug!(call_idx, port = %port_name, "daedalus graph: pulled grayscale preview output");
                                    }
                                }
                                Compute::Gpu(handle) => {
                                    if let Some(gpu) = self.gpu.as_ref() {
                                        match <GrayImage as daedalus::gpu::DeviceBridge>::download(&handle, gpu) {
                                            Ok(gray) => {
                                                popped_outputs += 1;
                                                if preview_key.is_none() && wants_preview {
                                                    preview_key = Some(port_lc.clone());
                                                }
                                                preview_output = Some(GraphPreviewOutput::Gray(Arc::new(gray)));
                                                image_popped = true;
                                            }
                                            Err(err) => {
                                                if wants_preview || policy::host_output_debug_enabled() {
                                                    tracing::warn!(target: "helios_engine::graph", port = %port_name, error = ?err, "host output GPU gray decode failed");
                                                }
                                            }
                                        }
                                    }
                                }
                            },
                            Ok(None) => {}
                            Err(_err) => {}
                        }

                        if !image_popped {
                            if let Some((_corr, gray)) = port.try_pop_any_arc::<GrayImage>() {
                                popped_outputs += 1;
                                if preview_key.is_none() && wants_preview {
                                    preview_key = Some(port_lc.clone());
                                }
                                preview_output = Some(GraphPreviewOutput::Gray(gray));
                                image_popped = true;
                            }
                        }

                        if !image_popped {
                            if let Some((_corr, gray)) = port.try_pop_any::<GrayImage>() {
                                popped_outputs += 1;
                                if preview_key.is_none() && wants_preview {
                                    preview_key = Some(port_lc.clone());
                                }
                                preview_output = Some(GraphPreviewOutput::Gray(Arc::new(gray)));
                                image_popped = true;
                            }
                        }
                    }

                    if !image_popped {
                        match port.try_pop::<DynamicImage>() {
                            Ok(Some((_corr, img))) => {
                                popped_outputs += 1;
                                if preview_key.is_none() && wants_preview {
                                    preview_key = Some(port_lc.clone());
                                }
                                if direct_preview && preview_output.is_none() {
                                    preview_output = Some(normalize_preview_output_for_port(GraphPreviewOutput::Image(img), &port_lc, &self.host_output_port_types));
                                } else {
                                    image_updates.push((port_lc.clone(), img));
                                }
                                image_popped = true;
                                if call_idx < 3 && wants_preview {
                                    tracing::debug!(call_idx, port = %port_name, "daedalus graph: pulled preview output");
                                }
                            }
                            Ok(None) => {}
                            Err(_err) => {
                                // Many CV nodes in lib-cv use `Compute<DynamicImage>` so they can
                                // run under GPU/CPU affinity without forcing node authors to
                                // manually handle transfers. The host output bridge should be
                                // able to decode those payloads too.
                                match port.try_pop::<Compute<DynamicImage>>() {
                                    Ok(Some((_corr, payload))) => match payload {
                                        Compute::Cpu(img) => {
                                            popped_outputs += 1;
                                            if preview_key.is_none() && wants_preview {
                                                preview_key = Some(port_lc.clone());
                                            }
                                            if direct_preview && preview_output.is_none() {
                                                preview_output = Some(normalize_preview_output_for_port(GraphPreviewOutput::Image(img), &port_lc, &self.host_output_port_types));
                                            } else {
                                                image_updates.push((port_lc.clone(), img));
                                            }
                                            image_popped = true;
                                        }
                                        Compute::Gpu(handle) => {
                                            if let Some(gpu) = self.gpu.as_ref() {
                                                match <DynamicImage as daedalus::gpu::DeviceBridge>::download(&handle, gpu) {
                                                    Ok(img) => {
                                                        popped_outputs += 1;
                                                        if preview_key.is_none() && wants_preview {
                                                            preview_key = Some(port_lc.clone());
                                                        }
                                                        if direct_preview && preview_output.is_none() {
                                                            preview_output = Some(normalize_preview_output_for_port(GraphPreviewOutput::Image(img), &port_lc, &self.host_output_port_types));
                                                        } else {
                                                            image_updates.push((port_lc.clone(), img));
                                                        }
                                                        image_popped = true;
                                                    }
                                                    Err(err) => {
                                                        if wants_preview || policy::host_output_debug_enabled() {
                                                            tracing::warn!(target: "helios_engine::graph", port = %port_name, error = ?err, "host output GPU image download failed");
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    },
                                    Ok(None) => {}
                                    Err(_err) => {}
                                }

                                if !image_popped {
                                    match port.try_pop::<Compute<GrayImage>>() {
                                        Ok(Some((_corr, payload))) => match payload {
                                            Compute::Cpu(gray) => {
                                                popped_outputs += 1;
                                                if preview_key.is_none() && wants_preview {
                                                    preview_key = Some(port_lc.clone());
                                                }
                                                if direct_preview && preview_output.is_none() {
                                                    preview_output = Some(GraphPreviewOutput::Gray(Arc::new(gray)));
                                                } else {
                                                    image_updates.push((port_lc.clone(), DynamicImage::ImageLuma8(gray)));
                                                }
                                                image_popped = true;
                                                if call_idx < 3 && wants_preview {
                                                    tracing::debug!(call_idx, port = %port_name, "daedalus graph: pulled preview output");
                                                }
                                            }
                                            Compute::Gpu(handle) => {
                                                if let Some(gpu) = self.gpu.as_ref() {
                                                    match <GrayImage as daedalus::gpu::DeviceBridge>::download(&handle, gpu) {
                                                        Ok(gray) => {
                                                            popped_outputs += 1;
                                                            if preview_key.is_none() && wants_preview {
                                                                preview_key = Some(port_lc.clone());
                                                            }
                                                            if direct_preview && preview_output.is_none() {
                                                                preview_output = Some(GraphPreviewOutput::Gray(Arc::new(gray)));
                                                            } else {
                                                                image_updates.push((port_lc.clone(), DynamicImage::ImageLuma8(gray)));
                                                            }
                                                            image_popped = true;
                                                        }
                                                        Err(err) => {
                                                            if wants_preview || policy::host_output_debug_enabled() {
                                                                tracing::warn!(target: "helios_engine::graph", port = %port_name, error = ?err, "host output GPU gray decode failed");
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        },
                                        Ok(None) => {}
                                        Err(_err) => {}
                                    }
                                }

                                if !image_popped {
                                    if let Some((_corr, gray)) = port.try_pop_any::<GrayImage>() {
                                        popped_outputs += 1;
                                        if preview_key.is_none() && wants_preview {
                                            preview_key = Some(port_lc.clone());
                                        }
                                        if direct_preview && preview_output.is_none() {
                                            preview_output = Some(GraphPreviewOutput::Gray(Arc::new(gray)));
                                        } else {
                                            image_updates.push((port_lc.clone(), DynamicImage::ImageLuma8(gray)));
                                        }
                                        image_popped = true;
                                    }
                                }

                                if !image_popped {
                                    if let Some((_corr, gray)) = port.try_pop_any_arc::<GrayImage>() {
                                        popped_outputs += 1;
                                        if preview_key.is_none() && wants_preview {
                                            preview_key = Some(port_lc.clone());
                                        }
                                        if direct_preview && preview_output.is_none() {
                                            preview_output = Some(GraphPreviewOutput::Gray(gray));
                                        } else {
                                            image_updates.push((port_lc.clone(), DynamicImage::ImageLuma8(Arc::unwrap_or_clone(gray))));
                                        }
                                        image_popped = true;
                                    }
                                }
                            }
                        }
                    }

                    if image_popped {
                        continue;
                    }
                    if typed_image {
                        // Typed image outputs can be empty on this tick; no value decode fallback.
                        continue;
                    }
                }

                let retain_structured_output = should_retain_structured_output_port(&port_lc, &requested_sample_ports, self.auto_target_roi_source_port.as_deref(), !self.preview_ports.is_empty());
                if !retain_structured_output {
                    popped_outputs += output_host.clear(port_name);
                    continue;
                }

                // Structured-only graphs still retain their latest outputs because downstream
                // readers pull them synchronously after each process call. Stream graphs with
                // preview/image outputs only keep structured ports that are explicitly requested
                // or needed for auto-ROI tracking.

                if port_type.is_some_and(is_aruco_detections_payload) {
                    if let Some(raw) = port.try_pop_raw() {
                        if let Some(detections) = decode_runtime_value_as_aruco_detections(&raw.inner) {
                            popped_outputs += 1;
                            let port_name = port_lc.clone();
                            if call_idx < 3 {
                                tracing::debug!(
                                    call_idx,
                                    port = %port_name,
                                    len = detections.len(),
                                    "daedalus graph: captured typed detection output"
                                );
                            }
                            typed_updates.push((port_name, TypedHostOutputSample::ArucoDetections(detections)));
                            continue;
                        }
                        port.restore_raw(raw);
                    }
                }

                match port.try_pop::<daedalus::data::model::Value>() {
                    Ok(Some((_corr, value))) => {
                        popped_outputs += 1;
                        let port_name = port_lc.clone();
                        value_updates.push((port_name, value));
                        continue;
                    }
                    Ok(None) => {
                        if let Some(raw) = port.try_pop_raw() {
                            if let Some(value) = decode_runtime_value_fallback(&raw.inner, port_type) {
                                popped_outputs += 1;
                                let port_name = port_lc.clone();
                                value_updates.push((port_name, value));
                                continue;
                            }
                            port.restore_raw(raw);
                        }
                    }
                    Err(err) => {
                        if let Some(raw) = port.try_pop_raw() {
                            if let Some(value) = decode_runtime_value_fallback(&raw.inner, port_type) {
                                popped_outputs += 1;
                                let port_name = port_lc.clone();
                                value_updates.push((port_name, value));
                                continue;
                            }
                            port.restore_raw(raw);
                        }

                        if policy::host_output_debug_enabled() {
                            // Many ports are neither image nor value-like; ignore unless debugging.
                            tracing::warn!(target: "helios_engine::graph", port = %port_name, error = ?err, "host output value decode failed");
                        }
                    }
                }

                if unresolved_type && !wants_image_sample {
                    // If the port type is still unresolved and we did not recognize a structured
                    // value, drain anything left so image outputs do not accumulate or get
                    // repeatedly materialized on later ticks.
                    popped_outputs += output_host.clear(port_name);
                }
            }
        }

        if call_idx < 3 {
            tracing::debug!(call_idx, popped_outputs, "daedalus graph: output host pop count");
        }

        if !value_updates.is_empty() {
            if let Ok(mut guard) = self.value_samples.lock() {
                for (port, value) in value_updates {
                    guard.insert(port, value);
                }
            }
        }
        if !typed_updates.is_empty() {
            if let Some(source_port) = self.auto_target_roi_source_port.as_deref() {
                if let Some((_port, TypedHostOutputSample::ArucoDetections(detections))) = typed_updates.iter().find(|(port, _)| port == source_port) {
                    if let Ok(mut guard) = self.auto_target_roi.lock() {
                        let _ = update_auto_target_roi_state(&mut guard, input_dims, detections.as_ref().as_slice());
                    }
                }
            }
            if let Ok(mut guard) = self.typed_samples.lock() {
                for (port, value) in typed_updates {
                    guard.insert(port, value);
                }
            }
        }
        if !image_updates.is_empty() {
            // Select the preview image locally first so we can still render a frame even if the
            // sample cache lock is poisoned/unavailable.
            if preview_output.is_none() {
                if let Some(key) = preview_key.as_deref() {
                    if let Some(idx) = image_updates.iter().position(|(port, _)| port == key) {
                        let (_port, img) = image_updates.swap_remove(idx);
                        preview_output = Some(normalize_preview_output_for_port(GraphPreviewOutput::Image(img), key, &self.host_output_port_types));
                    }
                }
            }
            let host_output_image_bytes: u64 = image_updates.iter().map(|(_, image)| dynamic_image_size_bytes(image)).sum();
            let preview_image_bytes = preview_output.as_ref().map(GraphPreviewOutput::size_bytes).unwrap_or(0);
            self.image_working_set.record(input_image_bytes, host_output_image_bytes, preview_image_bytes);
            if let Ok(mut guard) = self.image_samples.lock() {
                for (port, value) in image_updates {
                    guard.insert(port, value);
                }
            }
        } else {
            let preview_image_bytes = preview_output.as_ref().map(GraphPreviewOutput::size_bytes).unwrap_or(0);
            self.image_working_set.record(input_image_bytes, 0, preview_image_bytes);
        }
        self.prune_unrequested_output_samples(&requested_sample_ports);

        let output_materialization_duration = output_materialization_start.elapsed();
        let wrapper_duration = total_start.elapsed().saturating_sub(run_duration);
        if let Ok(mut metrics) = self.metrics.lock() {
            metrics.record_graph_duration(run_duration);
            metrics.record_wrapper_duration(wrapper_duration);
            metrics.record_lock_duration(lock_duration);
            metrics.record_output_materialization_duration(output_materialization_duration);
            metrics.record_telemetry(&telemetry);
            if let Some(sample) = perf_sample {
                metrics.record_perf_sample(sample);
            }
            if let Some(capture) = flamegraph_capture {
                metrics.record_flamegraph(capture);
            }
        }

        if let Some(img) = preview_output {
            self.maybe_trim_background_graph_allocators(options);
            return Some(img);
        }
        if !image_output_requested {
            self.maybe_trim_background_graph_allocators(options);
            return None;
        }
        // If no output port produced a frame, report it and keep the last good preview frame.
        if call_idx < 3 || call_idx.is_multiple_of(120) {
            tracing::warn!(call_idx, "graph produced no output; falling back to last preview");
        }
        if let Ok(mut metrics) = self.metrics.lock() {
            metrics.record_warning(format!("graph output missing: selected ports {:?} produced no frames", self.preview_ports));
        }
        self.maybe_trim_background_graph_allocators(options);
        None
    }

    fn pipeline_metrics(&self) -> Option<PipelineGraphMetrics> {
        self.pipeline_metrics_impl()
    }

    fn host_output_ports(&self) -> Option<Vec<String>> {
        Some(self.host_output_ports.clone())
    }

    fn request_output_sample(&self, port: &str) {
        self.request_output_sample_retention(port);
    }

    fn has_output_sample_demand(&self) -> bool {
        !self.active_requested_sample_ports().is_empty()
    }

    fn has_image_output(&self) -> bool {
        !self.preview_ports.is_empty()
    }

    fn prefers_grayscale_input(&self) -> bool {
        self.prefers_grayscale_input
    }

    fn host_output_port_types(&self) -> Option<BTreeMap<String, DaedalusTypeExpr>> {
        Some(self.host_output_port_types.clone())
    }

    fn sample_json_output(&self, port: &str) -> Option<Value> {
        self.sample_json_output_impl(port)
    }

    fn sample_value_output(&self, port: &str) -> Option<DaedalusValue> {
        self.sample_value_output_impl(port)
    }

    fn sample_image_output(&self, port: &str) -> Option<DynamicImage> {
        self.sample_image_output_impl(port)
    }

    fn disabled_state(&self) -> GraphDisabledState {
        self.disabled_state_impl()
    }

    fn clear_disabled(&self) {
        self.clear_disabled_impl();
    }

    fn set_perf_enabled(&self, _pipeline_id: Option<uuid::Uuid>, enabled: bool) {
        self.set_perf_enabled_impl(enabled);
    }

    fn reset_pipeline_metrics(&self, _pipeline_id: Option<uuid::Uuid>) {
        self.reset_pipeline_metrics_impl();
    }

    fn release_idle_retention(&self) {
        self.release_idle_retention_impl();
    }

    fn capture_flamegraph(&self, _pipeline_id: Option<uuid::Uuid>, duration_ms: u64) -> Result<(), String> {
        self.capture_flamegraph_impl(duration_ms)
    }
}
