use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::http::AppState;

pub(crate) async fn set_graph_validation_state(state: &AppState, graph_id: Uuid, diagnostics: Vec<super::types::PlannerDiagnostic>) {
    state.services.pipelines.set_graph_validation_state(graph_id, diagnostics).await;
}

pub(crate) async fn set_graph_validation_error(state: &AppState, graph_id: Uuid, code: Option<helios_engine::ipc::EngineErrorCode>, message: String) {
    state.services.pipelines.set_graph_validation_error(graph_id, code, message).await;
}

pub(crate) async fn clear_graph_validation_state(state: &AppState, graph_id: Uuid) {
    state.services.pipelines.clear_graph_validation_state(graph_id).await;
}

pub(crate) async fn invalidate_graph_list_cache(state: &AppState) {
    state.services.pipelines.invalidate_graph_list_cache().await;
}

pub(crate) async fn refresh_graph_validation(state: &AppState, graph_id: Uuid, graph: &JsonValue) {
    state.services.pipelines.refresh_graph_validation(state, graph_id, graph).await;
}

pub async fn warm_registry_cache(state: AppState) {
    state.services.pipelines.warm_registry_cache(state.clone()).await;
}

pub(super) async fn inject_cached_port_metadata(state: &AppState, graph: &mut JsonValue) {
    state.services.pipelines.inject_cached_port_metadata(state, graph).await;
}
