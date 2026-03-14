use image::{DynamicImage, GrayImage};
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Instant;
use tokio::sync::Mutex;

use crate::graph::GraphHandle;

use super::config::{LocalizationProfile, LocalizationSourceConfig};
use super::fetch::LocalizationSourceFetcher;
use super::sources::fetch_source_value;
use super::types::{LocalizationPipelineStatus, PipelineOutputSample};

#[derive(Debug, Clone)]
pub struct LoadedPipelineGraph {
    pub graph: serde_json::Value,
    pub graph_updated_at_ms: Option<i64>,
    pub template_mtime_ms: Option<i64>,
}

#[derive(Default)]
struct LocalizationPipelineRuntime {
    profile_id: Option<String>,
    graph: Option<GraphHandle>,
    last_error: Option<String>,
    last_run_ms: Option<f64>,
    graph_updated_at_ms: Option<i64>,
    template_mtime_ms: Option<i64>,
}

static LOCALIZATION_PIPELINE: OnceLock<Mutex<LocalizationPipelineRuntime>> = OnceLock::new();

fn pipeline_runtime() -> &'static Mutex<LocalizationPipelineRuntime> {
    LOCALIZATION_PIPELINE.get_or_init(|| Mutex::new(LocalizationPipelineRuntime::default()))
}

pub async fn status(profile_id: &str) -> LocalizationPipelineStatus {
    let runtime = pipeline_runtime().lock().await;
    let matches_runtime = runtime.profile_id.as_deref() == Some(profile_id);
    LocalizationPipelineStatus {
        configured: true,
        profile_id: Some(profile_id.to_string()),
        last_error: matches_runtime.then(|| runtime.last_error.clone()).flatten(),
        last_run_ms: matches_runtime.then_some(runtime.last_run_ms).flatten(),
    }
}

pub async fn list_outputs(profile: &LocalizationProfile, graph_doc: &LoadedPipelineGraph) -> Result<Vec<String>, String> {
    let mut runtime = pipeline_runtime().lock().await;
    ensure_graph(&mut runtime, profile, graph_doc).await?;
    Ok(runtime.graph.as_ref().and_then(|graph| graph.host_output_ports()).unwrap_or_default())
}

pub async fn sample_output<F: LocalizationSourceFetcher>(
    fetcher: &F,
    profile: &LocalizationProfile,
    sources: &[LocalizationSourceConfig],
    graph_doc: &LoadedPipelineGraph,
    output_key: &str,
) -> Result<PipelineOutputSample, String> {
    let mut runtime = pipeline_runtime().lock().await;
    ensure_graph(&mut runtime, profile, graph_doc).await?;

    let inputs = collect_inputs(fetcher, sources).await?;

    let run_start = Instant::now();
    if let Some(graph) = runtime.graph.as_ref() {
        graph.set_pipeline_inputs(None, &inputs);
        let _ = graph.process(dummy_image());
    }
    runtime.last_run_ms = Some(run_start.elapsed().as_secs_f64() * 1000.0);

    let Some(graph) = runtime.graph.as_ref() else {
        return Err("localization pipeline not initialized".to_string());
    };
    match graph.sample_json_output(output_key) {
        Some(value) => Ok(PipelineOutputSample { data_type: None, value }),
        None => Err("output sample not available".to_string()),
    }
}

async fn ensure_graph(runtime: &mut LocalizationPipelineRuntime, profile: &LocalizationProfile, graph_doc: &LoadedPipelineGraph) -> Result<(), String> {
    let mut needs_reload = runtime.graph.is_none() || runtime.profile_id.as_deref() != Some(profile.id.as_str());
    let next_updated_at_ms = graph_doc.graph_updated_at_ms;
    let next_mtime_ms = graph_doc.template_mtime_ms;
    if runtime.graph_updated_at_ms != next_updated_at_ms || runtime.template_mtime_ms != next_mtime_ms {
        needs_reload = true;
    }

    if !needs_reload {
        return Ok(());
    }

    let graph = GraphHandle::from_json(2, &graph_doc.graph).map_err(|err| format!("failed to build localization graph: {err}"))?;
    runtime.graph = Some(graph);
    runtime.profile_id = Some(profile.id.clone());
    runtime.last_error = None;
    runtime.last_run_ms = None;
    runtime.graph_updated_at_ms = next_updated_at_ms;
    runtime.template_mtime_ms = next_mtime_ms;
    Ok(())
}

async fn collect_inputs<F: LocalizationSourceFetcher>(fetcher: &F, sources: &[LocalizationSourceConfig]) -> Result<BTreeMap<String, Option<serde_json::Value>>, String> {
    let mut out = BTreeMap::new();
    let mut errors = Vec::new();
    for source in sources.iter().filter(|src| src.enabled) {
        let input_key = source.input_key.as_deref().unwrap_or(&source.output_key).trim().to_string();
        if input_key.is_empty() {
            continue;
        }
        match fetch_source_value(fetcher, source).await {
            Ok(value) => {
                out.insert(input_key, Some(value));
            }
            Err(err) => {
                errors.push(format!("{}: {}", source.id, err));
            }
        }
    }

    if out.is_empty() && !errors.is_empty() {
        return Err(errors.join("; "));
    }

    Ok(out)
}

fn dummy_image() -> DynamicImage {
    static DUMMY: OnceLock<DynamicImage> = OnceLock::new();
    DUMMY.get_or_init(|| DynamicImage::ImageLuma8(GrayImage::new(1, 1))).clone()
}
