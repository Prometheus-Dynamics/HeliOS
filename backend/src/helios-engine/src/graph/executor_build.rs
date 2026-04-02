use super::*;

impl DaedalusGraphExecutor {
    pub(super) fn new(graph: Graph, pool_size: Option<usize>, output_port: Option<String>) -> Result<Self, GraphError> {
        let mut graph = graph;
        let host_mgr = DaedalusBridgeManager::new();
        let built = build_daedalus_runtime_registry(&host_mgr, Some(&graph)).map_err(|e| GraphError::Build(e.to_string()))?;
        let (registry, handlers, _plugins) = built.into_parts();
        let const_coercers = registry.const_coercers.clone();
        let output_movers = registry.output_movers.clone();

        sync_graph_node_port_declarations(&mut graph, &registry);
        enforce_registry_default_compute_affinity(&mut graph, &registry);
        canonicalize_graph_const_inputs(&mut graph, &registry);
        if std::env::var_os("HELIOS_TRACE_GRAPH_CONSTS_STDERR").is_some() {
            for node in &graph.nodes {
                if node.id.0 == "cv:image:blur" || node.id.0 == "cv:color:grayscale" {
                    eprintln!("helios-engine: node consts id={} const_inputs={:?}", node.id.0, node.const_inputs);
                }
            }
        }

        let mut cfg = EngineConfig::default();
        apply_daedalus_engine_config_overrides(&mut cfg, &graph);
        let gpu_backend_overridden = graph.metadata.contains_key("helios.daedalus.gpu_backend");
        let planner_gpu_overridden = graph.metadata.contains_key("helios.daedalus.planner.enable_gpu");
        let runtime_policy_overridden = graph.metadata.contains_key(KEY_RUNTIME_DEFAULT_POLICY);
        let runtime_backpressure_overridden = graph.metadata.contains_key(KEY_RUNTIME_BACKPRESSURE);
        let graph_requests_gpu = graph.nodes.iter().any(|node| !matches!(node.compute, ComputeAffinity::CpuOnly));
        if graph_requests_gpu {
            if !gpu_backend_overridden {
                cfg.gpu = daedalus::engine::GpuBackend::Device;
            }
            if !planner_gpu_overridden {
                cfg.planner.enable_gpu = true;
            }
            if !runtime_policy_overridden {
                cfg.runtime.default_policy = EdgePolicyKind::Bounded { cap: policy::runtime_queue_cap() };
            }
            if !runtime_backpressure_overridden {
                cfg.runtime.backpressure = BackpressureStrategy::BoundedQueues;
            }
        }
        apply_daedalus_engine_env_overrides(&mut cfg);
        let engine = Engine::new(cfg).map_err(|e| GraphError::Build(e.to_string()))?;

        let graph_has_calibration = graph.nodes.iter().any(|node| {
            let id = node.id.0.as_str();
            if !(id == "io.host_bridge" || id.ends_with(":io.host_bridge")) {
                return false;
            }
            node.outputs.iter().any(|p| p.eq_ignore_ascii_case("calibration"))
        });

        let mut declared_host_bridge_ports: BTreeSet<String> = BTreeSet::new();
        for node in graph.nodes.iter().filter(|node| {
            let id = node.id.0.as_str();
            id == "io.host_bridge" || id.ends_with(":io.host_bridge")
        }) {
            for port in &node.outputs {
                let trimmed = port.trim();
                if trimmed.is_empty() {
                    continue;
                }
                declared_host_bridge_ports.insert(trimmed.to_string());
            }
        }

        // Host output ports are a graph contract: the authoritative port list comes from the graph
        // JSON node's declared `inputs` array (not from runtime inference).
        let mut declared_host_output_ports: Vec<String> = Vec::new();
        let mut declared_host_output_ports_lc: BTreeSet<String> = BTreeSet::new();
        for node in graph.nodes.iter().filter(|node| {
            let id = node.id.0.as_str();
            id == "io.host_output" || id.ends_with(":io.host_output")
        }) {
            for port in &node.inputs {
                let trimmed = port.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let lc = trimmed.to_ascii_lowercase();
                if declared_host_output_ports_lc.insert(lc) {
                    declared_host_output_ports.push(trimmed.to_string());
                }
            }
        }
        let graph_has_color_sensitive_nodes = graph.nodes.iter().any(|node| node_requires_color_input(node.id.0.as_str()));
        let graph_auto_target_roi_enabled = metadata_bool_flag(&graph.metadata, "helios.auto_target_roi").unwrap_or_else(policy::auto_target_roi_enabled);
        let planner_output = engine.plan(&registry.registry, graph).map_err(|e| GraphError::Build(e.to_string()))?;
        let runtime_plan = engine.build_runtime_plan(&planner_output.plan).map_err(|e| GraphError::Build(e.to_string()))?;
        host_mgr.populate_from_plan(&runtime_plan);
        // Ensure host-output incoming port types are available even when planner metadata is
        // incomplete for dynamic-input nodes. We can infer them from source-node output metadata.
        for (alias, incoming) in infer_host_output_incoming_types(&runtime_plan, &registry.registry) {
            host_mgr.register_port_types(alias, std::iter::empty::<(String, DaedalusTypeExpr)>(), incoming);
        }
        let (input_host_alias, input_port, output_hosts) = derive_host_aliases(&runtime_plan, &host_mgr)?;

        // The host bridge manager now owns solved types (from planning/runtime plan). Do not
        // accept any type hints from the user graph JSON; only use what the planner inferred.
        let mut calibration_port = host_mgr.handle(&input_host_alias).and_then(|handle| handle.outgoing_ports().find(|p| p.eq_ignore_ascii_case("calibration")).map(|p| p.to_string()));
        if calibration_port.is_none() && graph_has_calibration {
            calibration_port = Some("calibration".to_string());
        }

        // Solved types for declared host output ports (keyed by normalized name).
        let mut host_output_port_types: BTreeMap<String, DaedalusTypeExpr> = BTreeMap::new();
        for port in &declared_host_output_ports {
            let target = port.trim();
            if target.is_empty() {
                continue;
            }
            let mut found: Option<DaedalusTypeExpr> = None;
            for alias in &output_hosts {
                let Some(host) = host_mgr.handle(alias) else { continue };
                if let Some(p) = host.incoming_ports().find(|p| p.name().eq_ignore_ascii_case(target)) {
                    found = p.resolved_type().cloned();
                    break;
                }
            }
            if let Some(ty) = found {
                host_output_port_types.insert(target.to_ascii_lowercase(), ty);
            }
        }

        // Previewable image ports are those declared by the graph and solved as image payloads.
        let mut previewable: Vec<String> = Vec::new();
        for port in &declared_host_output_ports {
            let key = port.to_ascii_lowercase();
            if let Some(ty) = host_output_port_types.get(&key) {
                if is_image_payload(ty) {
                    previewable.push(port.clone());
                }
            }
        }

        // Default preview selection:
        // - selected output must be declared and must solve to an image payload.
        // - otherwise, use the first previewable port in graph-declared order (if any).
        let mut preview_ports: Vec<String> = Vec::new();
        if let Some(selected) = output_port {
            let selected_known = declared_host_output_ports.iter().any(|p| p.eq_ignore_ascii_case(&selected));
            if !selected_known {
                return Err(GraphError::Build(format!("selected output {:?} not found in graph-declared host outputs {:?}", selected, declared_host_output_ports)));
            }
            let key = selected.trim().to_ascii_lowercase();
            if let Some(ty) = host_output_port_types.get(&key) {
                if !is_image_payload(ty) {
                    return Err(GraphError::Build(format!("selected output {:?} is not an image payload (solved type {:?})", selected, ty)));
                }
            } else {
                // Runtime type propagation for host bridge outputs can be unavailable on cold-start
                // graphs. Allow the explicit output selection and rely on runtime decode attempts.
                tracing::warn!(selected_output = %selected, "selected output has unknown solved type; continuing with best-effort image decode");
            }
            let canonical = declared_host_output_ports.iter().find(|p| p.eq_ignore_ascii_case(&selected)).cloned().unwrap_or(selected);
            preview_ports = vec![canonical];
        } else if let Some(first) = previewable.first().cloned() {
            preview_ports = vec![first];
        }

        let host_output_ports = declared_host_output_ports;
        let host_output_ports_lc: BTreeSet<String> = host_output_ports.iter().map(|p| p.to_ascii_lowercase()).collect();
        let host_output_port_owners = infer_host_output_port_owners(&runtime_plan, &output_hosts);
        let prefers_grayscale_input = graph_prefers_grayscale_input(graph_has_color_sensitive_nodes, &preview_ports, &host_output_port_types);
        let plan = Arc::new(runtime_plan);
        let node_info: Vec<NodeInfo> = plan
            .nodes
            .iter()
            .map(|node| {
                let group = node.metadata.get("daedalus.embedded_group").and_then(|value| match value {
                    DaedalusValue::String(group) => {
                        let trimmed = group.as_ref().trim();
                        if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed.to_string())
                        }
                    }
                    _ => None,
                });
                NodeInfo { type_id: node.id.clone(), label: node.label.clone(), group }
            })
            .collect();
        let edge_info: Vec<EdgeInfo> = plan
            .edges
            .iter()
            .map(|(from_node, from_port, to_node, to_port, policy)| EdgeInfo {
                from_node_index: from_node.0,
                from_node_label: node_info.get(from_node.0).and_then(|info| info.label.clone()).or_else(|| node_info.get(from_node.0).map(|info| info.type_id.clone())),
                from_port: from_port.clone(),
                to_node_index: to_node.0,
                to_node_label: node_info.get(to_node.0).and_then(|info| info.label.clone()).or_else(|| node_info.get(to_node.0).map(|info| info.type_id.clone())),
                to_port: to_port.clone(),
                queue_capacity: match policy {
                    EdgePolicyKind::Bounded { cap } => Some(*cap as u64),
                    _ => None,
                },
                policy: format!("{policy:?}"),
            })
            .collect();

        let gpu = match engine.config().gpu {
            daedalus::engine::GpuBackend::Cpu => None,
            daedalus::engine::GpuBackend::Mock => {
                let opts = GpuOptions { preferred_backend: Some(GpuBackendKind::Mock), adapter_label: None, allow_software: true };
                match select_backend(&opts) {
                    Ok(handle) => {
                        tracing::info!(backend = ?handle.backend_kind(), adapter = %handle.adapter_info().name, skipped = ?handle.skipped_summary(), "daedalus gpu backend selected");
                        if handle.backend_kind() == GpuBackendKind::Noop {
                            return Err(GraphError::Build(format!("gpu backend requested but unavailable (mock); skipped={:?}", handle.skipped_summary())));
                        } else {
                            Some(handle)
                        }
                    }
                    Err(e) => {
                        return Err(GraphError::Build(format!("gpu backend init failed: {e}")));
                    }
                }
            }
            daedalus::engine::GpuBackend::Device => {
                let opts = GpuOptions { preferred_backend: Some(GpuBackendKind::Wgpu), adapter_label: None, allow_software: false };
                match select_backend(&opts) {
                    Ok(handle) => {
                        tracing::info!(backend = ?handle.backend_kind(), adapter = %handle.adapter_info().name, skipped = ?handle.skipped_summary(), "daedalus gpu backend selected");
                        if handle.backend_kind() == GpuBackendKind::Noop {
                            return Err(GraphError::Build(format!("gpu backend requested but unavailable; skipped={:?}", handle.skipped_summary())));
                        } else {
                            Some(handle)
                        }
                    }
                    Err(e) => {
                        return Err(GraphError::Build(format!("gpu backend init failed: {e}")));
                    }
                }
            }
        };

        if let Some(handle) = gpu.clone() {
            host_mgr.attach_gpu(handle);
        }

        let gpu_plan_active = gpu.is_some() && plan_uses_gpu(plan.as_ref());
        let host_outputs_in_graph = policy::host_outputs_in_graph_enabled(Some(plan.as_ref()), gpu_plan_active);
        let demand_driven = policy::demand_driven_enabled(Some(plan.as_ref()), gpu_plan_active);
        let run_metrics_level = engine.config().runtime.metrics_level;
        let roi_ports_present = declared_host_bridge_ports.contains("roi_x")
            && declared_host_bridge_ports.contains("roi_y")
            && declared_host_bridge_ports.contains("roi_w")
            && declared_host_bridge_ports.contains("roi_h");
        let auto_target_roi_source_port = if graph_auto_target_roi_enabled && roi_ports_present { host_output_detection_source_port(&host_output_ports) } else { None };
        let preview_aux_ports = auto_target_roi_source_port.iter().cloned().collect::<Vec<_>>();
        let active_nodes_with_image =
            build_demand_mask(plan.as_ref(), &output_hosts, &preview_ports, &host_output_ports, &host_output_port_types, &host_output_port_owners, demand_driven, true, true, &[]).map(Arc::new);
        let active_nodes_preview_only =
            build_demand_mask(plan.as_ref(), &output_hosts, &preview_ports, &host_output_ports, &host_output_port_types, &host_output_port_owners, demand_driven, true, false, &preview_aux_ports)
                .map(Arc::new);
        let active_nodes_without_image =
            build_demand_mask(plan.as_ref(), &output_hosts, &preview_ports, &host_output_ports, &host_output_port_types, &host_output_port_owners, demand_driven, false, true, &[]).map(Arc::new);
        let mut executor = DaedalusOwnedExecutor::new(plan.clone(), handlers.clone_arc())
            .with_host_bridges(host_mgr.clone())
            .with_const_coercers(const_coercers.clone())
            .with_output_movers(output_movers.clone())
            // Daedalus error-isolation: keep the graph running and surface errors via telemetry
            // instead of killing the whole run on the first failing node.
            .with_fail_fast(false)
            .with_metrics_level(run_metrics_level)
            // Host output execution can be moved "in graph" for responsiveness, but this changes
            // scheduling semantics and can cause missing outputs depending on executor ordering.
            // Keep it opt-in until Daedalus scheduling guarantees sink ordering.
            .with_host_outputs_in_graph(host_outputs_in_graph);

        let preview_ports_lc: BTreeSet<String> = preview_ports.iter().map(|p| p.to_ascii_lowercase()).collect();
        if let Some(mask) = active_nodes_with_image.clone() {
            executor = executor.with_active_nodes_mask(Some(mask));
        }
        if let Some(handle) = gpu.clone() {
            executor = executor.with_gpu(handle);
        }
        if let Some(size) = pool_size {
            executor = executor.with_pool_size(Some(size));
        }
        let dedicated_executor = policy::dedicated_executor();
        let busy_behavior = policy::executor_busy_behavior();
        let busy_timeout = policy::executor_busy_timeout();

        tracing::info!(
            input_host = %input_host_alias,
            input_port = %input_port,
            output_hosts = ?output_hosts,
            host_output_ports = ?host_output_ports,
            preview_ports = ?preview_ports,
            "daedalus graph: host ports configured"
        );

        let mut seeded_input_values: BTreeMap<String, DaedalusValue> = BTreeMap::new();
        for port in &declared_host_bridge_ports {
            if port.eq_ignore_ascii_case(&input_port) {
                continue;
            }
            if calibration_port.as_deref().is_some_and(|cal| port.eq_ignore_ascii_case(cal)) {
                continue;
            }
            let key = port.to_ascii_lowercase();
            let Some(default_value) = default_host_bridge_input_value(&key) else {
                continue;
            };
            seeded_input_values.insert(key, default_value);
        }

        let pprof_enabled = policy::pprof_enabled();
        let pprof_duration_ms = if pprof_enabled { policy::pprof_duration_ms() } else { None };
        let pprof_until_ms = pprof_duration_ms.and_then(|d| now_ms().checked_add(d)).unwrap_or(0);
        Ok(Self {
            plan,
            handlers,
            pool_size,
            const_coercers,
            output_movers,
            dedicated_executor,
            busy_behavior,
            busy_timeout,
            host_mgr,
            gpu,
            input_host_alias,
            input_port,
            calibration_port,
            declared_input_ports_lc: declared_host_bridge_ports.iter().map(|port| port.to_ascii_lowercase()).collect(),
            output_hosts,
            host_output_ports,
            host_output_ports_lc,
            host_output_port_types,
            host_output_port_owners,
            preview_ports,
            preview_ports_lc,
            preview_aux_ports: preview_aux_ports.clone(),
            prefers_grayscale_input,
            run_mode: engine.config().runtime.mode.clone(),
            run_metrics_level,
            active_nodes_with_image,
            active_nodes_preview_only,
            active_nodes_without_image,
            executor: Arc::new(std::sync::Mutex::new(executor)),
            metrics: Mutex::new(RollingGraphMetrics::new(node_metrics_window(run_metrics_level), node_info, edge_info)),
            value_samples: Mutex::new(BTreeMap::new()),
            typed_samples: Mutex::new(BTreeMap::new()),
            image_samples: Mutex::new(BTreeMap::new()),
            requested_sample_ports: Mutex::new(BTreeMap::new()),
            image_working_set: GraphImageWorkingSetTracker::default(),
            process_calls: AtomicU64::new(0),
            perf_enabled: AtomicBool::new(policy::perf_counters_enabled()),
            pprof_pending: AtomicBool::new(pprof_enabled),
            pprof_remaining: AtomicU64::new(if pprof_enabled && pprof_until_ms == 0 { policy::pprof_frames() } else { 0 }),
            pprof_until_ms: AtomicU64::new(pprof_until_ms),
            pprof_guard: Mutex::new(None),
            calibration_payload: std::sync::RwLock::new(calibration_to_daedalus_value(None)),
            default_input_values: seeded_input_values.clone(),
            input_values: std::sync::RwLock::new(seeded_input_values),
            auto_target_roi_source_port,
            auto_target_roi: Mutex::new(AutoTargetRoiState::default()),
            last_background_trim_ms: AtomicU64::new(0),
            last_error_detail: std::sync::RwLock::new(String::new()),
            failure_count: AtomicU64::new(0),
            disabled: AtomicBool::new(false),
            disabled_since_ms: AtomicU64::new(0),
            rebuild_requested: AtomicBool::new(false),
        })
    }

    pub(super) fn rebuild_shared_executor(&self) -> Result<(), String> {
        let gpu_plan_active = self.gpu.is_some() && plan_uses_gpu(self.plan.as_ref());
        let host_outputs_in_graph = policy::host_outputs_in_graph_enabled(Some(self.plan.as_ref()), gpu_plan_active);
        let mut executor = DaedalusOwnedExecutor::new(self.plan.clone(), self.handlers.clone_arc())
            .with_host_bridges(self.host_mgr.clone())
            .with_const_coercers(self.const_coercers.clone())
            .with_output_movers(self.output_movers.clone())
            .with_fail_fast(false)
            .with_metrics_level(self.run_metrics_level)
            .with_host_outputs_in_graph(host_outputs_in_graph);
        if let Some(mask) = self.active_nodes_with_image.clone() {
            executor = executor.with_active_nodes_mask(Some(mask));
        }
        if let Some(handle) = self.gpu.clone() {
            executor = executor.with_gpu(handle);
        }
        if let Some(size) = self.pool_size {
            executor = executor.with_pool_size(Some(size));
        }
        let mut guard = self.executor.lock().unwrap_or_else(PoisonError::into_inner);
        *guard = executor;
        Ok(())
    }
}

