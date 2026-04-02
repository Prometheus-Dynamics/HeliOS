use super::*;

pub(super) async fn start_recording(manager: &StreamManager, stream_id: Uuid, params: StartRecordingParams) -> Result<()> {
    let StartRecordingParams { source, output_path, container, codec, duration_ms, settings } = params;
    let ctx = manager.get_stream(stream_id).await?;
    {
        let recordings = manager.recordings.lock().await;
        if recordings.contains_key(&stream_id) {
            return Err(Error::Conflict("recording already active"));
        }
    }

    let manifest_snapshot = ctx.manifest.read().await.clone();
    let resolved_source = resolve_recording_source(&manifest_snapshot, source.clone());

    let record_path = PathBuf::from(output_path);
    let raw_path = match container {
        RecordingContainer::Mp4 => build_raw_path(&record_path, codec),
        RecordingContainer::Raw => record_path.clone(),
    };
    let frame_ts_path = recording_frame_ts_path(&record_path);
    let requested_fps = settings.as_ref().and_then(|s| s.fps).filter(|v| *v > 0.0).or_else(|| super::policy::infer_recording_fps(&manifest_snapshot));
    let encoder_hint = manifest_snapshot.encoder.codec_id.clone().filter(|value| !value.trim().is_empty());

    let started_at_ms = current_time_ms();
    let (stop_tx, stop_rx) = oneshot::channel();
    let (done_tx, done_rx) = watch::channel(RecordingState::Running);
    let session = Arc::new(AsyncMutex::new(RecordingSession { stop_tx: Some(stop_tx), done_rx, started_at_ms }));
    {
        let mut recordings = manager.recordings.lock().await;
        if recordings.contains_key(&stream_id) {
            return Err(Error::Conflict("recording already active"));
        }
        recordings.insert(stream_id, session.clone());
    }

    let passthrough_codec = super::policy::infer_recording_codec(manifest_snapshot.encoder_id());
    let use_encoded_passthrough = super::policy::recording_encoded_passthrough_enabled()
        && matches!(&resolved_source, ResolvedRecordingSource::Multiplex)
        && manifest_snapshot.encoder.enabled
        && passthrough_codec.is_some()
        && (matches!(container, RecordingContainer::Mp4) || passthrough_codec == Some(codec));
    if use_encoded_passthrough {
        let manager = manager.clone();
        let encoded_consumer = ManagedEncodedConsumer::new(Arc::clone(&ctx.managed_encoded_consumer_count), Arc::clone(&ctx.managed_encoded_consumer_last_seen_ms));
        let encoded_consumer_touch = encoded_consumer.touch_handle();
        let rx = ctx.encoded_tx.subscribe();
        let source_codec = passthrough_codec.unwrap_or(codec);
        let frame_ts_path = frame_ts_path.clone();
        tokio::spawn(async move {
            let _encoded_consumer = encoded_consumer;
            let result = record_encoded_session(EncodedRecordingSessionRequest {
                rx,
                stop_rx,
                output_path: record_path,
                raw_path,
                container,
                source_codec,
                target_codec: codec,
                duration_ms,
                fps: requested_fps,
                settings,
                timestamps_path: Some(frame_ts_path),
                consumer_touch: Some(encoded_consumer_touch),
            })
            .await;
            let _ = done_tx.send(RecordingState::Completed(result.clone()));
            manager.finish_recording(stream_id, result).await;
        });
        return Ok(());
    }

    let shadow_candidate = super::policy::recording_shadow_start_stop_enabled()
        && super::policy::shadow_recorder_feature_enabled()
        && manifest_snapshot.recording_mode.is_shadow_buffer()
        && matches!(source, RecordingSource::Multiplex)
        && manifest_snapshot.recording_mode.shadow_buffer_codec().is_some();
    if shadow_candidate {
        let stream_codec = manifest_snapshot.recording_mode.shadow_buffer_codec().unwrap();
        if codec != stream_codec {
            {
                let mut recordings = manager.recordings.lock().await;
                recordings.remove(&stream_id);
            }
            return Err(Error::InvalidStateOwned(format!("requested codec {codec:?} does not match stream encoder ({stream_codec:?}); configure encoder_id accordingly")));
        }

        let already_running = {
            let recorders = manager.shadow_recorders.lock().await;
            recorders.contains_key(&stream_id)
        };
        if !already_running {
            if let Err(err) = start_shadow_recorder(manager, stream_id, &manifest_snapshot).await {
                {
                    let mut recordings = manager.recordings.lock().await;
                    recordings.remove(&stream_id);
                }
                return Err(Error::InvalidStateOwned(format!("shadow recorder start failed: {err}")));
            }
        }

        let measured_fps = super::runtime::sample_stream_encoder_fps(manager, stream_id).await;
        let fps = settings.as_ref().and_then(|s| s.fps).filter(|v| *v > 0.0).or(measured_fps).or(requested_fps);

        let manager = manager.clone();
        let shadow_dir = super::policy::shadow_dir_for_stream(stream_id);
        tokio::spawn(async move {
            let result = record_shadow_segments_session(ShadowRecordingSessionRequest {
                stream_id,
                shadow_dir,
                codec,
                output_path: record_path,
                raw_path,
                container,
                started_at_ms,
                duration_ms,
                fps,
                settings,
                stop_rx,
            })
            .await;
            let _ = done_tx.send(RecordingState::Completed(result.clone()));
            manager.finish_recording(stream_id, result).await;
        });
        return Ok(());
    }

    let frame_source = match resolved_source {
        ResolvedRecordingSource::Multiplex => {
            let graph = ctx.host.read().await.clone();
            RecordingFrameSource::Multiplex { rx: graph.subscribe() }
        }
        ResolvedRecordingSource::Raw => RecordingFrameSource::Raw { rx: ctx.raw_tx.subscribe() },
        ResolvedRecordingSource::Pipeline { pipeline_id, output_key } => match build_recording_pipeline_graph(&manifest_snapshot, pipeline_id, output_key.as_deref()) {
            Ok(graph) => RecordingFrameSource::Pipeline { rx: ctx.raw_tx.subscribe(), graph },
            Err(err) => {
                tracing::warn!(
                    stream_id = %stream_id,
                    pipeline_id = %pipeline_id,
                    error = %err,
                    "recording pipeline invalid; falling back to multiplex"
                );
                let graph = ctx.host.read().await.clone();
                RecordingFrameSource::Multiplex { rx: graph.subscribe() }
            }
        },
    };
    let manager = manager.clone();
    tokio::spawn(async move {
        let params =
            RecordingFrameParams { output_path: &record_path, raw_path: &raw_path, container, codec, duration_ms, fps: requested_fps, settings, encoder_hint, frame_ts_path: Some(frame_ts_path) };
        let result = record_frame_stream(frame_source, stop_rx, params).await;
        let _ = done_tx.send(RecordingState::Completed(result.clone()));
        manager.finish_recording(stream_id, result).await;
    });

    Ok(())
}

