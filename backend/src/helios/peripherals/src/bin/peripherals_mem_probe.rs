use std::{collections::BTreeMap, path::PathBuf, time::Duration};

use helios_peripherals::{
    config::PeripheralConfig,
    model::NodeId,
    provider::OrionPeripheralPublisher,
    resources::{CaptureProbe, DiscoveryProbe, LemnosPeripheralStack, probe_capture_devices},
    runtime::build_inventory_service,
};
use orion::client::LocalNodeRuntime;
use styx::{
    BackendKind, ProbedDevice,
    codec::CodecRegistry,
    ipc::CameraService,
    prelude::{CaptureRequest, CaptureStartPolicy, RecvOutcome},
};
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = fmt().with_env_filter(filter).try_init();

    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "idle".to_string());
    let hold_secs = args.next().and_then(|value| value.parse::<u64>().ok()).unwrap_or(45);

    println!("probe_mode={mode} pid={}", std::process::id());
    match mode.as_str() {
        "idle" => {}
        "config" => {
            let config = PeripheralConfig::from_env();
            println!("node_id={} stream_dir={}", config.node_id, config.stream_dir.display());
        }
        "orion-snapshot" => {
            let config = PeripheralConfig::from_env();
            let runtime = LocalNodeRuntime::new(config.orion_ipc_socket_path, config.orion_ipc_stream_socket_path);
            let snapshot = runtime.control_plane("peripherals-mem-probe")?.fetch_state_snapshot().await?;
            println!(
                "desired_resources={} observed_resources={} workloads={}",
                snapshot.state.desired.resources.len(),
                snapshot.state.observed.resources.len(),
                snapshot.state.desired.workloads.len()
            );
        }
        "inventory-linux-off" => {
            let mut config = PeripheralConfig::from_env();
            config.enable_linux_probes = false;
            let service = build_inventory_service(&config)?;
            let report = service.refresh_report(now_ms());
            println!("resources={} probes={}", report.snapshot.resources.len(), report.probe_reports.len());
        }
        "inventory-build-linux-off" => {
            let mut config = PeripheralConfig::from_env();
            config.enable_linux_probes = false;
            let service = build_inventory_service(&config)?;
            println!("probe_names={}", service.probe_names().join(","));
        }
        "inventory-linux-on" => {
            let mut config = PeripheralConfig::from_env();
            config.enable_linux_probes = true;
            let service = build_inventory_service(&config)?;
            let report = service.refresh_report(now_ms());
            println!("resources={} probes={}", report.snapshot.resources.len(), report.probe_reports.len());
        }
        "inventory-build-linux-on" => {
            let mut config = PeripheralConfig::from_env();
            config.enable_linux_probes = true;
            let service = build_inventory_service(&config)?;
            println!("probe_names={}", service.probe_names().join(","));
        }
        "lemnos-stack" => {
            let node_id = NodeId::new("probe-node");
            let _stack = LemnosPeripheralStack::new(node_id)?;
            println!("lemnos_stack=initialized");
        }
        "orion-publisher-new" => {
            let config = PeripheralConfig::from_env();
            let _publisher = OrionPeripheralPublisher::from_config(&config);
            println!("orion_publisher=initialized");
        }
        "orion-publisher-register" => {
            let config = PeripheralConfig::from_env();
            let publisher = OrionPeripheralPublisher::from_config(&config);
            publisher.register_identities().await?;
            println!("orion_publisher=registered");
        }
        "inventory-publish-linux-on" => {
            let mut config = PeripheralConfig::from_env();
            config.enable_linux_probes = true;
            let service = build_inventory_service(&config)?;
            let report = service.refresh_report(now_ms());
            let publisher = OrionPeripheralPublisher::from_config(&config);
            publisher.register_identities().await?;
            publisher.publish_snapshot_with_feedback(&report.snapshot, &[], &BTreeMap::new()).await?;
            println!("resources={} probes={} published=true", report.snapshot.resources.len(), report.probe_reports.len());
        }
        "styx-capture-probe" => {
            let node_id = NodeId::new("probe-node");
            let context = helios_peripherals::resources::DiscoveryContext::new(node_id, now_ms());
            let snapshot = CaptureProbe.discover(&context)?;
            println!("capture_resources={}", snapshot.resources.len());
        }
        "styx-probe-raw" => {
            let result = styx::probe_all_with_errors();
            println!("styx_devices={} styx_errors={}", result.devices.len(), result.errors.len());
        }
        "codec-registry" => {
            let registry = CodecRegistry::with_enabled_codecs()?;
            let _handle = registry.handle();
            println!("codec_registry=initialized");
        }
        "camera-service" => {
            let path = PathBuf::from("/tmp/helios-peripherals-mem-probe/probe.styx.sock");
            std::fs::create_dir_all(path.parent().ok_or("socket path has no parent")?)?;
            let handle = CameraService::new(first_capture_device()?).serve(&path)?;
            println!("camera_service=serving path={}", handle.path().display());
            std::mem::forget(handle);
        }
        "capture-open" => {
            let device = first_capture_device()?;
            let mode = preferred_mode(&device)?;
            let handle = CaptureRequest::new(&device).mode(mode.id.clone()).start_with_policy(CaptureStartPolicy::resilient())?;
            let first = wait_frame(&handle).await?;
            println!(
                "capture_open=format:{} width:{} height:{} payload_bytes:{}",
                first.meta().format.code,
                first.meta().format.resolution.width.get(),
                first.meta().format.resolution.height.get(),
                first.payload_bytes()
            );
            std::mem::forget(handle);
        }
        "capture-read-loop" => {
            let device = first_capture_device()?;
            let mode = preferred_mode(&device)?;
            let handle = CaptureRequest::new(&device).mode(mode.id.clone()).start_with_policy(CaptureStartPolicy::resilient())?;
            let mut frames = 0_u64;
            let started = std::time::Instant::now();
            while started.elapsed() < Duration::from_secs(5) {
                if let RecvOutcome::Data(_) = handle.recv_async().await {
                    frames += 1;
                }
            }
            println!("capture_read_loop_frames={frames}");
            std::mem::forget(handle);
        }
        other => {
            return Err(format!(
                "unknown mode '{other}', expected idle|config|orion-snapshot|inventory-linux-off|inventory-linux-on|styx-capture-probe|styx-probe-raw|codec-registry|camera-service|capture-open|capture-read-loop"
            )
            .into());
        }
    }

    println!("probe_ready hold_secs={hold_secs}");
    tokio::time::sleep(Duration::from_secs(hold_secs)).await;
    Ok(())
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

