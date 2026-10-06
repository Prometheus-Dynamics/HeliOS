//! Run a vision graph document on a live camera, the way helios-engine runs it.
//!
//! ```sh
//! helios-vision-probe [--dict 36h11|4x4_50 | --graph FILE] [--plugin LIB] [--size 1280x800]
//!                     [--fps 30] [--frames 300] [--camera <name filter>] [--no-pyramid]
//!                     [--metrics off|basic|timing|detailed|profile]
//! ```
//!
//! Loads the Eidos plugin library (`libhelios_eidos_plugin.so`, built in the same cargo build as
//! this probe) through Daedalus's plugin loader, compiles the `GraphDocument` (by default the
//! engine's stored AprilTag 36h11 or ArUco 4x4_50 detector), captures luma frames with Styx and
//! drives the graph from them: the capture thread pushes each `FrameLease` into a latest-only
//! host input without copying it, and the graph thread runs `HostGraph::drive_blocking`, one
//! tick per frame. It prints detections, the frame rate, the push-to-outputs latency and, with
//! `--metrics`, Daedalus's per-node timings, then a JSON summary.
//!
//! Timings are only meaningful on the target board (the Raze CM5); build with
//! `cargo build --release -p helios-vision-probe -p helios-eidos-plugin`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use daedalus::{
    PluginLibrary,
    data::model::Value,
    dylib::InstallPath,
    engine::{Engine, EngineConfig, GpuBackend, MetricsLevel, RuntimeMode},
    planner::GraphDocument,
    runtime::{ExecutionTelemetry, plugins::PluginRegistry},
};
use styx::core::daedalus::{StyxFramesPlugin, frame_payload};
use styx::prelude::*;

const APRILTAG_DOCUMENT: &str = include_str!("../../engine/graphs/apriltag-36h11.graph.json");
const ARUCO_DOCUMENT: &str = include_str!("../../engine/graphs/aruco-4x4_50.graph.json");
/// Host ports of the Eidos detector templates.
const FRAME_INPUT: &str = "frame";
const DETECTIONS_OUTPUT: &str = "detections";
const PLUGIN_FILE: &str = "libhelios_eidos_plugin.so";
const PLUGIN_DIR: &str = "/usr/lib/helios/plugins/daedalus";
const USAGE: &str = "usage: helios-vision-probe [--dict 36h11|4x4_50 | --graph FILE] [--plugin LIB] [--size WxH] [--fps N] [--frames N] [--camera NAME] [--no-pyramid] [--metrics LEVEL]";

struct Args {
    document: String,
    plugin: PathBuf,
    size: (u32, u32),
    fps: u32,
    frames: usize,
    camera: Option<String>,
    /// Ask the ISP for a half-size luma companion (mask prep thresholds it).
    pyramid: bool,
    metrics: MetricsLevel,
}

fn default_plugin() -> PathBuf {
    if let Some(path) = std::env::var_os("HELIOS_EIDOS_PLUGIN") {
        return PathBuf::from(path);
    }
    let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join(PLUGIN_FILE)));
    beside.filter(|path| path.is_file()).unwrap_or_else(|| PathBuf::from(PLUGIN_DIR).join(PLUGIN_FILE))
}

