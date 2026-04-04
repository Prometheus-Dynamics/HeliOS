use super::*;

impl DaedalusGraphExecutor {
    pub(super) fn push_process_host_inputs(
        &self,
        image: DynamicImage,
        options: GraphProcessOptions,
        requested_sample_ports: &BTreeSet<String>,
        gpu_plan_active: bool,
        input_dims: (u32, u32),
        call_idx: u64,
    ) -> Option<()> {
        let preview_only_port_reads = options.preview_only && requested_sample_ports.is_empty();
        for alias in &self.output_hosts {
            let Some(output_host) = self.host_mgr.handle(alias) else {
                continue;
            };
            if preview_only_port_reads {
                for port in &self.preview_ports {
                    let _ = output_host.clear(port);
                }
            } else {
                for port in output_host.incoming_port_names() {
                    let _ = output_host.clear(&port);
                }
            }
        }

        let input_host = self.host_mgr.handle(&self.input_host_alias)?;
        let pushed = if gpu_plan_active {
            DaedalusEdgePayload::Data(DataCell::from_cpu::<DynamicImage>(image))
        } else {
            // CPU-only graphs still enter through the host bridge via `DataCell` so typed
            // image decoding stays on the same path as GPU-capable graphs.
            DaedalusEdgePayload::Data(DataCell::from_cpu::<DynamicImage>(image))
        };
        let correlation_id = input_host.push(&self.input_port, pushed, None);
        if call_idx < 3 {
            tracing::debug!(call_idx, correlation_id, port = %self.input_port, "daedalus graph: pushed input");
        }
        let calibration_port = self.calibration_port.clone().or_else(|| input_host.outgoing_ports().find(|p| p.eq_ignore_ascii_case("calibration")).map(|p| p.to_string()));

        if let Some(port) = calibration_port.as_deref() {
            let payload = self.calibration_payload.read().ok().map(|guard| guard.clone()).unwrap_or_else(|| calibration_to_daedalus_value(None));
            let pushed = DaedalusEdgePayload::Value(payload);
            let _ = input_host.push(port, pushed, Some(correlation_id));
        }
        if let Ok(guard) = self.input_values.read() {
            let auto_roi_values = if manual_roi_override_active(&guard) {
                None
            } else {
                self.auto_target_roi
                    .lock()
                    .ok()
                    .and_then(|state| state.rect.or_else(|| (!state.ever_detected).then(|| bootstrap_auto_target_roi_rect(input_dims, &guard)).flatten()))
                    .map(|rect| [("roi_x", rect.x), ("roi_y", rect.y), ("roi_w", rect.w), ("roi_h", rect.h)])
            };
            let auto_max_quads = if auto_roi_values.is_some() && self.declared_input_ports_lc.contains("max_quads") && !guard.contains_key("max_quads") { Some(4i64) } else { None };
            for (port, value) in guard.iter() {
                if port.eq_ignore_ascii_case(&self.input_port) {
                    continue;
                }
                if let Some(cal_port) = calibration_port.as_deref() {
                    if port.eq_ignore_ascii_case(cal_port) {
                        continue;
                    }
                }
                if auto_roi_values.is_some() && is_roi_port(port) {
                    continue;
                }
                let pushed = DaedalusEdgePayload::Value(value.clone());
                let _ = input_host.push(port, pushed, Some(correlation_id));
            }
            if let Some(auto_roi_values) = auto_roi_values {
                for (port, value) in auto_roi_values {
                    let _ = input_host.push(port, DaedalusEdgePayload::Value(DaedalusValue::Int(value)), Some(correlation_id));
                }
            }
            if let Some(max_quads) = auto_max_quads {
                let _ = input_host.push("max_quads", DaedalusEdgePayload::Value(DaedalusValue::Int(max_quads)), Some(correlation_id));
            }
        }
        Some(())
    }
}
