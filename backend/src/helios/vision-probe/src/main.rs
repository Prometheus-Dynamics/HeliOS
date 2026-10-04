//! Run the ArUco graph on a live camera.
//!
//! ```sh
//! helios-vision-probe [--dict 4x4_50|36h11] [--size 1280x800] [--fps 30] [--frames 300]
//!                     [--camera <name filter>] [--save <dir>]
//! ```
//!
//! Plans a luma capture with Styx, compiles the ArUco `GraphDocument`, feeds
//! every frame into a latest-only host input and prints detections, frame
//! rate and graph time. Every 30th frame it also times the stages directly.
//! `--save` writes the first and last frames as PGM plus their detections.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use daedalus::{
    engine::{Engine, EngineConfig},
    runtime::plugins::PluginRegistry,
    transport::{Payload, Residency, TypeKey},
};
use helios_vision::{
    aruco::{self, DecodeConfig, Dictionary},
    graphs::{FRAME_INPUT, MARKERS_OUTPUT, aruco_graph_document},
    image::GrayImage,
    plugin::{FRAMELEASE_TYPE_KEY, MarkerList, VisionPlugin, framelease_to_gray},
    quads::{QuadConfig, find_quads, scale_quads},
    threshold::{ThresholdConfig, adaptive_threshold},
};
use styx::prelude::*;

struct Args {
    dictionary: String,
    size: (u32, u32),
    fps: u32,
    frames: usize,
    camera: Option<String>,
    save: Option<PathBuf>,
    /// Time the stages directly every 30th frame (adds CPU; off for clean
    /// process measurements).
    stage_sampling: bool,
    /// Ask the ISP for a half-size luma companion (the downscale node uses it).
    pyramid: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args { dictionary: "4x4_50".into(), size: (1280, 800), fps: 30, frames: 300, camera: None, save: None, stage_sampling: true, pyramid: true };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--dict" => args.dictionary = value()?,
            "--size" => {
                let v = value()?;
                let (w, h) = v.split_once('x').ok_or("--size is WxH")?;
                args.size = (w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?);
            }
            "--fps" => args.fps = value()?.parse().map_err(|_| "bad --fps")?,
            "--frames" => args.frames = value()?.parse().map_err(|_| "bad --frames")?,
            "--camera" => args.camera = Some(value()?),
            "--save" => args.save = Some(PathBuf::from(value()?)),
            "--no-stage-sampling" => args.stage_sampling = false,
            "--no-pyramid" => args.pyramid = false,
            "-h" | "--help" => {
                return Err("usage: helios-vision-probe [--dict 4x4_50|36h11] [--size WxH] [--fps N] [--frames N] [--camera NAME] [--save DIR] [--no-stage-sampling] [--no-pyramid]".into());
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if Dictionary::by_name(&args.dictionary).is_none() {
        return Err(format!("unknown dictionary {}", args.dictionary));
    }
    Ok(args)
}

fn residency(frame: &FrameLease) -> Residency {
    match frame.residency() {
        FrameResidency::HostOwned | FrameResidency::CompressedPacket => Residency::Cpu,
        FrameResidency::HostExternal | FrameResidency::Dmabuf => Residency::External,
        FrameResidency::GpuTexture => Residency::Gpu,
    }
}

fn save_pgm(path: &std::path::Path, image: &GrayImage) -> std::io::Result<()> {
    let mut bytes = format!("P5\n{} {}\n255\n", image.width(), image.height()).into_bytes();
    bytes.extend_from_slice(image.data());
    std::fs::write(path, bytes)
}

#[derive(Default)]
struct Stats {
    frames: usize,
    with_markers: usize,
    tick_total: Duration,
    tick_max: Duration,
    ticks_ms: Vec<f64>,
    ids: std::collections::BTreeMap<String, usize>,
    stage_samples: usize,
    stage_total: [Duration; 4],
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    if let Err(error) = run(args) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let plugin = VisionPlugin::new();
    let mut registry = PluginRegistry::new();
    registry.install(&plugin)?;
    let document = aruco_graph_document(&registry, &plugin, &args.dictionary)?;
    let mut host = Engine::new(EngineConfig::default())?.compile_document(&registry, document)?;
    host.set_latest_input(FRAME_INPUT)?;
    for edge in host.explain_plan().edges {
        if !edge.adapter_steps.is_empty() {
            println!("plan: {} -> {} adapters {:?}", edge.from_port, edge.to_port, edge.adapter_steps);
        }
    }

