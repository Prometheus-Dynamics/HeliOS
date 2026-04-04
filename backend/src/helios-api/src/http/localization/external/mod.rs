mod registry;
mod sampling;
mod store;
#[cfg(test)]
mod tests;

pub(crate) use registry::{
    __path_delete_external_source, __path_list_external_sources, __path_update_external_sample, __path_upsert_external_source, delete_external_source, list_external_sources,
    list_external_sources_snapshot, update_external_sample, upsert_external_source,
};
pub(crate) use sampling::{__path_sample_external_output, fetch_external_value, sample_external_output};
pub(crate) use store::LocalizationExternalSourceRegistry;

use axum::{
    Router,
    routing::{get, post, put},
};

use super::super::AppState;

pub(super) use helios_engine::contracts::localization::DEVICE_IMU_EXTERNAL_SOURCE_ID as IMU_EXTERNAL_ID;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/external/sources", get(list_external_sources))
        .route("/external/sources/{id}", put(upsert_external_source).delete(delete_external_source))
        .route("/external/sources/{id}/sample", post(update_external_sample))
        .route("/external/{id}/outputs/{output_key}", get(sample_external_output))
}
