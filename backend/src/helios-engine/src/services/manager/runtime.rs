use super::*;

pub(super) fn new_manager() -> StreamManager {
    // Best-effort cleanup of orphaned shared-memory preview buffers from prior crashes/restarts.
    // These live on tmpfs and can permanently inflate RSS if left behind.
    cleanup_all_stream_files();
    // Clean up recording staging dirs that can be left behind if the engine crashes mid-recording.
    super::policy::cleanup_recording_stage_root_sync();
    StreamManager {
        streams: Arc::new(RwLock::new(HashMap::new())),
        starting: Arc::new(AsyncMutex::new(HashSet::new())),
        recordings: Arc::new(AsyncMutex::new(HashMap::new())),
        shadow_recorders: Arc::new(AsyncMutex::new(HashMap::new())),
    }
}

pub(super) async fn start_stream(manager: &StreamManager, manifest: ResolvedStreamConfig) -> Result<(Uuid, CaptureDescriptor)> {
    let mut manifest = manifest;
    let stream_id = manifest.identity.id.unwrap_or_else(Uuid::new_v4);
    manifest.identity.id = Some(stream_id);
    super::policy::apply_stream_start_policy(&mut manifest)?;

    {
        let streams = manager.streams.read().await;
        if streams.contains_key(&stream_id) {
            return Err(Error::Conflict("stream already exists"));
        }
    }
    if let Some(requested_alias) = normalize_alias(manifest.identity.alias.as_deref()) {
        let entries: Vec<Arc<StreamContext>> = {
            let streams = manager.streams.read().await;
            streams.values().cloned().collect()
        };
        for ctx in entries {
            let existing_alias = normalize_alias(ctx.manifest.read().await.identity.alias.as_deref());
            if existing_alias.as_deref() == Some(&requested_alias) {
                return Err(Error::Conflict("stream alias already exists"));
            }
        }
    }
    {
        let mut starting = manager.starting.lock().await;
        if !starting.insert(stream_id) {
            return Err(Error::Conflict("stream already starting"));
        }
    }

    let streams_handle = manager.streams.clone();
    let manifest_clone = manifest.clone();
    let start_res = tokio::task::spawn_blocking({
        let manifest = manifest.clone();
        move || {
            let host_buffer = manifest.host_buffer();
            let graph = crate::graph::build_graph_handle_for_manifest(host_buffer, &manifest, None).map_err(|err| Error::InvalidStateOwned(format!("pipeline graph invalid: {err}")))?;
            let shmem = match ShmemWriter::create(stream_id) {
                Ok(writer) => Some(writer),
                Err(err) => {
                    tracing::warn!(stream_id = %stream_id, error = %err, "shmem writer init failed; preview endpoints may be unavailable");
                    None
                }
            };
            let descriptor = descriptor_for_config_retrying(&manifest.capture).ok_or_else(|| Error::RetryableInvalidStateOwned("missing capture descriptor".to_string()))?;

            let runner = StreamRunner::new(StreamRunnerConfig {
                capture_config: manifest.capture.clone(),
                graph: graph.clone(),
                encoder_id: manifest.encoder.codec_id.clone(),
                decoder_id: manifest.decoder.codec_id.clone(),
                encoder_settings: manifest.encoder.settings.clone(),
                decoder_settings: manifest.decoder.settings.clone(),
                preview_jpeg_quality: manifest.preview_jpeg_quality,
                shmem,
                stream_id: Some(stream_id),
            });
            let encoded_tx = runner.encoded_sender();
            let managed_encoded_consumer_count = runner.managed_encoded_consumer_count_handle();
            let managed_encoded_consumer_last_seen_ms = runner.managed_encoded_consumer_last_seen_handle();
            let raw_tx = runner.raw_sender();
            let (command_tx, command_rx) = sync_channel::<StreamCommand>(stream_command_queue_size());
            let (exit_tx, exit_rx) = watch::channel(StreamExit::Running);
            let join: JoinHandle<()> = std::thread::Builder::new()
                .name(format!("helios-stream-{stream_id}"))
                .stack_size(stream_worker_stack_size_bytes())
                .spawn(move || run_stream_worker(runner, command_rx, exit_tx))
                .map_err(|err| Error::InvalidStateOwned(format!("stream worker spawn failed: {err}")))?;
            Ok::<_, Error>((descriptor, graph, encoded_tx, managed_encoded_consumer_count, managed_encoded_consumer_last_seen_ms, raw_tx, command_tx, exit_rx, join))
        }
    })
    .await
    .map_err(|_| Error::InvalidState("stream worker start cancelled"));
    let (descriptor, host, encoded_tx, managed_encoded_consumer_count, managed_encoded_consumer_last_seen_ms, raw_tx, command_tx, exit_rx, join) = match start_res {
        Ok(Ok(parts)) => parts,
        Ok(Err(err)) => {
            manager.finish_starting(stream_id).await;
            return Err(err);
        }
        Err(err) => {
            manager.finish_starting(stream_id).await;
            return Err(err);
        }
    };
    let cleanup_rx = exit_rx.clone();
    let monitor_rx = cleanup_rx.clone();
    let shadow_enabled = manifest_clone.recording_mode.is_shadow_buffer();
    let stream_started_at_ms = current_time_ms();
    let encoded_tx_for_ctx = encoded_tx.clone();
    let raw_tx_for_ctx = raw_tx.clone();
    let mut streams = manager.streams.write().await;
    if streams.contains_key(&stream_id) {
        drop(streams);
        abort_unregistered_stream_worker(stream_id, command_tx, join).await;
        manager.finish_starting(stream_id).await;
        return Err(Error::Conflict("stream already exists"));
    }
    streams.insert(
        stream_id,
        Arc::new(StreamContext {
            stream_started_at_ms,
            manifest: tokio::sync::RwLock::new(manifest_clone.clone()),
            descriptor: descriptor.clone(),
            host: tokio::sync::RwLock::new(host),
            calibration_mode_restore: tokio::sync::RwLock::new(None),
            encoded_tx: encoded_tx_for_ctx,
            managed_encoded_consumer_count,
            managed_encoded_consumer_last_seen_ms,
            raw_tx: raw_tx_for_ctx,
            command_tx,
            exit_rx,
            cleanup_rx: AsyncMutex::new(Some(cleanup_rx)),
            worker_join: AsyncMutex::new(Some(join)),
        }),
    );
    drop(streams);
    if shadow_enabled {
        if let Err(err) = manager.start_shadow_recorder(stream_id, &manifest_clone).await {
            tracing::warn!(stream_id = %stream_id, error = %err, "shadow recorder start failed");
        }
    }
    tracing::info!(stream_id = %stream_id, "stream registered");
    manager.finish_starting(stream_id).await;
    tokio::spawn({
        let streams_handle = streams_handle.clone();
        let manager = manager.clone();
        let mut exit_rx = monitor_rx;
        async move {
            let _ = exit_rx.changed().await;
            let _ = manager.stop_shadow_recorder(stream_id).await;
            let ctx = {
                let mut streams = streams_handle.write().await;
                streams.remove(&stream_id)
            };
            if let Some(ctx) = ctx {
                let exit_state = ctx.exit_rx.borrow().clone();
                finalize_stream_teardown(&manager, stream_id, ctx).await;
                if let StreamExit::Stopped(Err(err)) = exit_state {
                    tracing::warn!(stream_id = %stream_id, error = %err, "stream worker exited with error");
                }
            }
        }
    });
    Ok((stream_id, descriptor))
}

