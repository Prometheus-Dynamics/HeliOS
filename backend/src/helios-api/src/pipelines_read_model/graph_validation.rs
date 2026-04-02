use std::collections::HashSet;
use std::process::Stdio;
use std::sync::Arc;

use axum::response::Response;
use helios_engine::ipc::{EngineErrorCode, EngineEvent, GraphValidationHelperRequest, GraphValidationHelperResponse, GraphValidationReport, JsonWire};
use serde_json::Value as JsonValue;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::Duration;
use tracing::warn;
use uuid::Uuid;

use crate::http::AppState;
use crate::http::pipelines::{PipelineDocument, PipelineSummary, PlannerDiagnostic, PlannerDiagnosticSpan, map_io_error, pipeline_dir};

use super::{GRAPH_VALIDATION_CACHE_MAX_AGE_MS, GraphListCacheEntry, GraphValidationRequestError, GraphValidationState, PipelinesReadModelState, now_timestamp_ms};

const GRAPH_VALIDATION_HELPER_TIMEOUT: Duration = Duration::from_secs(20);

fn graph_validation_state_stale(state: &GraphValidationState, graph_updated_at_ms: i64, now_ms: i64) -> bool {
    if graph_updated_at_ms > 0 && state.updated_at_ms < graph_updated_at_ms {
        return true;
    }
    now_ms.saturating_sub(state.updated_at_ms) > GRAPH_VALIDATION_CACHE_MAX_AGE_MS
}

fn map_planner_diagnostics(diagnostics: Vec<helios_engine::ipc::PlannerDiagnostic>) -> Vec<PlannerDiagnostic> {
    diagnostics
        .into_iter()
        .map(|diag| PlannerDiagnostic { code: diag.code, message: diag.message, span: PlannerDiagnosticSpan { pass: diag.span.pass, node: diag.span.node, port: diag.span.port } })
        .collect()
}

impl PipelinesReadModelState {
    async fn snapshot_graph_validation(&self) -> std::collections::HashMap<Uuid, GraphValidationState> {
        self.graph_validation_cache.read().await.clone()
    }

    async fn prune_graph_validation_cache(&self, valid_ids: &HashSet<Uuid>) {
        let mut cache = self.graph_validation_cache.write().await;
        cache.retain(|id, _| valid_ids.contains(id));
    }

    pub async fn set_graph_validation_state(&self, graph_id: Uuid, diagnostics: Vec<PlannerDiagnostic>) {
        let mut cache = self.graph_validation_cache.write().await;
        cache.insert(graph_id, GraphValidationState { diagnostics, updated_at_ms: now_timestamp_ms() });
    }

    pub async fn set_graph_validation_error(&self, graph_id: Uuid, code: Option<EngineErrorCode>, message: String) {
        let fallback = code.map(|value| format!("{value:?}")).unwrap_or_else(|| "validation_error".to_string());
        let diag = PlannerDiagnostic { code: fallback, message, span: PlannerDiagnosticSpan { pass: "engine".to_string(), node: None, port: None } };
        self.set_graph_validation_state(graph_id, vec![diag]).await;
    }

    pub async fn clear_graph_validation_state(&self, graph_id: Uuid) {
        let mut cache = self.graph_validation_cache.write().await;
        cache.remove(&graph_id);
    }

    pub async fn invalidate_graph_list_cache(&self) {
        *self.graph_list_cache.write().await = None;
    }