fn first_capture_device() -> Result<ProbedDevice, Box<dyn std::error::Error>> {
    probe_capture_devices().into_iter().next().ok_or_else(|| "no capture device found".into())
}

fn preferred_mode(device: &ProbedDevice) -> Result<styx::prelude::Mode, Box<dyn std::error::Error>> {
    let backend = device.backends.iter().find(|backend| backend.kind == BackendKind::Native).or_else(|| device.backends.first()).ok_or("capture device has no backend")?;
    backend
        .descriptor
        .modes
        .iter()
        .min_by_key(|mode| {
            let code = mode.id.format.code.to_string();
            let resolution = mode.id.format.resolution;
            (if code == "NV12" { 0_u8 } else { 1_u8 }, std::cmp::Reverse(u64::from(resolution.width.get()) * u64::from(resolution.height.get())))
        })
        .cloned()
        .ok_or_else(|| "capture device has no modes".into())
}

async fn wait_frame(handle: &styx::prelude::CaptureHandle) -> Result<styx::core::buffer::FrameLease, Box<dyn std::error::Error>> {
    for _ in 0..100 {
        match tokio::time::timeout(Duration::from_millis(100), handle.recv_async()).await {
            Ok(RecvOutcome::Data(frame)) => return Ok(frame),
            Ok(RecvOutcome::Empty) | Err(_) => {}
            Ok(RecvOutcome::Closed) => return Err("capture closed before frame".into()),
        }
    }
    Err("timed out waiting for frame".into())
}
