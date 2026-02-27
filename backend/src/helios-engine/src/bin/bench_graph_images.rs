use helios_engine::graph::GraphHandle;
use helios_engine::stream::{PipelineGraphMetrics, PipelineNodeRuntimeMetrics};
use image::DynamicImage;
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BenchMode {
    Cpu,
    Gpu,
    Auto,
}

impl BenchMode {
    fn from_env() -> Self {
        match env::var("HELIOS_BENCH_MODE").ok().map(|v| v.to_lowercase()) {
            Some(v) if v == "cpu" => Self::Cpu,
            Some(v) if v == "gpu" => Self::Gpu,
            _ => Self::Auto,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
            Self::Auto => "auto",
        }
    }
}

#[derive(Clone, Debug)]
struct TopNode {
    node: String,
    avg_ms: f64,
    fps: f64,
    samples: u64,
    last_error: Option<String>,
}

fn parse_u64_env(name: &str, default: u64) -> u64 {
    env::var(name).ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(default)
}

fn parse_usize_env(name: &str, default: usize) -> usize {
    env::var(name).ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(default)
}

fn load_graph_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read graph {path:?}: {e}"))?;
    let doc: Value = serde_json::from_str(&text).map_err(|e| format!("parse graph json {path:?}: {e}"))?;
    Ok(doc.get("graph").cloned().unwrap_or(doc))
}

fn upsert_metadata_string(graph: &mut Value, key: &str, value: &str) {
    let Some(obj) = graph.as_object_mut() else {
        return;
    };
    let md = obj.entry("metadata".to_string()).or_insert_with(|| Value::Object(Default::default()));
    let Some(md_obj) = md.as_object_mut() else {
        return;
    };
    md_obj.insert(key.to_string(), Value::String(value.to_string()));
}

fn set_const_input_mode(node: &mut Value, mode: &str) {
    let Some(node_obj) = node.as_object_mut() else {
        return;
    };
    let const_inputs = node_obj.entry("const_inputs".to_string()).or_insert_with(|| Value::Array(Vec::new()));
    let Some(items) = const_inputs.as_array_mut() else {
        return;
    };

    let mut replaced = false;
    for entry in items.iter_mut() {
        let Some(pair) = entry.as_array_mut() else {
            continue;
        };
        if pair.len() < 2 {
            continue;
        }
        let Some(name) = pair.first().and_then(Value::as_str) else {
            continue;
        };
        if name != "mode" {
            continue;
        }
        pair[1] = json!({
            "type": "String",
            "value": mode
        });
        replaced = true;
        break;
    }

    if !replaced {
        items.push(json!([
            "mode",
            {
                "type": "String",
                "value": mode
            }
        ]));
    }
}

fn apply_node_mode_override(node: &mut Value, mode: BenchMode) {
    let Some(id) = node.get("id").and_then(Value::as_str) else {
        return;
    };
    match id {
        "cv:image:blur" | "cv:color:grayscale" => {
            if let Some(obj) = node.as_object_mut() {
                match mode {
                    BenchMode::Cpu => {
                        obj.insert("compute".to_string(), Value::String("CpuOnly".to_string()));
                        set_const_input_mode(node, "cpu");
                    }
                    BenchMode::Gpu => {
                        obj.insert("compute".to_string(), Value::String("GpuRequired".to_string()));
                        set_const_input_mode(node, "gpu");
                    }
                    BenchMode::Auto => {}
                }
            }
        }
        _ => {}
    }
}

fn walk_and_patch_nodes(value: &mut Value, mode: BenchMode) {
    match value {
        Value::Object(_) => {
            apply_node_mode_override(value, mode);
            if let Value::Object(map) = value {
                for child in map.values_mut() {
                    walk_and_patch_nodes(child, mode);
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                walk_and_patch_nodes(child, mode);
            }
        }
        _ => {}
    }
}

fn apply_mode_overrides(graph: &mut Value, mode: BenchMode) {
    match mode {
        BenchMode::Cpu => {
            upsert_metadata_string(graph, "helios.daedalus.gpu_backend", "cpu");
            upsert_metadata_string(graph, "helios.daedalus.planner.enable_gpu", "false");
        }
        BenchMode::Gpu => {
            upsert_metadata_string(graph, "helios.daedalus.gpu_backend", "gpu");
            upsert_metadata_string(graph, "helios.daedalus.planner.enable_gpu", "true");
        }
        BenchMode::Auto => {}
    }

    walk_and_patch_nodes(graph, mode);
}

fn collect_image_paths() -> Result<Vec<PathBuf>, String> {
    if let Ok(list) = env::var("HELIOS_BENCH_IMAGES") {
        let paths: Vec<PathBuf> = list.split(',').map(str::trim).filter(|s| !s.is_empty()).map(PathBuf::from).collect();
        if !paths.is_empty() {
            return Ok(paths);
        }
    }

    let prefix = env::var("HELIOS_BENCH_IMAGE_PREFIX").unwrap_or_else(|_| "/var/lib/helios/api-data/media/calibration_e1f0d4b5-40dc-4649-ae76-666f2128e9cf_".to_string());
    let suffix = env::var("HELIOS_BENCH_IMAGE_SUFFIX").unwrap_or_else(|_| ".jpg".to_string());
    let root = Path::new("/var/lib/helios/api-data/media");
    let entries = fs::read_dir(root).map_err(|e| format!("read_dir {root:?}: {e}"))?;
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("read_dir entry error: {e}"))?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
            continue;
        };
        if name.starts_with(prefix.rsplit('/').next().unwrap_or(prefix.as_str())) && name.ends_with(&suffix) {
            out.push(path);
        }
    }
    out.sort();
    if out.is_empty() {
        return Err("no input images found; set HELIOS_BENCH_IMAGES (comma-separated full paths)".to_string());
    }
    Ok(out)
}

