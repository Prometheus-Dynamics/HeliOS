use super::*;

impl DaedalusGraphExecutor {
    pub(super) fn pipeline_metrics_impl(&self) -> Option<PipelineGraphMetrics> {
        self.metrics.lock().ok().map(|metrics| {
            let mut snapshot = metrics.snapshot();
            snapshot.sample_cache = sample_cache_metrics(&self.image_samples, &self.value_samples, &self.typed_samples);
            snapshot.image_working_set = self.image_working_set.snapshot();
            annotate_retained_output_metrics(&mut snapshot, &self.host_output_port_owners);
            snapshot
        })
    }

    pub(super) fn sample_json_output_impl(&self, port: &str) -> Option<Value> {
        let key = port.to_ascii_lowercase();
        if let Some(value) = self.value_samples.lock().ok()?.get(&key).cloned() {
            return daedalus_value_to_json(&value);
        }
        self.typed_samples.lock().ok()?.get(&key).and_then(TypedHostOutputSample::to_json)
    }

    pub(super) fn sample_value_output_impl(&self, port: &str) -> Option<DaedalusValue> {
        let key = port.to_ascii_lowercase();
        if let Some(value) = self.value_samples.lock().ok()?.get(&key).cloned() {
            return Some(value);
        }
        self.typed_samples.lock().ok()?.get(&key).and_then(TypedHostOutputSample::to_daedalus_value)
    }

    pub(super) fn sample_image_output_impl(&self, port: &str) -> Option<DynamicImage> {
        let key = port.to_ascii_lowercase();
        self.image_samples.lock().ok()?.get(&key).cloned()
    }

    pub(super) fn disabled_state_impl(&self) -> GraphDisabledState {
        let disabled = self.disabled.load(Ordering::Relaxed);
        if !disabled {
            return GraphDisabledState::default();
        }
        let disabled_since_ms = self.disabled_since_ms.load(Ordering::Relaxed);
        let reason = self.last_error_detail.read().ok().map(|guard| guard.trim().to_string()).filter(|text| !text.is_empty());
        GraphDisabledState { disabled, disabled_since_ms: if disabled_since_ms == 0 { None } else { Some(disabled_since_ms) }, disabled_reason: reason }
    }

    pub(super) fn clear_disabled_impl(&self) {
        self.disabled.store(false, Ordering::Relaxed);
        self.disabled_since_ms.store(0, Ordering::Relaxed);
        self.failure_count.store(0, Ordering::Relaxed);
    }

    pub(super) fn set_perf_enabled_impl(&self, enabled: bool) {
        if !cfg!(all(feature = "perf-counters", target_os = "linux")) {
            self.perf_enabled.store(false, Ordering::Relaxed);
            return;
        }
        self.perf_enabled.store(enabled, Ordering::Relaxed);
    }

    pub(super) fn reset_pipeline_metrics_impl(&self) {
        if let Ok(mut metrics) = self.metrics.lock() {
            metrics.reset();
        }
    }

    pub(super) fn release_idle_retention_impl(&self) {
        if let Ok(mut metrics) = self.metrics.lock() {
            metrics.release_idle_retention();
        }
        self.image_working_set.clear_current();
        if let Ok(mut guard) = self.image_samples.lock() {
            guard.clear();
        }
        if let Ok(mut guard) = self.value_samples.lock() {
            guard.clear();
        }
        if let Ok(mut guard) = self.typed_samples.lock() {
            guard.clear();
        }
        if let Ok(mut guard) = self.requested_sample_ports.lock() {
            guard.clear();
        }
        if let Ok(exec) = self.executor.lock() {
            let _ = exec.on_idle();
        }
        for alias in &self.output_hosts {
            let Some(output_host) = self.host_mgr.handle(alias) else {
                continue;
            };
            for port in output_host.incoming_ports() {
                let _ = output_host.clear(port.name());
            }
        }
    }

    pub(super) fn capture_flamegraph_impl(&self, duration_ms: u64) -> Result<(), String> {
        if duration_ms == 0 {
            return Err("duration_ms must be > 0".into());
        }
        if !cfg!(feature = "pprof") {
            return Err("pprof feature not enabled".into());
        }

        if self.pprof_pending.swap(true, Ordering::Relaxed) {
            return Err("flamegraph capture already in progress".into());
        }
        self.pprof_remaining.store(0, Ordering::Relaxed);
        self.pprof_until_ms.store(now_ms().saturating_add(duration_ms), Ordering::Relaxed);
        Ok(())
    }
}