impl DaedalusGraphExecutor {
    pub(super) fn active_nodes_for_process_options(&self, options: GraphProcessOptions) -> Option<Arc<Vec<bool>>> {
        if options.preview_only {
            return self.active_nodes_preview_only.clone().or_else(|| self.active_nodes_with_image.clone());
        }
        if options.require_image_output {
            return self.active_nodes_with_image.clone();
        }
        self.active_nodes_without_image.clone().or_else(|| self.active_nodes_with_image.clone())
    }

    pub(super) fn maybe_trim_background_graph_allocators(&self, options: GraphProcessOptions) {
        let interval_ms = if options.require_image_output { policy::active_graph_trim_interval_ms() } else { policy::background_graph_trim_interval_ms() };
        if interval_ms == 0 {
            return;
        }
        let now = now_ms();
        let last = self.last_background_trim_ms.load(Ordering::Relaxed);
        if last != 0 && now.saturating_sub(last) < interval_ms {
            return;
        }
        self.last_background_trim_ms.store(now, Ordering::Relaxed);
        styx::codec::decoder::clear_packed_frame_pools_all_threads();
        lib_cv::compact_runtime_scratch_after_frame();
    }

    pub(super) fn normalize_host_output_port_key(&self, port: &str) -> Option<String> {
        let key = port.trim().to_ascii_lowercase();
        if key.is_empty() || !self.host_output_ports_lc.contains(&key) {
            return None;
        }
        Some(key)
    }