fn load_images(paths: &[PathBuf]) -> Result<Vec<DynamicImage>, String> {
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        let img = image::open(path).map_err(|e| format!("failed to open image {path:?}: {e}"))?;
        out.push(img);
    }
    if out.is_empty() {
        return Err("no decodable images".to_string());
    }
    Ok(out)
}

fn top_nodes(metrics: &PipelineGraphMetrics, limit: usize) -> Vec<TopNode> {
    let mut nodes: Vec<TopNode> = metrics
        .nodes
        .iter()
        .map(|(name, item): (&String, &PipelineNodeRuntimeMetrics)| TopNode {
            node: name.clone(),
            avg_ms: item.metrics.average_time_ms,
            fps: item.metrics.average_fps,
            samples: item.metrics.sample_count,
            last_error: item.last_error.clone(),
        })
        .collect();
    nodes.sort_by(|a, b| b.avg_ms.partial_cmp(&a.avg_ms).unwrap_or(Ordering::Equal));
    nodes.truncate(limit);
    nodes
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let graph_path = env::var("HELIOS_BENCH_GRAPH").unwrap_or_else(|_| "/usr/share/helios/pipeline-templates/daedalus_aruco.json".to_string());
    let output_port = env::var("HELIOS_BENCH_OUTPUT").unwrap_or_else(|_| "frame".to_string());
    let warmup = parse_u64_env("HELIOS_BENCH_WARMUP_FRAMES", 90);
    let sample = parse_u64_env("HELIOS_BENCH_SAMPLE_FRAMES", 450);
    let top_n = parse_usize_env("HELIOS_BENCH_TOP_NODES", 12);
    let mode = BenchMode::from_env();

    let mut graph_json = load_graph_json(Path::new(&graph_path))?;
    apply_mode_overrides(&mut graph_json, mode);

    let image_paths = collect_image_paths()?;
    let images = load_images(&image_paths)?;

    let host_buffer = parse_usize_env("HELIOS_BENCH_HOST_BUFFER", 8).max(1);
    let graph = GraphHandle::from_json_with_output(host_buffer, &graph_json, Some(output_port.as_str())).map_err(|e| format!("graph build failed: {e}"))?;
    graph.set_perf_enabled(None, true);

    for i in 0..warmup {
        let img = images[(i as usize) % images.len()].clone();
        let _ = graph.process(img);
    }

    let mut frame_ms = Vec::with_capacity(sample as usize);
    for i in 0..sample {
        let img = images[(i as usize) % images.len()].clone();
        let t0 = Instant::now();
        let _ = graph.process(img);
        frame_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    let avg_frame_ms = if frame_ms.is_empty() { 0.0 } else { frame_ms.iter().sum::<f64>() / frame_ms.len() as f64 };
    let fps = if avg_frame_ms > 0.0 { 1000.0 / avg_frame_ms } else { 0.0 };

    let pipeline_metrics = graph.pipeline_metrics();
    let top = pipeline_metrics.as_ref().map(|m| top_nodes(m, top_n)).unwrap_or_default();

    let summary = json!({
        "mode": mode.as_str(),
        "graph_path": graph_path,
        "output_port": output_port,
        "warmup_frames": warmup,
        "sample_frames": sample,
        "images_loaded": images.len(),
        "avg_frame_ms": avg_frame_ms,
        "fps": fps,
        "top_nodes": top.iter().map(|n| json!({
            "node": n.node,
            "avg_ms": n.avg_ms,
            "fps": n.fps,
            "samples": n.samples,
            "last_error": n.last_error
        })).collect::<Vec<_>>(),
        "has_pipeline_metrics": pipeline_metrics.is_some(),
    });

    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