    pub async fn get_cached_graph_summaries_snapshot(&self, state: &AppState) -> Result<(Arc<Vec<PipelineSummary>>, u64), Box<Response>> {
        const FRESH_FOR: Duration = Duration::from_secs(2);

        if let Some(entry) = self.graph_list_cache.read().await.clone()
            && entry.fetched_at.elapsed() < FRESH_FOR
        {
            self.graph_list_stats.record_hit();
            return Ok((entry.payload, entry.revision));
        }

        self.graph_list_stats.record_miss();
        let _refresh_guard = self.graph_list_refresh_lock.lock().await;
        if let Some(entry) = self.graph_list_cache.read().await.clone()
            && entry.fetched_at.elapsed() < FRESH_FOR
        {
            self.graph_list_stats.record_hit();
            return Ok((entry.payload, entry.revision));
        }

        let stale = self.graph_list_cache.read().await.clone();
        match self.load_graph_summaries(state).await {
            Ok(summaries) => {
                let payload = Arc::new(summaries);
                let revision = self.graph_list_stats.record_refresh();
                *self.graph_list_cache.write().await = Some(GraphListCacheEntry { fetched_at: tokio::time::Instant::now(), revision, payload: payload.clone() });
                Ok((payload, revision))
            }
            Err(resp) => match stale {
                Some(entry) => {
                    self.graph_list_stats.record_stale_fallback();
                    Ok((entry.payload, entry.revision))
                }
                None => Err(resp),
            },
        }
    }

    async fn load_graph_summaries(&self, state: &AppState) -> Result<Vec<PipelineSummary>, Box<Response>> {
        let dir = pipeline_dir()?;
        let mut loaded_docs: Vec<(PipelineDocument, i64)> = Vec::new();
        let mut existing_ids = HashSet::new();
        let validation_snapshot = self.snapshot_graph_validation().await;
        let mut entries = match fs::read_dir(dir).await {
            Ok(entries) => entries,
            Err(err) => return Err(Box::new(map_io_error(err, "failed to read pipeline directory"))),
        };

        loop {
            let entry = match entries.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(err) => return Err(Box::new(map_io_error(err, "failed to read pipeline entry"))),
            };

            let meta = match entry.metadata().await {
                Ok(meta) if meta.is_file() => meta,
                Ok(_) => continue,
                Err(err) => return Err(Box::new(map_io_error(err, "failed to stat pipeline file"))),
            };

            let data = match fs::read(entry.path()).await {
                Ok(data) => data,
                Err(err) => return Err(Box::new(map_io_error(err, "failed to read pipeline file"))),
            };
            if let Ok(doc) = PipelineDocument::decode_slice(&data) {
                let doc_id = doc.id;
                let mut updated_at_ms = doc.updated_at_ms.max(0);
                if updated_at_ms == 0
                    && let Ok(modified) = meta.modified()
                    && let Ok(ts) = modified.duration_since(std::time::UNIX_EPOCH)
                {
                    updated_at_ms = ts.as_millis() as i64;
                }
                loaded_docs.push((doc, updated_at_ms));
                existing_ids.insert(doc_id);
            }
        }

        let now_ms = now_timestamp_ms();
        let mut refresh_docs = Vec::new();
        let summaries: Vec<PipelineSummary> = loaded_docs
            .into_iter()
            .map(|(doc, updated_at_ms)| {
                let needs_refresh = match validation_snapshot.get(&doc.id) {
                    Some(state) => graph_validation_state_stale(state, updated_at_ms, now_ms),
                    None => true,
                };
                if needs_refresh {
                    refresh_docs.push((doc.id, doc.graph.clone()));
                }
                let issue_count = validation_snapshot.get(&doc.id).map(|state| state.diagnostics.len()).unwrap_or(0);
                PipelineSummary { id: doc.id, name: doc.name, updated_at_ms, issue_count }
            })
            .collect();

        self.prune_graph_validation_cache(&existing_ids).await;
        if !refresh_docs.is_empty() {
            let state = state.clone();
            let pipelines = state.services.pipelines.clone();
            tokio::spawn(async move {
                for (graph_id, graph) in refresh_docs {
                    pipelines.refresh_graph_validation(&state, graph_id, &graph).await;
                }
                pipelines.invalidate_graph_list_cache().await;
            });
        }

        Ok(summaries)
    }

    pub async fn refresh_graph_validation(&self, state: &AppState, graph_id: Uuid, graph: &JsonValue) {
        match validate_graph_report(state, graph.clone(), Vec::new(), true).await {
            Ok(report) => {
                let diagnostics = map_planner_diagnostics(report.diagnostics);
                self.set_graph_validation_state(graph_id, diagnostics).await;
            }
            Err(GraphValidationRequestError::Rejected { code, reason }) => {
                self.set_graph_validation_error(graph_id, Some(code), reason).await;
            }
            Err(GraphValidationRequestError::Transport(err)) => {
                warn!(error = %err, graph_id = %graph_id, "graph validation failed");
                self.set_graph_validation_error(graph_id, Some(EngineErrorCode::Internal), format!("validation failed: {err}")).await;
            }
        }
    }
}

