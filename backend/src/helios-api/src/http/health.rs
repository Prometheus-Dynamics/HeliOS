use axum::Json;
use chrono::Utc;
use once_cell::sync::Lazy;
use serde::Serialize;
use std::time::Instant;
use utoipa::ToSchema;

static STARTED_AT: Lazy<Instant> = Lazy::new(Instant::now);

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct FeaturesPayload {
    pub shadow_recorder: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct HealthPayload {
    pub ok: bool,
    pub server_time_ms: i64,
    pub uptime_ms: u64,
    pub version: String,
    pub features: FeaturesPayload,
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
    Json(HealthPayload {
        ok: true,
        server_time_ms: Utc::now().timestamp_millis(),
        uptime_ms: STARTED_AT.elapsed().as_millis() as u64,
        version: env!("CARGO_PKG_VERSION").to_string(),
        features: FeaturesPayload { shadow_recorder: crate::features::shadow_recorder_enabled() },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_payload_smoke() {
        let Json(payload) = health().await;
        assert!(payload.ok);
        assert!(payload.server_time_ms > 0);
        assert!(!payload.version.is_empty());
    }
}
