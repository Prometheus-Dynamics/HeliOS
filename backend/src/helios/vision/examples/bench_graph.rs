//! Graph overhead: the compiled ArUco GraphDocument against the same stages
//! called directly, on PGM frames wrapped as Styx frames.
//! `cargo run --release -p helios-vision --example bench_graph -- 4x4_50 <runs> a.pgm ...`

use std::{num::NonZeroU32, str::FromStr, sync::Arc, time::Instant};

use daedalus::{
    engine::{Engine, EngineConfig},
    runtime::plugins::PluginRegistry,
    transport::{Payload, Residency, TypeKey},
};
use helios_vision::{
    aruco::{self, DecodeConfig, DetectorConfig, Dictionary},
    graphs::aruco_graph_document,
    image::GrayImage,
    plugin::{FRAMELEASE_TYPE_KEY, MarkerList, VisionPlugin, framelease_to_gray},
};
use styx::{
    core::prelude::{BufferPool, ColorSpace, FourCc, FrameMeta, MediaFormat, Resolution, plane_layout_from_dims},
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

fn report(name: &str, values: &mut [f64]) {
    values.sort_by(f64::total_cmp);
    let at = |q: f64| values[((values.len() - 1) as f64 * q).round() as usize];
    let avg = values.iter().sum::<f64>() / values.len() as f64;
    println!("{name:<16} avg {avg:.3} p50 {:.3} p90 {:.3} p95 {:.3} p99 {:.3} ms", at(0.5), at(0.9), at(0.95), at(0.99));
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dictionary_name = args.next().expect("dictionary");
    let dictionary = Dictionary::by_name(&dictionary_name).expect("known dictionary");
    let runs: usize = args.next().expect("runs").parse().expect("runs");
    let frames: Vec<Arc<FrameLease>> = args.map(|path| Arc::new(grey_frame(&read_pgm(&path)))).collect();

    let plugin = VisionPlugin::new();
    let mut registry = PluginRegistry::new();
    registry.install(&plugin).expect("install");
    let document = aruco_graph_document(&registry, &plugin, &dictionary_name).expect("graph");
    let mut host = Engine::new(EngineConfig::default()).unwrap().compile_document(&registry, document).expect("compile");
    host.set_latest_input("frame").unwrap();

    let config = DetectorConfig { decode: DecodeConfig { dictionary, ..DecodeConfig::default() }, ..DetectorConfig::default() };
    let (mut graph, mut direct, mut copy) = (Vec::new(), Vec::new(), Vec::new());
    let (mut graph_markers, mut direct_markers) = (0, 0);
    for _ in 0..runs {
        for frame in &frames {
            let t0 = Instant::now();
            let payload = Payload::shared_with(TypeKey::new(FRAMELEASE_TYPE_KEY), frame.clone(), Residency::Cpu, None, Some(frame.payload_bytes() as u64));
            host.push_payload("frame", payload);
            host.tick().expect("tick");
            let markers: MarkerList = host.take("markers").unwrap_or_default();
            graph_markers += markers.markers.len();
            let t1 = Instant::now();
            let gray = framelease_to_gray(frame).expect("luma");
            let t2 = Instant::now();
            direct_markers += aruco::detect(&gray.0, &config).len();
            let t3 = Instant::now();
            graph.push((t1 - t0).as_secs_f64() * 1e3);
            copy.push((t2 - t1).as_secs_f64() * 1e3);
            direct.push((t3 - t1).as_secs_f64() * 1e3);
        }
    }
    let n = runs * frames.len();
    println!("{n} frames; markers graph {graph_markers} direct {direct_markers}");
    report("graph tick", &mut graph);
    report("direct (copy+det)", &mut direct);
    report("luma copy", &mut copy);
}
