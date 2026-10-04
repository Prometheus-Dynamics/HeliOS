//! Per-stage timing percentiles, CPU and memory of the ArUco detector over
//! PGM frames, as the graph runs it (downscale, threshold, quads, decode).
//! `cargo run --release -p helios-vision --example bench_replay -- 4x4_50 <runs> a.pgm b.pgm`

use std::time::Instant;

use helios_vision::aruco::{self, DecodeConfig, Dictionary};
use helios_vision::quads::{QuadConfig, find_quads, scale_quads};
use helios_vision::threshold::{ThresholdConfig, adaptive_threshold};

mod common;
use common::read_pgm;

const STAGES: [&str; 5] = ["downscale", "threshold", "find_quads", "decode", "total"];

fn main() {
    let mut args = std::env::args().skip(1);
    let dictionary = Dictionary::by_name(&args.next().expect("dictionary")).expect("known dictionary");
    let runs: usize = args.next().expect("runs").parse().expect("runs is a number");
    let images: Vec<_> = args.map(|path| read_pgm(&path)).collect();
    let decode = DecodeConfig { dictionary, ..DecodeConfig::default() };

    let mut samples: Vec<Vec<f64>> = vec![Vec::with_capacity(runs * images.len()); STAGES.len()];
    let mut markers = 0usize;
    // BENCH_INTERVAL_MS paces frames like a camera (the detector idles between frames).
    let interval = std::env::var("BENCH_INTERVAL_MS").ok().and_then(|v| v.parse::<f64>().ok()).map(|ms| std::time::Duration::from_secs_f64(ms / 1e3));
    let faults_start = minor_faults();
    let cpu_start = cpu_seconds();
    let started = Instant::now();
    for _ in 0..runs {
        for image in &images {
            if let Some(interval) = interval {
                std::thread::sleep(interval);
            }
            let t0 = Instant::now();
            let small = image.downscale2();
            let t1 = Instant::now();
            let binary = adaptive_threshold(&small, &ThresholdConfig::default());
            let t2 = Instant::now();
            let quads = scale_quads(&find_quads(&binary, &QuadConfig::default()), 2.0);
            let t3 = Instant::now();
            markers += aruco::decode_quads(image.view(), &quads, &decode).len();
            let t4 = Instant::now();
            for (stage, (a, b)) in [(t0, t1), (t1, t2), (t2, t3), (t3, t4), (t0, t4)].into_iter().enumerate() {
                samples[stage].push((b - a).as_secs_f64() * 1e3);
            }
        }
    }
    let elapsed = started.elapsed().as_secs_f64();
    let cpu = cpu_seconds() - cpu_start;
    let faults = minor_faults() - faults_start;
    let frames = runs * images.len();

    println!("{frames} frames ({}x{}), {:.2} markers/frame", images[0].width(), images[0].height(), markers as f64 / frames as f64);
    println!("{:<11} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}", "stage ms", "avg", "p50", "p90", "p95", "p99", "max");
    for (name, values) in STAGES.iter().zip(samples.iter_mut()) {
        values.sort_by(f64::total_cmp);
        let at = |q: f64| values[((values.len() - 1) as f64 * q).round() as usize];
        let avg = values.iter().sum::<f64>() / values.len() as f64;
        println!("{name:<11} {avg:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3}", at(0.5), at(0.9), at(0.95), at(0.99), values[values.len() - 1]);
    }
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| status.lines().find_map(|line| line.strip_prefix(name)).map(str::trim).unwrap_or("?").to_string();
    println!("minor page faults per frame {:.1}", faults as f64 / (runs * images.len()) as f64);
    println!("cpu {:.1}% of one core ({:.3} ms/frame), rss {}, peak rss {}, threads {}", 100.0 * cpu / elapsed, cpu * 1e3 / frames as f64, field("VmRSS:"), field("VmHWM:"), field("Threads:"));
}

/// Minor page faults of this process so far.
fn minor_faults() -> u64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    stat.rsplit_once(')').map(|(_, rest)| rest).unwrap_or("").split_whitespace().nth(7).and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// User plus system CPU time of this process.
fn cpu_seconds() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let fields: Vec<&str> = stat.rsplit_once(')').map(|(_, rest)| rest).unwrap_or("").split_whitespace().collect();
    let ticks = |i: usize| fields.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    (ticks(11) + ticks(12)) as f64 / 100.0
}
