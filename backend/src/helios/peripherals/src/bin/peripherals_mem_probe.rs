use std::{collections::BTreeMap, path::PathBuf, time::Duration};

use helios_peripherals::{
    config::PeripheralConfig,
    model::{NodeId, ResourceDescriptor, ResourceKind},
    provider::{OrionPeripheralPublisher, streams::PeripheralStreamWriter},
    resources::{CaptureProbe, DiscoveryProbe, LemnosPeripheralStack, ResourceBuilder},
    runtime::build_inventory_service,
};
use orion::client::LocalNodeRuntime;
use styx::{
    BackendHandle, BackendKind, ProbedBackend, ProbedDevice,
    codec::CodecRegistry,
    prelude::{CaptureRequest, CaptureStartPolicy, FfmpegMjpegEncoder, RecvOutcome},
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
            let libcamera = styx::prelude::probe_libcamera();
            println!("styx_devices={} styx_errors={} libcamera_devices={}", result.devices.len(), result.errors.len(), libcamera.len());
        }
        "codec-registry" => {
            let registry = CodecRegistry::with_enabled_codecs()?;
            let _handle = registry.handle();
            println!("codec_registry=initialized");
        }
        "mjpeg-nv12" => {
            let _encoder = FfmpegMjpegEncoder::new_nv12()?;
            println!("mjpeg_nv12_encoder=initialized");
        }
        "mjpeg-rgb24" => {
            let _encoder = FfmpegMjpegEncoder::new_rgb24()?;
            println!("mjpeg_rgb24_encoder=initialized");
        }
        "stream-writer" => {
            let node_id = NodeId::new("probe-node");
            let resource = dummy_capture_resource(node_id.clone())?;
            let path = PathBuf::from("/tmp/helios-peripherals-mem-probe/probe.raw.stream.json");
            let _writer = PeripheralStreamWriter::create_capture_channel(&path, node_id.as_str(), &resource)?;
            println!("stream_writer=initialized path={}", path.display());
        }
        "capture-open" => {
            let device = first_capture_device()?;
            let mode = preferred_mode(&device)?;
            let handle = CaptureRequest::new(&device).backend(BackendKind::Libcamera).mode(mode.id.clone()).start_with_policy(CaptureStartPolicy::resilient())?;
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
            let handle = CaptureRequest::new(&device).backend(BackendKind::Libcamera).mode(mode.id.clone()).start_with_policy(CaptureStartPolicy::resilient())?;
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
        "capture-publish-writer" => {
            let device = first_capture_device()?;
            let mode = preferred_mode(&device)?;
            let handle = CaptureRequest::new(&device).backend(BackendKind::Libcamera).mode(mode.id.clone()).start_with_policy(CaptureStartPolicy::resilient())?;
            let node_id = NodeId::new("probe-node");
            let resource = dummy_capture_resource(node_id.clone())?;
            let path = PathBuf::from("/tmp/helios-peripherals-mem-probe/probe.raw.stream.json");
            let mut writer = PeripheralStreamWriter::create_capture_channel(&path, node_id.as_str(), &resource)?;
            let mut frames = 0_u64;
            let started = std::time::Instant::now();
            while started.elapsed() < Duration::from_secs(5) {
                if let RecvOutcome::Data(frame) = handle.recv_async().await {
                    writer.publish_capture_frame(&frame)?;
                    frames += 1;
                }
            }
            println!("capture_publish_writer_frames={frames}");
            std::mem::forget(handle);
        }
        other => {
            return Err(format!(
                "unknown mode '{other}', expected idle|config|orion-snapshot|inventory-linux-off|inventory-linux-on|styx-capture-probe|styx-probe-raw|codec-registry|mjpeg-nv12|mjpeg-rgb24|stream-writer|capture-open|capture-read-loop|capture-publish-writer"
            )
            .into());
        }
    }

    println!("probe_ready hold_secs={hold_secs}");
    tokio::time::sleep(Duration::from_secs(hold_secs)).await;
    Ok(())
}

fn dummy_capture_resource(node_id: NodeId) -> Result<ResourceDescriptor, Box<dyn std::error::Error>> {
    Ok(ResourceBuilder::new(node_id, ResourceKind::CaptureDevice, "probe-camera", "Probe Camera")?.capability("capture", Some("probe")).build())
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

fn first_capture_device() -> Result<ProbedDevice, Box<dyn std::error::Error>> {
    let mut result = styx::probe_all_with_errors();
    for device in styx::prelude::probe_libcamera() {
        let device_id = device.id.clone();
        let already_present = result.devices.iter().any(|existing| existing.backends.iter().any(|backend| matches!(&backend.handle, BackendHandle::Libcamera { id } if id == &device_id)));
        if already_present {
            continue;
        }
        result.devices.push(styx::ProbedDevice {
            identity: styx::DeviceIdentity { display: device_id.clone(), keys: vec![device_id.clone()] },
            backends: vec![ProbedBackend { kind: BackendKind::Libcamera, handle: BackendHandle::Libcamera { id: device_id }, descriptor: device.descriptor, properties: device.properties }],
        });
    }
    result.devices.into_iter().find(|device| device.backends.iter().any(|backend| backend.kind == BackendKind::Libcamera)).ok_or_else(|| "no libcamera capture device found".into())
}

fn preferred_mode(device: &ProbedDevice) -> Result<styx::prelude::Mode, Box<dyn std::error::Error>> {
    let backend = device.backends.iter().find(|backend| backend.kind == BackendKind::Libcamera).or_else(|| device.backends.first()).ok_or("capture device has no backend")?;
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
