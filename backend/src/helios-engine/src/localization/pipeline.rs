use image::{DynamicImage, GrayImage};
use serde::Serialize;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;
use std::time::Instant;
use tokio::sync::Mutex;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::graph::GraphHandle;

use super::config::{select_profile, LocalizationConfig, LocalizationProfile, LocalizationSourceConfig};
use super::sources::{fetch_source_value, LocalizationSourceFetcher};
use super::types::PipelineOutputSample;

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationPipelineStatus {
    pub configured: bool,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub template_id: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_run_ms: Option<f64>,
}

#[derive(Default)]
struct LocalizationPipelineRuntime {
    profile_id: Option<String>,
    template_id: Option<String>,
    graph: Option<GraphHandle>,
    last_error: Option<String>,
    last_run_ms: Option<f64>,
    graph_updated_at_ms: Option<i64>,
    template_mtime_ms: Option<i64>,
}

pub struct PipelineGraphDocument {
    pub graph: serde_json::Value,
    pub updated_at_ms: i64,
}

pub trait LocalizationPipelineGraphProvider: Send + Sync {
    fn load_graph_document<'a>(&'a self, id: Uuid) -> Pin<Box<dyn Future<Output = Result<PipelineGraphDocument, String>> + Send + 'a>>;
    fn load_template_graph<'a>(&'a self, template_id: &'a str) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, String>> + Send + 'a>>;
    fn template_last_modified_ms<'a>(&'a self, template_id: &'a str) -> Pin<Box<dyn Future<Output = Result<Option<i64>, String>> + Send + 'a>>;
}

static LOCALIZATION_PIPELINE: OnceLock<Mutex<LocalizationPipelineRuntime>> = OnceLock::new();

fn pipeline_runtime() -> &'static Mutex<LocalizationPipelineRuntime> {
    LOCALIZATION_PIPELINE.get_or_init(|| Mutex::new(LocalizationPipelineRuntime::default()))
}

pub async fn status(config: &LocalizationConfig, profile_id: Option<&str>) -> Result<LocalizationPipelineStatus, String> {
    let profile = select_profile(config, profile_id)?;
    let template_id = profile.pipeline_template_id.clone();
    let runtime = pipeline_runtime().lock().await;
    Ok(LocalizationPipelineStatus {
        configured: template_id.as_ref().is_some_and(|id| !id.trim().is_empty()),
        profile_id: Some(profile.id.clone()),
        template_id,
        last_error: runtime.last_error.clone(),
        last_run_ms: runtime.last_run_ms,
    })
}

pub async fn list_outputs<P: LocalizationPipelineGraphProvider>(provider: &P, profile: &LocalizationProfile, template_id: &str) -> Result<Vec<String>, String> {
    let mut runtime = pipeline_runtime().lock().await;
    ensure_graph(provider, &mut runtime, profile, template_id).await?;
    Ok(runtime.graph.as_ref().and_then(|graph| graph.host_output_ports()).unwrap_or_default())
}

pub async fn sample_output<P: LocalizationPipelineGraphProvider, F: LocalizationSourceFetcher>(
    provider: &P,
    fetcher: &F,
    profile: &LocalizationProfile,
    template_id: &str,
    output_key: &str,
) -> Result<PipelineOutputSample, String> {
    let mut runtime = pipeline_runtime().lock().await;
    ensure_graph(provider, &mut runtime, profile, template_id).await?;

    let inputs = collect_inputs(fetcher, &profile.sources).await?;

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

async fn ensure_graph<P: LocalizationPipelineGraphProvider>(provider: &P, runtime: &mut LocalizationPipelineRuntime, profile: &LocalizationProfile, template_id: &str) -> Result<(), String> {
    let mut needs_reload = runtime.graph.is_none() || runtime.profile_id.as_deref() != Some(profile.id.as_str()) || runtime.template_id.as_deref() != Some(template_id);

    let mut graph_json: Option<serde_json::Value> = None;
    let mut next_updated_at_ms: Option<i64> = None;
    let mut next_mtime_ms: Option<i64> = None;

    if let Ok(uuid) = template_id.parse::<Uuid>() {
        let doc = provider.load_graph_document(uuid).await?;
        next_updated_at_ms = Some(doc.updated_at_ms);
        if runtime.graph_updated_at_ms != next_updated_at_ms {
            needs_reload = true;
        }
        if needs_reload {
            graph_json = Some(doc.graph);
        }
    } else {
        let mtime_ms = provider.template_last_modified_ms(template_id).await?;
        next_mtime_ms = mtime_ms;
        if runtime.template_mtime_ms != next_mtime_ms {
            needs_reload = true;
        }
        if needs_reload {
            graph_json = Some(provider.load_template_graph(template_id).await?);
        }
    }

    if !needs_reload {
        return Ok(());
    }

    let graph_json = graph_json.ok_or_else(|| format!("pipeline template not found: {template_id}"))?;

    let graph = GraphHandle::from_json(2, &graph_json).map_err(|err| format!("failed to build localization graph: {err}"))?;
    runtime.graph = Some(graph);
    runtime.profile_id = Some(profile.id.clone());
    runtime.template_id = Some(template_id.to_string());
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