    let mut cameras = styx::probe_all();
    if let Some(filter) = &args.camera {
        let filter = filter.to_lowercase();
        cameras.retain(|c| std::iter::once(&c.identity.display).chain(&c.identity.keys).any(|k| k.to_lowercase().contains(&filter)));
    }
    let mut wants = Frames::gray().size(args.size.0, args.size.1).fps(args.fps).latest();
    if args.pyramid {
        wants = wants.pyramid(1);
    }
    let plan = wants.plan_best(&cameras)?;
    println!("delivered: {:?}", plan.delivered());
    print!("{plan}");
    let mut frames = plan.start()?;

    let dictionary = Dictionary::by_name(&args.dictionary).expect("checked");
    let decode_config = DecodeConfig { dictionary, ..DecodeConfig::default() };
    let mut stats = Stats::default();
    let started = Instant::now();
    let process_at_start = ProcessSample::read();
    let mut last_report = Instant::now();
    let mut last_ids = String::new();

    while stats.frames < args.frames {
        let RecvOutcome::Data(frame) = frames.next_frame(Duration::from_secs(2)) else {
            println!("no frame within 2 s");
            break;
        };
        stats.frames += 1;
        let first_or_last = stats.frames == 1 || stats.frames == args.frames;
        let sample_stages = args.stage_sampling && stats.frames % 30 == 1;
        let gray = if first_or_last || sample_stages { Some(framelease_to_gray(&frame)?.0) } else { None };

        let bytes = frame.payload_bytes() as u64;
        let residency = residency(&frame);
        let payload = Payload::shared_with(TypeKey::new(FRAMELEASE_TYPE_KEY), Arc::new(frame), residency, None, Some(bytes));
        let tick_started = Instant::now();
        host.push_payload(FRAME_INPUT, payload);
        host.tick()?;
        let tick = tick_started.elapsed();
        stats.tick_total += tick;
        stats.tick_max = stats.tick_max.max(tick);
        stats.ticks_ms.push(tick.as_secs_f64() * 1e3);
        let markers: MarkerList = host.take(MARKERS_OUTPUT).unwrap_or_default();
        if !markers.markers.is_empty() {
            stats.with_markers += 1;
        }
        let mut ids: Vec<String> = markers.markers.iter().map(|m| format!("{}:{}", m.dictionary, m.id)).collect();
        ids.sort();
        for id in &ids {
            *stats.ids.entry(id.clone()).or_default() += 1;
        }
        let ids = ids.join(",");
        if ids != last_ids || last_report.elapsed() > Duration::from_secs(2) {
            let fps = stats.frames as f64 / started.elapsed().as_secs_f64();
            let corners: Vec<String> = markers.markers.iter().map(|m| format!("{}@({:.0},{:.0})", m.id, m.center.x, m.center.y)).collect();
            println!("frame {:>5} {:>5.1} fps tick {:>6.2} ms markers [{}]", stats.frames, fps, tick.as_secs_f64() * 1e3, corners.join(" "));
            last_ids = ids;
            last_report = Instant::now();
        }

        if let Some(gray) = &gray {
            if sample_stages {
                let t0 = Instant::now();
                let small = gray.downscale2();
                let binary = adaptive_threshold(&small, &ThresholdConfig::default());
                let t1 = Instant::now();
                let quads = scale_quads(&find_quads(&binary, &QuadConfig::default()), 2.0);
                let t2 = Instant::now();
                let _ = aruco::decode_quads(gray.view(), &quads, &decode_config);
                let t3 = Instant::now();
                stats.stage_samples += 1;
                stats.stage_total[0] += t1 - t0;
                stats.stage_total[1] += t2 - t1;
                stats.stage_total[2] += t3 - t2;
                stats.stage_total[3] += Duration::from_nanos(quads.len() as u64);
            }
            if first_or_last && let Some(dir) = &args.save {
                std::fs::create_dir_all(dir)?;
                let name = if stats.frames == 1 { "first" } else { "last" };
                save_pgm(&dir.join(format!("{name}.pgm")), gray)?;
                std::fs::write(dir.join(format!("{name}.json")), serde_json::to_string_pretty(&markers_json(&markers))?)?;
            }
        }
    }