pub(crate) async fn validate_graph_report(state: &AppState, graph: JsonValue, active_features: Vec<String>, enable_lints: bool) -> Result<GraphValidationReport, GraphValidationRequestError> {
    match validate_graph_report_via_helper(graph.clone(), active_features.clone(), enable_lints).await {
        Ok(report) => return Ok(report),
        Err(GraphValidationRequestError::Rejected { code, reason }) => {
            return Err(GraphValidationRequestError::Rejected { code, reason });
        }
        Err(GraphValidationRequestError::Transport(err)) => {
            warn!(error = %err, "graph validation helper failed; falling back to live engine IPC");
        }
    }

    match state.engine.validate_graph_event(graph, active_features, enable_lints).await {
        Ok(EngineEvent::GraphValidation { report, .. }) => Ok(report),
        Ok(EngineEvent::Nack { code, reason, .. }) => Err(GraphValidationRequestError::Rejected { code, reason }),
        Ok(other) => {
            warn!(?other, "engine returned unexpected event for graph validation");
            Err(GraphValidationRequestError::Rejected { code: EngineErrorCode::Internal, reason: "unexpected engine response".to_string() })
        }
        Err(err) => Err(GraphValidationRequestError::Transport(err)),
    }
}

async fn validate_graph_report_via_helper(graph: JsonValue, active_features: Vec<String>, enable_lints: bool) -> Result<GraphValidationReport, GraphValidationRequestError> {
    let engine_bin = super::registry::registry_generator_binary();
    let request = GraphValidationHelperRequest { graph: JsonWire(graph), active_features, enable_lints };
    let payload = serde_json::to_vec(&request).map_err(|err| GraphValidationRequestError::Transport(helper_io_error(format!("encode graph validation request failed: {err}"))))?;

    let mut child = Command::new(&engine_bin)
        .arg("validate-graph")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| GraphValidationRequestError::Transport(helper_io_error(format!("spawn graph validation helper failed ({}): {err}", engine_bin.display()))))?;

    let Some(mut stdin) = child.stdin.take() else {
        return Err(GraphValidationRequestError::Transport(helper_io_error(format!("graph validation helper missing stdin pipe ({})", engine_bin.display()))));
    };
    stdin.write_all(&payload).await.map_err(|err| GraphValidationRequestError::Transport(helper_io_error(format!("write graph validation helper stdin failed ({}): {err}", engine_bin.display()))))?;
    drop(stdin);

    let output = tokio::time::timeout(GRAPH_VALIDATION_HELPER_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| {
            GraphValidationRequestError::Transport(helper_io_error(format!("graph validation helper timed out after {}s ({})", GRAPH_VALIDATION_HELPER_TIMEOUT.as_secs(), engine_bin.display())))
        })?
        .map_err(|err| GraphValidationRequestError::Transport(helper_io_error(format!("wait graph validation helper failed ({}): {err}", engine_bin.display()))))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("exit status {}", output.status)
        };
        return Err(GraphValidationRequestError::Transport(helper_io_error(format!("graph validation helper failed ({}): {detail}", engine_bin.display()))));
    }

    let response: GraphValidationHelperResponse = serde_json::from_slice(&output.stdout)
        .map_err(|err| GraphValidationRequestError::Transport(helper_io_error(format!("decode graph validation helper response failed ({}): {err}", engine_bin.display()))))?;

    match response {
        GraphValidationHelperResponse::Report { report } => Ok(report),
        GraphValidationHelperResponse::Error { code, reason } => Err(GraphValidationRequestError::Rejected { code, reason }),
    }
}

fn helper_io_error(message: String) -> lib_ipc::client::ClientTransportError {
    lib_ipc::client::ClientTransportError::Io(std::io::Error::other(message))
}