pub(super) async fn stop_recording(manager: &StreamManager, stream_id: Uuid) -> Result<()> {
    let session = {
        let recordings = manager.recordings.lock().await;
        recordings.get(&stream_id).cloned()
    };
    let Some(session) = session else {
        return Ok(());
    };

    let (stop_tx, mut done_rx) = {
        let mut session = session.lock().await;
        (session.stop_tx.take(), session.done_rx.clone())
    };
    if let Some(stop_tx) = stop_tx {
        let _ = stop_tx.send(());
    }

    if matches!(*done_rx.borrow(), RecordingState::Completed(_)) {
        return recording_state_to_result(done_rx.borrow().clone());
    }

    match tokio::time::timeout(RECORDING_STOP_TIMEOUT, done_rx.changed()).await {
        Ok(Ok(())) => recording_state_to_result(done_rx.borrow().clone()),
        Ok(Err(_)) => Err(Error::InvalidState("recording status channel closed")),
        Err(_) => Err(Error::Timeout),
    }
}

pub(super) async fn capture_shadow_recording(manager: &StreamManager, stream_id: Uuid, output_path: String, container: RecordingContainer, window_ms: u64) -> Result<()> {
    if !super::policy::shadow_recorder_feature_enabled() {
        return Err(Error::InvalidState("shadow recorder feature is disabled"));
    }
    let ctx = manager.get_stream(stream_id).await?;
    let manifest_snapshot = ctx.manifest.read().await.clone();
    let Some(configured_codec) = manifest_snapshot.recording_mode.shadow_buffer_codec() else {
        return Err(Error::InvalidState("shadow recorder disabled"));
    };
    let (requested_codec, raw_format) = {
        let recorders = manager.shadow_recorders.lock().await;
        recorders.get(&stream_id).map(|handle| {
            let raw = RawRecordingFormat::from_u8(handle.raw_format.load(Ordering::Acquire));
            (handle.requested_codec, Some(raw))
        })
    }
    .unwrap_or((configured_codec, None));
    let raw_format = raw_format.unwrap_or_else(|| RawRecordingFormat::from_codec(requested_codec));
    let window_ms = super::policy::normalize_shadow_window_ms(window_ms);
    if window_ms == 0 {
        return Err(Error::InvalidState("shadow capture window must be > 0"));
    }
    let preroll_ms = super::policy::shadow_segment_ms().max(1_000);
    let capture_ms = window_ms.saturating_add(preroll_ms).min(super::policy::shadow_window_ms());

    let shadow_dir = super::policy::shadow_dir_for_stream(stream_id);
    if fs::metadata(&shadow_dir).await.is_err() {
        return Err(Error::NotFound("shadow recorder data not found"));
    }

    let output_path = PathBuf::from(output_path);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).await.map_err(|err| Error::InvalidStateOwned(format!("shadow output dir create failed: {err}")))?;
    }

    let raw_path = match container {
        RecordingContainer::Mp4 => build_raw_path(&output_path, requested_codec),
        RecordingContainer::Raw => output_path.clone(),
    };

    capture_shadow_segments(&shadow_dir, &raw_path, requested_codec, capture_ms).await.map_err(Error::InvalidStateOwned)?;

    if matches!(container, RecordingContainer::Mp4) {
        let forced_fps = probe_raw_frames(&raw_path, raw_format).await.ok().map(|frames| {
            let secs = (capture_ms as f32 / 1000.0).max(0.001);
            (frames as f32 / secs).clamp(1.0, 240.0)
        });
        let fps = forced_fps.or(super::runtime::sample_stream_encoder_fps(manager, stream_id).await).or_else(|| super::policy::infer_recording_fps(&manifest_snapshot));

        let pretrim = pretrim_output_path(&output_path);
        let finalize_res = finalize_recording_mp4(raw_path.clone(), pretrim.clone(), raw_format, requested_codec, fps, None).await;
        if let Err(err) = finalize_res {
            let _ = fs::remove_file(&pretrim).await;
            return Err(Error::InvalidStateOwned(err));
        }
        let trim_res = trim_mp4_to_last_window(&pretrim, &output_path, window_ms, requested_codec).await;
        let _ = fs::remove_file(&pretrim).await;
        if let Err(err) = trim_res {
            let _ = fs::remove_file(&output_path).await;
            return Err(Error::InvalidStateOwned(err));
        }
    }

    Ok(())
}