fn parse_metrics(value: &str) -> Result<MetricsLevel, String> {
    Ok(match value.to_ascii_lowercase().as_str() {
        "off" => MetricsLevel::Off,
        "basic" => MetricsLevel::Basic,
        "timing" => MetricsLevel::Timing,
        "detailed" => MetricsLevel::Detailed,
        "profile" => MetricsLevel::Profile,
        other => return Err(format!("unknown metrics level {other}")),
    })
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args { document: APRILTAG_DOCUMENT.into(), plugin: default_plugin(), size: (1280, 800), fps: 30, frames: 300, camera: None, pyramid: true, metrics: MetricsLevel::Off };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--dict" => {
                args.document = match value()?.as_str() {
                    "36h11" | "apriltag_36h11" => APRILTAG_DOCUMENT.into(),
                    "4x4_50" | "aruco_4x4_50" => ARUCO_DOCUMENT.into(),
                    other => return Err(format!("no stored graph for {other}; pass --graph FILE")),
                }
            }
            "--graph" => {
                let path = value()?;
                args.document = std::fs::read_to_string(&path).map_err(|error| format!("{path}: {error}"))?;
            }
            "--plugin" => args.plugin = PathBuf::from(value()?),
            "--size" => {
                let v = value()?;
                let (w, h) = v.split_once('x').ok_or("--size is WxH")?;
                args.size = (w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?);
            }
            "--fps" => args.fps = value()?.parse().map_err(|_| "bad --fps")?,
            "--frames" => args.frames = value()?.parse().map_err(|_| "bad --frames")?,
            "--camera" => args.camera = Some(value()?),
            "--no-pyramid" => args.pyramid = false,
            "--metrics" => args.metrics = parse_metrics(&value()?)?,
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }
    Ok(args)
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

#[derive(Default)]
struct Stats {
    ticks: usize,
    with_detections: usize,
    latency_ms: Vec<f64>,
    graph_ms: Vec<f64>,
    ids: BTreeMap<i64, usize>,
    /// Per node label: calls and summed handler time.
    nodes: BTreeMap<String, (u64, Duration)>,
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut registry = PluginRegistry::new();
    registry.install(&StyxFramesPlugin::new())?;
    // Safety: the library is HeliOS's own Eidos plugin, built with this probe.
    #[allow(unsafe_code)]
    let library = unsafe { PluginLibrary::load(&args.plugin) }.map_err(|error| format!("{}: {error}", args.plugin.display()))?;
    // Frames and Eidos's hand-off types are Rust types: only the Rust-ABI path carries them.
    library.install_into_as(&mut registry, InstallPath::RustAbi).map_err(|error| format!("{}: {error} (build it in the same cargo build as the probe)", args.plugin.display()))?;

    let document = GraphDocument::from_json(&args.document)?;
    let mut config = EngineConfig { gpu: GpuBackend::Cpu, ..EngineConfig::default() }.with_metrics_level(args.metrics);
    config.planner.enable_gpu = false;
    config.runtime.mode = RuntimeMode::Serial;
    let mut host = Engine::new(config)?.compile_document(&registry, document)?;
    host.set_latest_input(FRAME_INPUT)?;
    let explanation = host.explain_plan();
    let labels: Vec<String> = explanation.nodes.iter().map(|node| node.label.clone().unwrap_or_else(|| node.id.clone())).collect();
    for edge in &explanation.edges {
        if !edge.adapter_steps.is_empty() {
            println!("plan: {}.{} -> {}.{} adapters {:?}", labels[edge.from_node], edge.from_port, labels[edge.to_node], edge.to_port, edge.adapter_steps);
        }
    }
    println!("plan: {} nodes, {} edges, {} with adapters", explanation.nodes.len(), explanation.edges.len(), explanation.edges.iter().filter(|edge| !edge.adapter_steps.is_empty()).count());

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

