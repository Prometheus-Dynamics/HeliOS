use helios_engine::{
    capture::{BackendHandle, BackendKind, CaptureConfig, ControlAssignment, DiscoveredDevice},
    identity::DeviceIdentity,
    ipc::{EngineEvent, StreamManifest},
    stream::touch_stream_preview,
};
use std::{collections::HashMap, sync::OnceLock, time::Duration};
use styx::codec::{CodecKind, CodecRegistry};
use styx::core::format::Interval;
use styx::prelude::FourCc;
use tokio::time::sleep;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::http::AppState;

use super::wait::{wait_for_stream_gone, wait_for_stream_started};

pub fn backend_handle_eq(a: &BackendHandle, b: &BackendHandle) -> bool {
    serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
}

fn interval_from_fps(target_fps: u32) -> Option<Interval> {
    use std::num::NonZeroU32;
    let fps = NonZeroU32::new(target_fps)?;
    Some(Interval { numerator: NonZeroU32::new(1).unwrap(), denominator: fps })
}

pub fn identity_for_keys(keys: &[String]) -> DeviceIdentity {
    DeviceIdentity { id: None, alias: None, hardware_id: keys.first().cloned() }
}

pub async fn discover_devices(state: &AppState) -> Vec<DiscoveredDevice> {
    state.engine.discover_devices().await.map(|discovery| discovery.devices).unwrap_or_default()
}

pub fn find_matching_backend(devices: &[DiscoveredDevice], backend: BackendKind, handle: &BackendHandle, device_keys: &[String]) -> Option<(DiscoveredDevice, styx::ProbedBackend)> {
    let dev = devices.iter().find(|d| {
        if device_keys.is_empty() {
            return true;
        }
        d.identity.keys.iter().any(|k| device_keys.iter().any(|want| want == k))
    })?;

    let backend = dev.backends.iter().find(|b| b.kind == backend && backend_handle_eq(handle, &b.handle)).or_else(|| dev.backends.iter().find(|b| b.kind == backend))?;
    Some((dev.clone(), backend.clone()))
}

#[derive(Clone)]
pub struct CodecImplCache {
    pub decoders_by_input: HashMap<FourCc, Vec<String>>,
    pub encoders_rg24: Vec<String>,
}

fn list_codec_impls_by_input(kind: CodecKind) -> HashMap<FourCc, Vec<String>> {
    let mut out: HashMap<FourCc, Vec<String>> = HashMap::new();
    if let Ok(entries) = CodecRegistry::list_enabled_codecs() {
        for (_fourcc, descs) in entries {
            for desc in descs {
                if desc.kind != kind {
                    continue;
                }
                out.entry(desc.input).or_default().push(desc.impl_name.to_string());
            }
        }
    }
    for impls in out.values_mut() {
        impls.sort();
        impls.dedup();
    }
    out
}

pub fn codec_impl_cache() -> &'static CodecImplCache {
    static CACHE: OnceLock<CodecImplCache> = OnceLock::new();
    CACHE.get_or_init(|| {
        let decoders_by_input = list_codec_impls_by_input(CodecKind::Decoder);
        let encoders_by_input = list_codec_impls_by_input(CodecKind::Encoder);
        let encoders_rg24 = encoders_by_input.get(&FourCc::new(*b"RG24")).cloned().unwrap_or_default();
        CodecImplCache { decoders_by_input, encoders_rg24 }
    })
}

async fn touch_preview_periodically(stream_id: Uuid, ms: u64, stop: tokio::sync::watch::Receiver<bool>) {
    let period = Duration::from_millis(ms.max(50));
    let mut stop = stop;
    loop {
        if *stop.borrow() {
            break;
        }
        let _ = tokio::task::spawn_blocking(move || touch_stream_preview(stream_id)).await;
        tokio::select! {
            _ = sleep(period) => {}
            _ = stop.changed() => {}
        }
    }
}

