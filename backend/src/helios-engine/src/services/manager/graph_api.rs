use super::*;

async fn build_manifest_graph(host_buffer: usize, manifest: crate::ipc::ResolvedStreamConfig, selected_output: Option<String>) -> Result<crate::graph::GraphHandle> {
    tokio::task::spawn_blocking(move || {
        crate::graph::build_graph_handle_for_manifest(host_buffer, &manifest, selected_output.as_deref()).map_err(|err| Error::InvalidStateOwned(format!("pipeline graph invalid: {err}")))
    })
    .await
    .map_err(|err| Error::InvalidStateOwned(format!("pipeline graph build failed: {err}")))?
}

impl StreamManager {
    pub async fn set_calibration(&self, stream_id: Uuid, calibration: Option<crate::ipc::StreamCalibration>) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;

        {
            let mut manifest = ctx.manifest.write().await;
            manifest.calibration = calibration.clone();
        }

        {
            let host = ctx.host.read().await;
            host.set_calibration(calibration.clone());
        }

        self.send_command(stream_id, move |respond_to| StreamCommand::SetCalibration { calibration, respond_to }).await
    }

    pub async fn set_pipeline_inputs(&self, stream_id: Uuid, pipeline_id: Option<Uuid>, inputs: std::collections::BTreeMap<String, Option<serde_json::Value>>) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;
        let mut normalized_inputs: std::collections::BTreeMap<String, Option<serde_json::Value>> = std::collections::BTreeMap::new();
        for (raw_key, value) in inputs {
            let key = raw_key.trim().to_ascii_lowercase();
            if key.is_empty() {
                continue;
            }
            normalized_inputs.insert(key, value);
        }
        if normalized_inputs.is_empty() {
            return Ok(());
        }

        {
            let host = ctx.host.read().await;
            host.set_pipeline_inputs(pipeline_id, &normalized_inputs);
        }

        self.send_command(stream_id, {
            let command_inputs = normalized_inputs.clone();
            move |respond_to| StreamCommand::SetPipelineInputs { pipeline_id, inputs: command_inputs, respond_to }
        })
        .await?;

        {
            let mut manifest = ctx.manifest.write().await;
            for (key, value) in normalized_inputs {
                if let Some(value) = value {
                    manifest.pipeline_host_inputs.insert(key, JsonWire(value));
                } else {
                    manifest.pipeline_host_inputs.remove(&key);
                }
            }
        }

        Ok(())
    }

    pub async fn set_calibration_mode(&self, stream_id: Uuid, enabled: bool, dictionary: Option<String>, mode: Option<String>) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;

        if enabled {
            tracing::info!(
                stream_id = %stream_id,
                enabled,
                mode = mode.as_deref().unwrap_or("calibration"),
                "calibration mode requested"
            );
            {
                let mut restore = ctx.calibration_mode_restore.write().await;
                if restore.is_none() {
                    let mut manifest = ctx.manifest.read().await.clone();
                    reconcile_manifest_pipeline_references(&mut manifest);
                    *restore = Some(CalibrationModeRestore::from_manifest(&manifest));
                }
            }

            let manifest_snapshot = ctx.manifest.read().await.clone();
            let host_buffer = calibration_mode_host_buffer(manifest_snapshot.host_buffer());
            let calibration = manifest_snapshot.calibration.clone();

            let mode = mode.unwrap_or_else(|| "calibration".to_string());
            let template_id = if mode.eq_ignore_ascii_case("undistort") || mode.eq_ignore_ascii_case("undistorted") { UNDISTORT_TEMPLATE_ID } else { CALIBRATION_TEMPLATE_ID };
            let output_port = calibration_mode_output_port(template_id);
            let mut graph_json = if template_id == CALIBRATION_TEMPLATE_ID {
                load_calibration_mode_graph_json().map_err(|err| Error::InvalidStateOwned(format!("calibration template missing: {err}")))?
            } else {
                crate::pipelines::load_template_graph_json(template_id).map_err(|err| Error::InvalidStateOwned(format!("calibration template missing: {err}")))?
            };
            if template_id == CALIBRATION_TEMPLATE_ID {
                let mut selected_dictionary: Option<String> = None;
                if let Some(raw_dictionary) = dictionary.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                    let Some(dictionary) = normalize_calibration_dictionary_name(raw_dictionary) else {
                        return Err(Error::InvalidStateOwned(format!("unknown ArUco dictionary '{raw_dictionary}'")));
                    };
                    patch_dictionary_const(&mut graph_json, &dictionary);
                    selected_dictionary = Some(dictionary);
                }
                patch_calibration_mode_detection_strictness(&mut graph_json, selected_dictionary.as_deref());
                ensure_calibration_mode_frame_output(&mut graph_json);
                ensure_calibration_mode_detections_json_output(&mut graph_json);
            }

            tracing::info!(
                stream_id = %stream_id,
                template_id,
                "calibration mode building graph"
            );

            let mut graph_json_for_build = graph_json.clone();
            let stream_alias = manifest_snapshot.identity.alias.as_deref().map(|v| crate::graph::context::sanitize_segment(v, "stream")).unwrap_or_else(|| "stream".to_string());
            crate::graph::context::inject_node_context(&mut graph_json_for_build, &stream_alias, "calibration");

            let mut manifest_for_build = manifest_snapshot.clone();
            manifest_for_build.pipeline_enabled = true;
            manifest_for_build.pipelines = vec![crate::ipc::StreamPipelineBinding {
                pipeline_id: CALIBRATION_MODE_PIPELINE_UUID,
                pipeline_graph: Some(crate::ipc::JsonWire(graph_json_for_build.clone())),
                pipeline_output: Some(output_port.to_string()),
                pipeline_patch: None,
            }];
            manifest_for_build.active_pipeline_id = Some(CALIBRATION_MODE_PIPELINE_UUID);
            manifest_for_build.active_pipeline_output = Some(output_port.to_string());
            manifest_for_build.pipeline_layout = None;
            manifest_for_build.pipeline_wires = Vec::new();

            let graph = tokio::task::spawn_blocking(move || {
                let graph = crate::graph::build_graph_handle_for_manifest(host_buffer, &manifest_for_build, Some(output_port))
                    .map_err(|err| Error::InvalidStateOwned(format!("pipeline graph invalid: {err}")))?;
                graph.set_calibration(calibration);
                Ok::<_, Error>(graph)
            })
            .await
            .map_err(|err| Error::InvalidStateOwned(format!("pipeline graph build failed: {err}")))??;
            tracing::info!(
                stream_id = %stream_id,
                outputs = ?graph.host_output_ports().unwrap_or_default(),
                "calibration mode graph built"
            );

            let (tx, rx) = oneshot::channel();
            enqueue_stream_command(&ctx.command_tx, StreamCommand::SetGraph { graph: graph.clone(), respond_to: tx })?;
            rx.await.map_err(|_| Error::InvalidState("stream worker dropped response"))??;
            *ctx.host.write().await = graph;
            let outputs = {
                let host = ctx.host.read().await;
                host.host_output_ports().unwrap_or_default()
            };
            tracing::info!(
                stream_id = %stream_id,
                outputs = ?outputs,
                "calibration mode graph applied"
            );

            let mut manifest = ctx.manifest.write().await;
            manifest.pipeline_enabled = true;
            manifest.pipelines = vec![crate::ipc::StreamPipelineBinding {
                pipeline_id: CALIBRATION_MODE_PIPELINE_UUID,
                pipeline_graph: Some(crate::ipc::JsonWire(graph_json)),
                pipeline_output: Some(output_port.to_string()),
                pipeline_patch: None,
            }];
            manifest.active_pipeline_id = Some(CALIBRATION_MODE_PIPELINE_UUID);
            manifest.active_pipeline_output = Some(output_port.to_string());
            manifest.pipeline_layout = None;
            manifest.pipeline_wires = Vec::new();
            return Ok(());
        }

        let restore = ctx.calibration_mode_restore.write().await.take();
        let Some(restore) = restore else {
            return Ok(());
        };

        let manifest_snapshot = {
            let mut manifest = ctx.manifest.write().await;
            restore.apply_to_manifest(&mut manifest);
            reconcile_manifest_pipeline_references(&mut manifest);
            manifest.clone()
        };

        let graph = build_manifest_graph(manifest_snapshot.host_buffer(), manifest_snapshot, None).await?;

        let (tx, rx) = oneshot::channel();
        enqueue_stream_command(&ctx.command_tx, StreamCommand::SetGraph { graph: graph.clone(), respond_to: tx })?;
        rx.await.map_err(|_| Error::InvalidState("stream worker dropped response"))??;
        *ctx.host.write().await = graph;
        Ok(())
    }

    pub async fn subscribe_frames(&self, stream_id: Uuid) -> Result<Receiver<Arc<image::DynamicImage>>> {
        let ctx = self.get_stream(stream_id).await?;
        let host = ctx.host.read().await;
        Ok(host.subscribe())
    }

    pub async fn subscribe_encoded(&self, stream_id: Uuid) -> Result<Receiver<EncodedFrame>> {
        let ctx = self.get_stream(stream_id).await?;
        Ok(ctx.encoded_tx.subscribe())
    }

    pub async fn set_graph_output(&self, stream_id: Uuid, output: Option<String>) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;

        let mut manifest_snapshot = ctx.manifest.read().await.clone();
        let host_buffer = manifest_snapshot.host_buffer();
        let active_pipeline_id = manifest_snapshot.active_pipeline_id.or_else(|| manifest_snapshot.pipelines.first().map(|p| p.pipeline_id));
        let view_pipeline_id = single_view_slot_pipeline_id(&manifest_snapshot).or(active_pipeline_id);
        let output_targets_active_pipeline = view_pipeline_id == active_pipeline_id;
        let effective_view_pipeline_id = if view_pipeline_id.is_none() && manifest_snapshot.pipelines.is_empty() { Some(RAW_STREAM_PIPELINE_UUID) } else { view_pipeline_id };
        let normalized_output = normalize_output_for_pipeline(output.clone(), effective_view_pipeline_id).map_err(|err| Error::InvalidStateOwned(format!("invalid pipeline output: {err}")))?;

        let wants_output = normalized_output.as_deref().map(str::trim).filter(|v| !v.is_empty());
        let wants_non_default_raw_output = wants_output.is_some_and(|v| !v.eq_ignore_ascii_case("raw"));
        let implicit_raw_view = view_pipeline_id.is_none() && manifest_snapshot.pipelines.is_empty();
        let wants_raw_graph = (effective_view_pipeline_id == Some(RAW_STREAM_PIPELINE_UUID) || implicit_raw_view) && wants_non_default_raw_output;
        if !manifest_snapshot.pipeline_enabled && wants_raw_graph {
            manifest_snapshot.pipeline_enabled = true;
        }
        if output_targets_active_pipeline {
            manifest_snapshot.active_pipeline_output = normalized_output.clone();
        }
        if let Some(target_id) = view_pipeline_id {
            if let Some(binding) = manifest_snapshot.pipelines.iter_mut().find(|p| p.pipeline_id == target_id) {
                binding.pipeline_output = normalized_output.clone();
            }
        }
        if let Some(layout) = manifest_snapshot.pipeline_layout.as_mut() {
            if layout.rows == 1 && layout.columns == 1 {
                if let Some(slot) = layout.slots.iter_mut().find(|slot| slot.row == 0 && slot.column == 0) {
                    slot.output_key = normalized_output.clone();
                }
            }
        }

        let selected_output = if output_targets_active_pipeline { normalized_output.clone() } else { manifest_snapshot.active_pipeline_output.clone() };
        let graph = build_manifest_graph(host_buffer, manifest_snapshot.clone(), selected_output.clone()).await?;

        let (tx, rx) = oneshot::channel();
        enqueue_stream_command(&ctx.command_tx, StreamCommand::SetGraph { graph: graph.clone(), respond_to: tx })?;
        rx.await.map_err(|_| Error::InvalidState("stream worker dropped response"))??;

        *ctx.host.write().await = graph;

        {
            let mut manifest = ctx.manifest.write().await;
            if !manifest.pipeline_enabled && wants_raw_graph {
                manifest.pipeline_enabled = true;
            }
            if output_targets_active_pipeline {
                manifest.active_pipeline_output = selected_output.clone();
            }
            if let Some(active_id) = manifest.active_pipeline_id.or_else(|| manifest.pipelines.first().map(|p| p.pipeline_id)) {
                manifest.active_pipeline_id = Some(active_id);
                if output_targets_active_pipeline {
                    if let Some(binding) = manifest.pipelines.iter_mut().find(|p| p.pipeline_id == active_id) {
                        binding.pipeline_output = selected_output.clone();
                    }
                }
            }
            let applied_view_output = if output_targets_active_pipeline { selected_output.clone() } else { normalized_output.clone() };
            if let Some(target_id) = view_pipeline_id {
                if let Some(binding) = manifest.pipelines.iter_mut().find(|p| p.pipeline_id == target_id) {
                    binding.pipeline_output = applied_view_output.clone();
                }
            }

            if let Some(layout) = manifest.pipeline_layout.as_mut() {
                if layout.rows == 1 && layout.columns == 1 {
                    if let Some(slot) = layout.slots.iter_mut().find(|slot| slot.row == 0 && slot.column == 0) {
                        slot.output_key = applied_view_output.clone();
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn set_graph(&self, stream_id: Uuid, graph_json: serde_json::Value, pipeline_id: Option<Uuid>, output: Option<String>) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;
        let graph_wire = crate::ipc::JsonWire(graph_json);

        let mut manifest_snapshot = ctx.manifest.read().await.clone();
        manifest_snapshot.pipeline_enabled = true;

        let target_pipeline_id = match pipeline_id {
            Some(id) => id,
            None => {
                return Err(Error::InvalidState("pipeline_id is required when setting a stream pipeline"));
            }
        };
        let graph_payload = Some(graph_wire.clone());

        let matching_count = manifest_snapshot.pipelines.iter().filter(|binding| binding.pipeline_id == target_pipeline_id).count();
        let mut updated = false;
        for binding in &mut manifest_snapshot.pipelines {
            if binding.pipeline_id == target_pipeline_id {
                binding.pipeline_graph = graph_payload.clone();
                binding.pipeline_patch = None;
                if output.is_some() && matching_count <= 1 {
                    binding.pipeline_output = output.clone();
                }
                updated = true;
            }
        }
        if !updated {
            manifest_snapshot.pipelines.push(crate::ipc::StreamPipelineBinding {
                pipeline_id: target_pipeline_id,
                pipeline_graph: graph_payload,
                pipeline_output: output.clone(),
                pipeline_patch: None,
            });
        }
        if manifest_snapshot.active_pipeline_id.is_none() {
            manifest_snapshot.active_pipeline_id = Some(target_pipeline_id);
        }
        if output.is_some() && manifest_snapshot.active_pipeline_id == Some(target_pipeline_id) {
            manifest_snapshot.active_pipeline_output = output.clone();
        }

        let selected_output = if manifest_snapshot.active_pipeline_id == Some(target_pipeline_id) { output.clone() } else { None };
        let graph = build_manifest_graph(manifest_snapshot.host_buffer(), manifest_snapshot.clone(), selected_output).await?;

        let (tx, rx) = oneshot::channel();
        enqueue_stream_command(&ctx.command_tx, StreamCommand::SetGraph { graph: graph.clone(), respond_to: tx })?;
        rx.await.map_err(|_| Error::InvalidState("stream worker dropped response"))??;

        *ctx.host.write().await = graph;
        *ctx.manifest.write().await = manifest_snapshot;

        Ok(())
    }

    pub async fn apply_graph_patch(&self, stream_id: Uuid, patch_json: serde_json::Value, pipeline_id: Option<Uuid>) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;
        let patch: GraphPatch = serde_json::from_value(patch_json.clone()).map_err(|err| Error::InvalidStateOwned(format!("invalid graph patch: {err}")))?;

        let mut manifest_snapshot = ctx.manifest.read().await.clone();
        manifest_snapshot.pipeline_enabled = true;

        let target_pipeline_id = match pipeline_id {
            Some(id) => id,
            None => {
                return Err(Error::InvalidState("pipeline_id is required when applying a stream patch"));
            }
        };

        let patch_wire = JsonWire(patch_json);
        let mut updated = false;
        for binding in &mut manifest_snapshot.pipelines {
            if binding.pipeline_id == target_pipeline_id {
                binding.pipeline_patch = Some(patch_wire.clone());
                updated = true;
            }
        }
        if !updated {
            manifest_snapshot.pipelines.push(crate::ipc::StreamPipelineBinding {
                pipeline_id: target_pipeline_id,
                pipeline_graph: None,
                pipeline_output: None,
                pipeline_patch: Some(patch_wire.clone()),
            });
        }
        if manifest_snapshot.active_pipeline_id.is_none() {
            manifest_snapshot.active_pipeline_id = Some(target_pipeline_id);
        }

        let applied = {
            let host = ctx.host.read().await;
            host.apply_graph_patch(Some(target_pipeline_id), &patch)
        };

        if applied.is_none() {
            let graph = build_manifest_graph(manifest_snapshot.host_buffer(), manifest_snapshot.clone(), None).await?;

            let (tx, rx) = oneshot::channel();
            enqueue_stream_command(&ctx.command_tx, StreamCommand::SetGraph { graph: graph.clone(), respond_to: tx })?;
            rx.await.map_err(|_| Error::InvalidState("stream worker dropped response"))??;
            *ctx.host.write().await = graph;
        }

        *ctx.manifest.write().await = manifest_snapshot;

        Ok(())
    }

    pub async fn set_pipeline_layout(&self, stream_id: Uuid, layout: Option<crate::ipc::StreamPipelineLayout>) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;

        let mut manifest_snapshot = ctx.manifest.read().await.clone();
        manifest_snapshot.pipeline_layout = layout.clone();
        if layout.is_some() {
            manifest_snapshot.pipeline_enabled = true;
        }
        if let Some(layout) = manifest_snapshot.pipeline_layout.as_mut() {
            let previous_active_pipeline_id = manifest_snapshot.active_pipeline_id;
            let mut layout_ids = std::collections::BTreeSet::new();
            let mut layout_selected_ids = std::collections::BTreeSet::new();
            for slot in &mut layout.slots {
                if slot.pipeline_id == Some(RAW_STREAM_PIPELINE_UUID) {
                    slot.output_key = normalize_output_for_pipeline(slot.output_key.clone(), Some(RAW_STREAM_PIPELINE_UUID))
                        .map_err(|err| Error::InvalidStateOwned(format!("invalid RAW pipeline layout output: {err}")))?;
                }
                if let Some(id) = slot.pipeline_id {
                    layout_selected_ids.insert(id);
                    if id == RAW_STREAM_PIPELINE_UUID {
                        continue;
                    }
                    layout_ids.insert(id);
                }
            }
            if !layout_ids.is_empty() {
                let existing: std::collections::BTreeSet<Uuid> = manifest_snapshot.pipelines.iter().map(|binding| binding.pipeline_id).collect();
                for pipeline_id in layout_ids {
                    if existing.contains(&pipeline_id) {
                        continue;
                    }
                    manifest_snapshot.pipelines.push(crate::ipc::StreamPipelineBinding { pipeline_id, pipeline_graph: None, pipeline_output: None, pipeline_patch: None });
                }
            }
            let single_slot_layout = layout.rows == 1 && layout.columns == 1;
            let active_in_layout = manifest_snapshot.active_pipeline_id.is_some_and(|id| layout_selected_ids.contains(&id));
            let should_retarget_active = single_slot_layout || !active_in_layout;
            let selected_slot = layout
                .slots
                .iter()
                .find(|slot| slot.row == 0 && slot.column == 0 && slot.pipeline_id.is_some())
                .or_else(|| layout.slots.iter().find(|slot| slot.pipeline_id.is_some()))
                .and_then(|slot| slot.pipeline_id.map(|pipeline_id| (pipeline_id, slot.output_key.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(|value| value.to_string()))));
            if should_retarget_active {
                if let Some((active_id, slot_output)) = selected_slot {
                    manifest_snapshot.active_pipeline_id = Some(active_id);
                    let active_changed = previous_active_pipeline_id != Some(active_id);
                    if active_id == RAW_STREAM_PIPELINE_UUID {
                        if let Some(slot_output) = slot_output {
                            manifest_snapshot.active_pipeline_output = Some(slot_output);
                        } else if active_changed {
                            manifest_snapshot.active_pipeline_output = manifest_snapshot.pipelines.iter().find(|p| p.pipeline_id == active_id).and_then(|binding| binding.pipeline_output.clone());
                        }
                    } else if active_changed || manifest_snapshot.active_pipeline_output.is_none() {
                        manifest_snapshot.active_pipeline_output = manifest_snapshot.pipelines.iter().find(|p| p.pipeline_id == active_id).and_then(|binding| binding.pipeline_output.clone());
                    }
                } else {
                    manifest_snapshot.active_pipeline_id = None;
                    manifest_snapshot.active_pipeline_output = None;
                }
            }
        }

        manifest_snapshot.active_pipeline_output = normalize_output_for_pipeline(manifest_snapshot.active_pipeline_output.clone(), manifest_snapshot.active_pipeline_id)
            .map_err(|err| Error::InvalidStateOwned(format!("invalid pipeline layout output: {err}")))?;
        let selected_output = manifest_snapshot.active_pipeline_output.clone();
        let graph = build_manifest_graph(manifest_snapshot.host_buffer(), manifest_snapshot.clone(), selected_output).await?;

        let (tx, rx) = oneshot::channel();
        enqueue_stream_command(&ctx.command_tx, StreamCommand::SetGraph { graph: graph.clone(), respond_to: tx })?;
        rx.await.map_err(|_| Error::InvalidState("stream worker dropped response"))??;

        *ctx.host.write().await = graph;
        *ctx.manifest.write().await = manifest_snapshot;

        Ok(())
    }

    pub async fn set_pipeline_wires(&self, stream_id: Uuid, wires: Vec<crate::ipc::StreamPipelineWire>) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;

        let mut manifest_snapshot = ctx.manifest.read().await.clone();
        manifest_snapshot.pipeline_wires = wires;
        if !manifest_snapshot.pipeline_wires.is_empty() {
            manifest_snapshot.pipeline_enabled = true;
        }

        let mut referenced: std::collections::BTreeSet<Uuid> = std::collections::BTreeSet::new();
        for wire in &manifest_snapshot.pipeline_wires {
            if wire.from.pipeline_id != RAW_STREAM_PIPELINE_UUID {
                referenced.insert(wire.from.pipeline_id);
            }
            if wire.to.pipeline_id != RAW_STREAM_PIPELINE_UUID {
                referenced.insert(wire.to.pipeline_id);
            }
        }
        if !referenced.is_empty() {
            let existing: std::collections::BTreeSet<Uuid> = manifest_snapshot.pipelines.iter().map(|binding| binding.pipeline_id).collect();
            for id in referenced {
                if !existing.contains(&id) {
                    return Err(Error::InvalidStateOwned(format!("pipeline {id} referenced by wiring is missing from stream manifest")));
                }
            }
        }

        let selected_output = manifest_snapshot.active_pipeline_output.clone();
        let graph = build_manifest_graph(manifest_snapshot.host_buffer(), manifest_snapshot.clone(), selected_output).await?;

        let (tx, rx) = oneshot::channel();
        enqueue_stream_command(&ctx.command_tx, StreamCommand::SetGraph { graph: graph.clone(), respond_to: tx })?;
        rx.await.map_err(|_| Error::InvalidState("stream worker dropped response"))??;

        *ctx.host.write().await = graph;
        *ctx.manifest.write().await = manifest_snapshot;

        Ok(())
    }

    pub async fn list_graph_outputs(&self, stream_id: Uuid) -> Result<Vec<crate::ipc::GraphOutputPortDescriptor>> {
        let ctx = self.get_stream(stream_id).await?;
        let host = ctx.host.read().await;
        Ok(host.host_output_port_descriptors())
    }

    pub async fn get_graph_output_sample(&self, stream_id: Uuid, port: String, fresh: bool) -> Result<serde_json::Value> {
        let ctx = self.get_stream(stream_id).await?;
        if !fresh {
            let host = ctx.host.read().await;
            return host.read_json_output(&port, false).ok_or(Error::NotFound("graph output sample unavailable"));
        }
        {
            let host = ctx.host.read().await;
            host.request_output_sample(&port);
            if let Some(value) = host.read_json_output(&port, false) {
                return Ok(value);
            }
        }
        let deadline = tokio::time::Instant::now() + Duration::from_millis(250);
        loop {
            {
                let host = ctx.host.read().await;
                if let Some(value) = host.read_json_output(&port, false) {
                    return Ok(value);
                }
            }
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Err(Error::NotFound("graph output sample unavailable"))
    }

    pub async fn set_graph_perf(&self, stream_id: Uuid, pipeline_id: Option<Uuid>, enabled: bool) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;
        let host = ctx.host.read().await;
        host.set_perf_enabled(pipeline_id, enabled);
        Ok(())
    }

    pub async fn reset_graph_metrics(&self, stream_id: Uuid, pipeline_id: Option<Uuid>) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;
        let host = ctx.host.read().await;
        host.reset_pipeline_metrics(pipeline_id);
        Ok(())
    }

    pub async fn capture_graph_flamegraph(&self, stream_id: Uuid, pipeline_id: Option<Uuid>, duration_ms: u64) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;
        let host = ctx.host.read().await;
        host.capture_flamegraph(pipeline_id, duration_ms).map_err(Error::InvalidStateOwned)?;
        Ok(())
    }
}