    pub(super) fn request_output_sample_retention(&self, port: &str) {
        let Some(key) = self.normalize_host_output_port_key(port) else {
            return;
        };
        if let Ok(mut guard) = self.requested_sample_ports.lock() {
            let requested_at_ms = now_ms();
            if policy::host_output_debug_enabled() {
                tracing::info!(
                    target: "helios_engine::graph",
                    port = %key,
                    requested_at_ms,
                    "host output sample retention requested"
                );
            }
            guard.insert(key, requested_at_ms);
        }
    }

    pub(super) fn active_requested_sample_ports(&self) -> BTreeSet<String> {
        let mut active = BTreeSet::new();
        let now = now_ms();
        let ttl_ms = policy::host_output_sample_ttl_ms();
        if let Ok(mut guard) = self.requested_sample_ports.lock() {
            guard.retain(|port, requested_at_ms| {
                let keep = now.saturating_sub(*requested_at_ms) <= ttl_ms;
                if keep {
                    active.insert(port.clone());
                }
                keep
            });
        }
        active
    }

    pub(super) fn prune_unrequested_output_samples(&self, requested_ports: &BTreeSet<String>) {
        // Structured outputs are retained as rolling last-sample state. Only image outputs are
        // aggressively pruned because they materially impact memory use.
        if let Ok(mut guard) = self.image_samples.lock() {
            guard.retain(|port, _| requested_ports.contains(port));
        }
    }
}