    // The graph thread: one tick per pushed frame.
    let input = host.bind_payload_input(FRAME_INPUT);
    let stop = host.stop_handle();
    let pushed_at: Arc<Mutex<Option<Instant>>> = Arc::default();
    let graph_thread = {
        let (stop, pushed_at) = (stop.clone(), pushed_at.clone());
        std::thread::Builder::new().name("probe-graph".into()).spawn(move || -> Result<Stats, String> {
            let mut stats = Stats::default();
            let mut last_report = Instant::now();
            host.drive_blocking(&stop, |host, turn| {
                let latency = pushed_at.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).map(|at| at.elapsed());
                stats.ticks += 1;
                if let Some(latency) = latency {
                    stats.latency_ms.push(latency.as_secs_f64() * 1e3);
                }
                if let Some(telemetry) = &turn.telemetry {
                    record_telemetry(&mut stats, telemetry, &labels);
                }
                let ids = host.take_payload(DETECTIONS_OUTPUT).map(|payload| detection_ids(host.inspect_payload(&payload).value())).unwrap_or_default();
                if !ids.is_empty() {
                    stats.with_detections += 1;
                }
                for id in &ids {
                    *stats.ids.entry(*id).or_default() += 1;
                }
                if last_report.elapsed() > Duration::from_secs(2) {
                    println!("tick {:>5} latency {:>6.2} ms ids {:?}", stats.ticks, latency.unwrap_or_default().as_secs_f64() * 1e3, ids);
                    last_report = Instant::now();
                }
                Ok(())
            })
            .map_err(|error| error.to_string())?;
            Ok(stats)
        })?
    };

    let started = Instant::now();
    let process_at_start = ProcessSample::read();
    let mut captured = 0;
    while captured < args.frames {
        let RecvOutcome::Data(frame) = frames.next_frame(Duration::from_secs(2)) else {
            println!("no frame within 2 s");
            break;
        };
        captured += 1;
        *pushed_at.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
        input.push(frame_payload(frame));
    }
    // Let the last frame finish, then stop the graph thread.
    std::thread::sleep(Duration::from_millis(200));
    stop.stop();
    let mut stats = graph_thread.join().map_err(|_| "graph thread panicked")??;

    let elapsed = started.elapsed().as_secs_f64();
    let process = ProcessSample::read();
    let cpu_seconds = process.cpu_seconds - process_at_start.cpu_seconds;
    let nodes: BTreeMap<&String, serde_json::Value> =
        stats.nodes.iter().map(|(label, (calls, handler))| (label, serde_json::json!({ "calls": calls, "handler_us_mean": handler.as_secs_f64() * 1e6 / (*calls).max(1) as f64 }))).collect();
    let summary = serde_json::json!({
        "frames_captured": captured,
        "ticks": stats.ticks,
        "frames_replaced_before_tick": captured.saturating_sub(stats.ticks),
        "fps": stats.ticks as f64 / elapsed,
        "ticks_with_detections": stats.with_detections,
        "detections_total": stats.ids.values().sum::<usize>(),
        "push_to_outputs_ms": percentiles(&mut stats.latency_ms),
        "graph_ms": if stats.graph_ms.is_empty() { serde_json::Value::Null } else { percentiles(&mut stats.graph_ms) },
        "metrics_level": format!("{:?}", args.metrics),
        "nodes": nodes,
        "process": {
            "cpu_percent_of_one_core": 100.0 * cpu_seconds / elapsed,
            "cpu_ms_per_frame": cpu_seconds * 1e3 / captured.max(1) as f64,
            "rss_mib": process.rss_kib as f64 / 1024.0,
            "peak_rss_mib": process.peak_rss_kib as f64 / 1024.0,
            "threads": process.threads,
        },
        "detections_by_id": stats.ids,
    });
    println!("summary {summary}");
    Ok(())
}

fn record_telemetry(stats: &mut Stats, telemetry: &ExecutionTelemetry, labels: &[String]) {
    if telemetry.metrics_level == MetricsLevel::Off {
        return;
    }
    stats.graph_ms.push(telemetry.graph_duration.as_secs_f64() * 1e3);
    for (index, node) in telemetry.node_metrics.iter() {
        let label = labels.get(index).cloned().unwrap_or_else(|| index.to_string());
        let entry = stats.nodes.entry(label).or_default();
        entry.0 += node.calls as u64;
        entry.1 += node.handler_duration;
    }
}

/// Marker ids of an `eidos:detections` value (`{ detections: [{ id, .. }] }`).
fn detection_ids(value: Option<&Value>) -> Vec<i64> {
    match value.and_then(|value| value.field("detections")) {
        Some(Value::List(items)) => items
            .iter()
            .filter_map(|item| match item.field("id") {
                Some(Value::Int(id)) => Some(*id),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
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