pub async fn start_stream_for_mode(args: StartStreamArgs<'_>) -> Result<Uuid, String> {
    let StartStreamArgs { state, identity, keys, backend, handle, mode_id, target_fps, controls } = args;
    let capture = CaptureConfig {
        device_keys: keys.to_vec(),
        backend,
        handle,
        mode: mode_id.clone(),
        target_fps: if backend == BackendKind::Libcamera { Some(target_fps) } else { None },
        interval: if backend == BackendKind::Libcamera { None } else { interval_from_fps(target_fps) },
        controls,
        enable_tdn_output: false,
    };

    let mut manifest = StreamManifest {
        identity: identity.clone(),
        capture,
        host_buffer: 0,
        internal: true,
        pipeline_enabled: None,
        pipelines: Vec::new(),
        active_pipeline_id: None,
        active_pipeline_output: None,
        pipeline_layout: None,
        pipeline_wires: Vec::new(),
        pipeline_host_inputs: std::collections::BTreeMap::new(),
        calibration: None,
        pose: None,
        encoder_enabled: None,
        encoder_id: None,
        decoder_enabled: None,
        decoder_id: None,
        encoder_settings: None,
        decoder_settings: None,
        shadow_recorder_enabled: false,
        start_on_boot: false,
    };

    let requested_id = Uuid::new_v4();
    manifest.identity.id = Some(requested_id);

    let stream_id = match state.engine.start_stream(manifest.clone()).await {
        Ok(EngineEvent::Started { stream_id, .. }) => stream_id,
        Ok(EngineEvent::Nack { reason, .. }) => return Err(reason),
        Ok(_) | Err(_) => match wait_for_stream_started(state, requested_id, Duration::from_secs(15)).await {
            Ok(Some(_descriptor)) => requested_id,
            Ok(None) => return Err("start timeout".into()),
            Err(err) => return Err(err.to_string()),
        },
    };

    Ok(stream_id)
}

pub struct StartStreamArgs<'a> {
    pub state: &'a AppState,
    pub identity: &'a DeviceIdentity,
    pub keys: &'a [String],
    pub backend: BackendKind,
    pub handle: BackendHandle,
    pub mode_id: &'a styx::capture::ModeId,
    pub target_fps: u32,
    pub controls: Vec<ControlAssignment>,
}

pub async fn sample_stream_metrics(state: &AppState, stream_id: Uuid, sample_ms: u64, touch_preview: bool) -> Result<helios_engine::stream::StreamMetrics, String> {
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let touch_join = touch_preview.then(|| tokio::spawn(touch_preview_periodically(stream_id, 200, stop_rx)));

    sleep(Duration::from_millis(sample_ms)).await;

    let _ = stop_tx.send(true);
    if let Some(join) = touch_join {
        let _ = join.await;
    }

    match state.engine.get_metrics(stream_id).await {
        Ok(EngineEvent::Metrics { metrics, .. }) => Ok(metrics),
        Ok(other) => {
            warn!(?other, stream_id = %stream_id, "unexpected engine event for get_metrics");
            Err("unexpected metrics response".into())
        }
        Err(err) => Err(err.to_string()),
    }
}

pub async fn stop_stream_best_effort(state: &AppState, stream_id: Uuid) {
    let _ = state.engine.stop_stream(stream_id).await;
    let _ = wait_for_stream_gone(state, stream_id, Duration::from_secs(15)).await;
    sleep(Duration::from_millis(250)).await;
}

pub async fn stop_conflicting_streams(state: &AppState, keys: &[String]) -> Vec<StreamManifest> {
    let mut restored: Vec<StreamManifest> = Vec::new();
    if let Ok(running) = state.engine.list_streams().await {
        for s in running {
            if s.manifest.capture.device_keys.iter().any(|k| keys.iter().any(|want| want == k)) {
                debug!(stream_id = %s.stream_id, "stopping conflicting stream for benchmark");
                restored.push(s.manifest.clone());
                let _ = state.engine.stop_stream(s.stream_id).await;
                let _ = wait_for_stream_gone(state, s.stream_id, Duration::from_secs(10)).await;
                sleep(Duration::from_millis(250)).await;
            }
        }
    }
    restored
}

pub async fn restore_streams_best_effort(state: &AppState, restored: Vec<StreamManifest>, warnings: &mut Vec<String>) {
    for mut manifest in restored {
        manifest.identity.id = None;
        match state.engine.start_stream(manifest.clone()).await {
            Ok(EngineEvent::Started { .. }) => {}
            Ok(EngineEvent::Nack { reason, .. }) => warnings.push(format!("failed to restore stream: {reason}")),
            Ok(_) | Err(_) => {}
        }
        sleep(Duration::from_millis(250)).await;
    }
}