pub(super) async fn start_shadow_recorder(manager: &StreamManager, stream_id: Uuid, manifest: &ResolvedStreamConfig) -> Result<()> {
    let ctx = manager.get_stream(stream_id).await?;
    if !manifest.encoder.enabled {
        return Err(Error::InvalidState("shadow recorder requires encoder enabled"));
    }
    let preferred_codec = manifest.recording_mode.shadow_buffer_codec().ok_or(Error::InvalidState("shadow recorder requires shadow-buffer recording mode"))?;
    let raw_format = RawRecordingFormat::from_codec(preferred_codec);
    let shadow_dir = super::policy::shadow_dir_for_stream(stream_id);
    let window_ms = super::policy::shadow_window_ms();
    let segment_ms = super::policy::shadow_segment_ms();
    let format_tracker = Arc::new(AtomicU8::new(raw_format.to_u8()));
    let tracker_for_worker = Arc::clone(&format_tracker);
    let (stop_tx, stop_rx) = oneshot::channel();
    let encoded_consumer = ManagedEncodedConsumer::new(Arc::clone(&ctx.managed_encoded_consumer_count), Arc::clone(&ctx.managed_encoded_consumer_last_seen_ms));
    let encoded_consumer_touch = encoded_consumer.touch_handle();
    let mut encoded_rx = ctx.encoded_tx.subscribe();
    let join = tokio::spawn(async move {
        let _encoded_consumer = encoded_consumer;
        let worker = match ShadowRecorderWorker::start(ShadowRecorderConfig { shadow_dir: shadow_dir.clone(), codec: preferred_codec, segment_ms, window_ms, format_tracker: tracker_for_worker }) {
            Ok(worker) => worker,
            Err(err) => {
                tracing::warn!(stream_id = %stream_id, error = %err, "shadow recorder worker start failed");
                return;
            }
        };
        if let Err(err) = run_shadow_recorder_stream(&mut encoded_rx, worker, stop_rx, Some(encoded_consumer_touch)).await {
            tracing::warn!(stream_id = %stream_id, error = %err, "shadow recorder stopped with error");
        }
    });

    let mut recorders = manager.shadow_recorders.lock().await;
    if recorders.contains_key(&stream_id) {
        join.abort();
        return Err(Error::Conflict("shadow recorder already active"));
    }
    recorders.insert(stream_id, ShadowRecorderHandle { stop_tx: Some(stop_tx), join, requested_codec: preferred_codec, raw_format: format_tracker });
    Ok(())
}

pub(super) async fn stop_shadow_recorder(manager: &StreamManager, stream_id: Uuid) -> Result<()> {
    let handle = {
        let mut recorders = manager.shadow_recorders.lock().await;
        recorders.remove(&stream_id)
    };
    let Some(mut handle) = handle else {
        return Ok(());
    };
    if let Some(stop_tx) = handle.stop_tx.take() {
        let _ = stop_tx.send(());
    }
    match tokio::time::timeout(SHADOW_STOP_TIMEOUT, handle.join).await {
        Ok(_) => Ok(()),
        Err(_) => Err(Error::Timeout),
    }
}
