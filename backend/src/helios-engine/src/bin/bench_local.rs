//! Local stream benchmark using the virtual capture backend (no device needed).

use helios_engine::capture::{default_virtual_device, CaptureConfig};
use helios_engine::identity::DeviceIdentity;
use helios_engine::ipc::{EngineCommand, EngineEvent, JsonWire, StreamManifest, StreamPipelineBinding};
use helios_engine::runtime::EngineRuntime;
use lib_ipc::types::CommandId;
use serde_json::json;
use std::env;
use std::fs;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let graph_path = env::var("HELIOS_BENCH_GRAPH").unwrap_or_else(|_| "configs/templates/daedalus_aruco.json".to_string());
    let duration_secs: u64 = env::var("HELIOS_BENCH_DURATION_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
    let warmup_secs: u64 = env::var("HELIOS_BENCH_WARMUP_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
    let fps: u32 = env::var("HELIOS_BENCH_FPS").ok().and_then(|v| v.parse().ok()).unwrap_or(30);
    let encoder_id = env::var("HELIOS_BENCH_ENCODER_ID").ok().filter(|s| !s.trim().is_empty());
    let decoder_id = env::var("HELIOS_BENCH_DECODER_ID").ok().filter(|s| !s.trim().is_empty());

    let graph_text = fs::read_to_string(&graph_path)?;
    let doc: serde_json::Value = serde_json::from_str(&graph_text)?;
    let graph_json = doc.get("graph").cloned().unwrap_or(doc);

    let device = default_virtual_device();
    let backend = device.backends.first().ok_or("virtual device missing backend")?;
    let mode = backend.descriptor.modes.first().ok_or("virtual device missing mode")?.id.clone();

    let capture = CaptureConfig {
        device_keys: device.identity.keys.clone(),
        backend: backend.kind,
        handle: backend.handle.clone(),
        mode,
        target_fps: Some(fps),
        interval: None,
        controls: Vec::new(),
        enable_tdn_output: false,
    };

    let identity = DeviceIdentity::new(None, device.identity.keys.first().cloned(), Some(device.identity.display.clone()));

    let pipeline_id = Uuid::new_v4();
    let manifest = StreamManifest {
        identity,
        capture,
        host_buffer: 0,
        internal: true,
        pipeline_enabled: Some(true),
        pipelines: vec![StreamPipelineBinding { pipeline_id, pipeline_graph: Some(JsonWire(graph_json)), pipeline_output: Some("frame".into()), pipeline_patch: None }],
        active_pipeline_id: Some(pipeline_id),
        active_pipeline_output: Some("frame".into()),
        pipeline_layout: None,
        pipeline_wires: Vec::new(),
        pipeline_host_inputs: std::collections::BTreeMap::new(),
        calibration: None,
        pose: None,
        encoder_enabled: None,
        encoder_id,
        decoder_enabled: None,
        decoder_id,
        encoder_settings: None,
        decoder_settings: None,
        shadow_recorder_enabled: true,
        start_on_boot: false,
    };

    let engine = EngineRuntime::new();
    let start = engine.handle_command(EngineCommand::Start { command_id: CommandId::new(), manifest: Box::new(manifest) }).await;

    let stream_id = match start {
        EngineEvent::Started { stream_id, .. } => stream_id,
        EngineEvent::Nack { reason, .. } => {
            return Err(format!("start failed: {reason}").into());
        }
        other => {
            return Err(format!("unexpected start response: {other:?}").into());
        }
    };

    let _encoded_rx = engine.subscribe_encoded(stream_id).await.ok();
    sleep(Duration::from_secs(warmup_secs)).await;
    sleep(Duration::from_secs(duration_secs)).await;

    let metrics = engine.handle_command(EngineCommand::GetMetrics { command_id: CommandId::new(), stream_id }).await;

    let _ = engine.handle_command(EngineCommand::Stop { command_id: CommandId::new(), stream_id }).await;

    let output = match metrics {
        EngineEvent::Metrics { metrics, .. } => {
            let decoder = metrics.decoder.clone();
            let encoder = metrics.encoder.clone();
            json!({
                "duration_secs": duration_secs,
                "warmup_secs": warmup_secs,
                "fps": fps,
                "decoder": decoder,
                "encoder": encoder,
                "capture": metrics.capture,
                "host": metrics.host,
            })
        }
        EngineEvent::Nack { reason, .. } => {
            json!({ "error": reason })
        }
        other => json!({ "error": format!("unexpected metrics response: {other:?}") }),
    };

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