pub(super) async fn stop_stream(manager: &StreamManager, stream_id: Uuid) -> Result<()> {
    tracing::info!(stream_id = %stream_id, "stream stop requested");
    let ctx = {
        let streams = manager.streams.read().await;
        streams.get(&stream_id).cloned()
    };
    let Some(ctx) = ctx else {
        return Ok(());
    };
    let _ = manager.stop_recording(stream_id).await;
    let _ = manager.stop_shadow_recorder(stream_id).await;
    let (tx, rx) = oneshot::channel();
    enqueue_stream_command(&ctx.command_tx, StreamCommand::Stop { respond_to: tx })?;
    let _ = rx.await;

    let mut exit_rx = ctx.cleanup_rx.lock().await.take().unwrap_or_else(|| ctx.exit_rx.clone());
    let stopped = tokio::time::timeout(Duration::from_secs(25), async {
        loop {
            if matches!(exit_rx.borrow().clone(), StreamExit::Stopped(_)) {
                break;
            }
            if exit_rx.changed().await.is_err() {
                break;
            }
        }
    })
    .await
    .is_ok();
    if !stopped {
        tracing::warn!(stream_id = %stream_id, "stream stop did not complete within 25s");
        return Err(Error::Timeout);
    }
    let ctx = {
        let mut streams = manager.streams.write().await;
        streams.remove(&stream_id)
    };
    if let Some(ctx) = ctx {
        finalize_stream_teardown(manager, stream_id, ctx).await;
    }
    tracing::info!(stream_id = %stream_id, "stream stop completed");
    Ok(())
}

pub(super) async fn finalize_stream_teardown(_manager: &StreamManager, stream_id: Uuid, ctx: Arc<StreamContext>) {
    cleanup_stream_files(stream_id);
    if let Some(join) = ctx.worker_join.lock().await.take() {
        let _ = tokio::task::spawn_blocking(move || join.join()).await;
    }
}

pub(super) async fn sample_stream_encoder_fps(manager: &StreamManager, stream_id: Uuid) -> Option<f32> {
    for _ in 0..5 {
        if let Ok(metrics) = manager.get_metrics(stream_id).await {
            if let Some(fps) = metrics.encoder.as_ref().map(|m| m.fps as f32).filter(|v| v.is_finite() && *v > 0.0) {
                return Some(fps);
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    None
}
