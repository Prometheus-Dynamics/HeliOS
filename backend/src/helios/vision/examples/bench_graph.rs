//! The ArUco GraphDocument on PGM frames wrapped as Styx frames: per-frame
//! graph tick percentiles, process CPU and the markers found per file.
//! `cargo run --release -p helios-vision --example bench_graph -- 4x4_50 <runs> a.pgm ...`
//! With `HALF=1` each frame carries a CPU-made half-size pyramid companion,
//! as the ISP provides on the device.

use std::{num::NonZeroU32, str::FromStr, sync::Arc, time::Instant};

use daedalus::{
    engine::{Engine, EngineConfig},
    runtime::plugins::PluginRegistry,
    transport::{Payload, Residency, TypeKey},
};
use helios_vision::{
    graphs::{FRAME_INPUT, MARKERS_OUTPUT, aruco_graph_document},
    image::GrayImage,
    plugin::{FRAMELEASE_TYPE_KEY, MarkerList, VisionPlugin},
};
use styx::{
    core::prelude::{BufferPool, ColorSpace, CompanionKind, FourCc, FrameMeta, MediaFormat, Resolution, plane_layout_from_dims},
    imports::framelease::FrameLease,
};

mod common;
use common::read_pgm;

fn grey_frame(image: &GrayImage) -> FrameLease {
    let (w, h) = (image.width() as u32, image.height() as u32);
    let format = MediaFormat::new(FourCc::from_str("GREY").unwrap(), Resolution::new(w, h).unwrap(), ColorSpace::Srgb);
    let layout = plane_layout_from_dims(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap(), 1);
    let pool = BufferPool::lazy(layout.len, 1);
    let mut frame = FrameLease::single_plane(FrameMeta::new(format, 1), pool.lease(), layout.len, layout.stride);
    frame.copy_slice_to_visible_plane(0, image.data()).unwrap();
    frame
}

fn cpu_seconds() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let fields: Vec<&str> = stat.rsplit_once(')').map(|(_, rest)| rest).unwrap_or("").split_whitespace().collect();
    let ticks = |i: usize| fields.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    (ticks(11) + ticks(12)) as f64 / 100.0
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dictionary = args.next().expect("dictionary");
    let runs: usize = args.next().expect("runs").parse().expect("runs");
    let files: Vec<String> = args.collect();
    let with_half = std::env::var_os("HALF").is_some();
    let frames: Vec<Arc<FrameLease>> = files
        .iter()
        .map(|path| {
            let image = read_pgm(path);
            let frame = grey_frame(&image);
            let frame = if with_half { frame.with_companion(CompanionKind::Pyramid { level: 1 }, grey_frame(&image.downscale2())).expect("companion") } else { frame };
            Arc::new(frame)
        })
        .collect();

    let plugin = VisionPlugin::new();
    let mut registry = PluginRegistry::new();
    registry.install(&plugin).expect("install");
    let document = aruco_graph_document(&registry, &plugin, &dictionary).expect("graph");
    let mut host = Engine::new(EngineConfig::default()).unwrap().compile_document(&registry, document).expect("compile");
    host.set_latest_input(FRAME_INPUT).unwrap();

    let mut ticks = Vec::with_capacity(runs * frames.len());
    let cpu_start = cpu_seconds();
    let started = Instant::now();
    for run in 0..runs {
        for (frame, file) in frames.iter().zip(&files) {
            let t0 = Instant::now();
            let payload = Payload::shared_with(TypeKey::new(FRAMELEASE_TYPE_KEY), frame.clone(), Residency::Cpu, None, Some(frame.payload_bytes() as u64));
            host.push_payload(FRAME_INPUT, payload);
            host.tick().expect("tick");
            let markers: MarkerList = host.take(MARKERS_OUTPUT).unwrap_or_default();
            ticks.push(t0.elapsed().as_secs_f64() * 1e3);
            if run == 0 {
                let list: Vec<String> = markers.markers.iter().map(|m| format!("[{},{}]", m.id, m.corners.iter().map(|c| format!("[{:.2},{:.2}]", c.x, c.y)).collect::<Vec<_>>().join(","))).collect();
                println!("{{\"file\":\"{file}\",\"markers\":[{}]}}", list.join(","));
            }
        }
    }
    let (elapsed, cpu) = (started.elapsed().as_secs_f64(), cpu_seconds() - cpu_start);
    ticks.sort_by(f64::total_cmp);
    let at = |q: f64| ticks[((ticks.len() - 1) as f64 * q).round() as usize];
    let avg = ticks.iter().sum::<f64>() / ticks.len() as f64;
    eprintln!(
        "{} frames{}: tick avg {avg:.3} p50 {:.3} p90 {:.3} p95 {:.3} p99 {:.3} ms; cpu {:.1}% of one core",
        ticks.len(),
        if with_half { " (with half-size companion)" } else { "" },
        at(0.5),
        at(0.9),
        at(0.95),
        at(0.99),
        100.0 * cpu / elapsed
    );
}