    let elapsed = started.elapsed().as_secs_f64();
    let process = ProcessSample::read();
    let cpu_seconds = process.cpu_seconds - process_at_start.cpu_seconds;
    let samples = stats.stage_samples.max(1) as f64;
    let ms = |d: Duration| d.as_secs_f64() * 1e3 / samples;
    let summary = serde_json::json!({
        "frames": stats.frames,
        "fps": stats.frames as f64 / elapsed,
        "frames_with_markers": stats.with_markers,
        "tick_ms_avg": stats.tick_total.as_secs_f64() * 1e3 / stats.frames.max(1) as f64,
        "tick_ms_max": stats.tick_max.as_secs_f64() * 1e3,
        "tick_ms_percentiles": percentiles(&mut stats.ticks_ms),
        "process": {
            "cpu_percent_of_one_core": 100.0 * cpu_seconds / elapsed,
            "cpu_ms_per_frame": cpu_seconds * 1e3 / stats.frames.max(1) as f64,
            "rss_mib": process.rss_kib as f64 / 1024.0,
            "peak_rss_mib": process.peak_rss_kib as f64 / 1024.0,
            "threads": process.threads,
        },
        "stage_ms_avg": {
            "threshold": ms(stats.stage_total[0]),
            "find_quads": ms(stats.stage_total[1]),
            "decode": ms(stats.stage_total[2]),
            "quads_per_frame": stats.stage_total[3].as_nanos() as f64 / samples,
        },
        "detections": stats.ids,
    });
    println!("summary {summary}");
    Ok(())
}

/// p50/p90/p95/p99 of the given samples (sorted in place).
fn percentiles(values: &mut [f64]) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    let at = |q: f64| values.get(((values.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or(0.0);
    serde_json::json!({ "p50": at(0.50), "p90": at(0.90), "p95": at(0.95), "p99": at(0.99) })
}

/// CPU time, memory and threads of this process from /proc.
struct ProcessSample {
    cpu_seconds: f64,
    rss_kib: u64,
    peak_rss_kib: u64,
    threads: u64,
}

impl ProcessSample {
    fn read() -> Self {
        let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        let field = |name: &str| status.lines().find_map(|line| line.strip_prefix(name)).and_then(|rest| rest.split_whitespace().next()).and_then(|v| v.parse().ok()).unwrap_or(0);
        // utime and stime are fields 14 and 15, after the parenthesised command name.
        let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
        let after_comm = stat.rsplit_once(')').map(|(_, rest)| rest).unwrap_or("");
        let fields: Vec<&str> = after_comm.split_whitespace().collect();
        let ticks = |i: usize| fields.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        const CLOCK_TICKS_PER_SECOND: f64 = 100.0;
        Self { cpu_seconds: (ticks(11) + ticks(12)) as f64 / CLOCK_TICKS_PER_SECOND, rss_kib: field("VmRSS:"), peak_rss_kib: field("VmHWM:"), threads: field("Threads:") }
    }
}

fn markers_json(markers: &MarkerList) -> serde_json::Value {
    serde_json::Value::Array(
        markers
            .markers
            .iter()
            .map(|m| serde_json::json!({ "dictionary": m.dictionary, "id": m.id, "hamming": m.hamming, "corners": m.corners.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>() }))
            .collect(),
    )
}
