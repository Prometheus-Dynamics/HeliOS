use std::path::PathBuf;

const CALIBRATION_TEMPLATE_ID: &str = "daedalus_aruco";

fn main() {
    let path: PathBuf = std::env::args_os().nth(1).map(PathBuf::from).expect("usage: calib_eval <image-path>");

    let img: image::DynamicImage = image::open(&path).expect("open image");

    let graph_json = helios_engine::pipelines::load_template_graph_json(CALIBRATION_TEMPLATE_ID).expect("load calibration template graph");

    let graph = helios_engine::graph::GraphHandle::from_json_with_output(2, &graph_json, Some("frame")).expect("build graph");

    let _ = graph.process(img);
    let Some(raw) = graph.sample_json_output("detections") else {
        println!("no output");
        return;
    };

    let parsed: serde_json::Value = match raw {
        serde_json::Value::String(s) => serde_json::from_str(&s).unwrap_or_else(|_| serde_json::json!({ "raw": s })),
        other => other,
    };

    let dets = parsed.as_array().or_else(|| parsed.get("detections").and_then(|v| v.as_array())).into_iter().flatten();
    let ids: Vec<u32> = dets.filter_map(|det| det.get("id").and_then(|v| v.as_u64()).map(|v| v as u32)).collect();

    println!("detections={} unique_ids={}", ids.len(), ids.iter().copied().collect::<std::collections::BTreeSet<_>>().len());
    println!("{}", serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| "{}".into()));
}
