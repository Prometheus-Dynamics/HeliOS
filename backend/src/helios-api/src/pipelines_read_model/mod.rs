mod graph_validation;
mod registry;

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use helios_engine::ipc::EngineErrorCode;
use tokio::sync::RwLock;
use tokio::time::Instant;
use uuid::Uuid;

use crate::api_observability::{ApiCacheMetric, CacheMetricCounters};
use crate::http::pipelines::{PipelineSummary, PlannerDiagnostic, RegistryPortMetadataLookup};

pub(crate) use graph_validation::validate_graph_report;
pub(crate) use registry::load_registry_snapshot_from_disk_or_helper;

#[derive(Clone, Debug)]
struct GraphValidationState {
    diagnostics: Vec<PlannerDiagnostic>,
    updated_at_ms: i64,
}

#[derive(Clone)]
struct GraphListCacheEntry {
    fetched_at: Instant,
    revision: u64,
    payload: Arc<Vec<PipelineSummary>>,
}

#[derive(Clone)]
struct CachedRegistryPayload {
    port_metadata_lookup: Arc<RegistryPortMetadataLookup>,
    body: Bytes,
}

#[derive(Clone)]
struct RegistryCacheEntry {
    fetched_at: Instant,
    revision: u64,
    payload: Arc<CachedRegistryPayload>,
}

const GRAPH_VALIDATION_CACHE_MAX_AGE_MS: i64 = 60_000;

#[derive(Debug)]
pub(crate) enum GraphValidationRequestError {
    Rejected { code: EngineErrorCode, reason: String },
    Transport(lib_ipc::client::ClientTransportError),
}

#[derive(Default)]
pub struct PipelinesReadModelState {
    graph_validation_cache: RwLock<HashMap<Uuid, GraphValidationState>>,
    graph_list_cache: RwLock<Option<GraphListCacheEntry>>,
    graph_list_refresh_lock: tokio::sync::Mutex<()>,
    graph_list_stats: CacheMetricCounters,
    registry_cache: Arc<RwLock<Option<RegistryCacheEntry>>>,
    registry_refresh_lock: tokio::sync::Mutex<()>,
    registry_stats: CacheMetricCounters,
}

fn now_timestamp_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl PipelinesReadModelState {
    pub fn graph_list_cache_metrics(&self) -> ApiCacheMetric {
        self.graph_list_stats.snapshot()
    }

    pub fn registry_cache_metrics(&self) -> ApiCacheMetric {
        self.registry_stats.snapshot()
    }
}
