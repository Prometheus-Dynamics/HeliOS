use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use helios_engine::ipc::StreamRuntimeCapabilities;
use helios_engine::ipc::cached_stream_runtime_capabilities;
use once_cell::sync::Lazy;
use serde::Serialize;
use std::time::Duration;
use std::time::Instant;
use utoipa::ToSchema;

static STARTED_AT: Lazy<Instant> = Lazy::new(Instant::now);

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct FeaturesPayload {
    pub shadow_recorder: bool,
    pub pipeline_registry_startup_warm: bool,
    pub pipeline_registry_prefetch: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct BinaryDependencyPayload {
    pub ok: bool,
    pub path: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct DependenciesPayload {
    pub api_tools_helper: BinaryDependencyPayload,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct HealthPayload {
    pub ok: bool,
    pub server_time_ms: i64,
    pub uptime_ms: u64,
    pub version: String,
    pub features: FeaturesPayload,
    pub dependencies: DependenciesPayload,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStreamsPayload {
    pub capabilities: crate::http::streams::validation::StreamCapabilitiesResponse,
    pub codecs: Vec<crate::http::streams::types::CodecInfo>,
    pub resolved_streams: Vec<crate::http::streams::types::StreamInfo>,
    pub stale: bool,
    pub revision: u64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RootStatusPayload {
    pub health: HealthPayload,
    pub streams: RuntimeStreamsPayload,
}

pub(crate) fn build_health_payload() -> HealthPayload {
    let helper = crate::api_tools_client::helper_status();
    HealthPayload {
        ok: helper.ok,
        server_time_ms: Utc::now().timestamp_millis(),
        uptime_ms: STARTED_AT.elapsed().as_millis() as u64,
        version: env!("CARGO_PKG_VERSION").to_string(),
        features: FeaturesPayload {
            shadow_recorder: crate::features::shadow_recorder_enabled(),
            pipeline_registry_startup_warm: crate::features::warm_pipeline_registry_enabled(),
            pipeline_registry_prefetch: crate::features::prefetch_pipeline_registry_enabled(),
        },
        dependencies: DependenciesPayload { api_tools_helper: BinaryDependencyPayload { ok: helper.ok, path: helper.path.display().to_string() } },
    }
}

#[utoipa::path(
    get,
    path = "/health",
    tag = "Device",
    responses(
        (status = 200, description = "Backend is reachable", body = HealthPayload),
    )
)]
pub async fn health() -> Json<HealthPayload> {
    Json(build_health_payload())
}

#[utoipa::path(
    get,
    path = "/",
    tag = "Device",
    responses(
        (status = 200, description = "Runtime status, capabilities, codec inventory, and resolved streams", body = RootStatusPayload),
    )
)]
pub async fn root_status(State(state): State<crate::http::AppState>) -> Response {
    let (resolved_streams, stale, revision) = state.services.streams.get_cached_streams_snapshot_with_revision(&state).await;
    let runtime = match state.engine.get_stream_runtime_capabilities_with_timeout(Duration::from_secs(2)).await {
        Ok(runtime) => runtime,
        Err(err) => match cached_stream_runtime_capabilities() {
            Ok(runtime) => {
                tracing::warn!(error = %err, "root status using local runtime capability fallback");
                runtime
            }
            Err(local_err) => {
                tracing::warn!(error = %err, fallback_error = %local_err, "root status using empty runtime capability fallback");
                StreamRuntimeCapabilities { codecs: Vec::new(), default_encoder_id: None, default_decoder_ids_by_capture_format: Default::default() }
            }
        },
    };
    let capabilities = crate::http::streams::validation::stream_capabilities(&runtime);
    let codecs = crate::http::streams::lifecycle::codec_inventory_from_runtime(runtime);

    Json(RootStatusPayload { health: build_health_payload(), streams: RuntimeStreamsPayload { capabilities, codecs, resolved_streams, stale, revision } }).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_payload_smoke() {
        let Json(payload) = health().await;
        assert!(payload.server_time_ms > 0);
        assert!(!payload.version.is_empty());
        assert!(!payload.dependencies.api_tools_helper.path.is_empty());
    }

    #[test]
    fn build_health_payload_populates_dependency_path() {
        let payload = build_health_payload();
        assert!(payload.server_time_ms > 0);
        assert!(payload.uptime_ms <= STARTED_AT.elapsed().as_millis() as u64);
        assert!(!payload.version.is_empty());
        assert!(!payload.dependencies.api_tools_helper.path.is_empty());
    }
}
