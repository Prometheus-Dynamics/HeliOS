use super::*;

#[utoipa::path(
    get,
    path = "/ota/state",
    tag = "OTA",
    responses(
        (status = 200, description = "Current updater state", body = UpdateStateResponse),
        (status = 503, description = "Updater unavailable", body = UploadUpdateError)
    )
)]
pub async fn updater_state(State(state): State<AppState>) -> impl IntoResponse {
    let upload_dir = match ota_storage::ensure_upload_dir() {
        Ok(dir) => dir,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(UploadUpdateError { error: err.to_string() })).into_response(),
    };
    let upload_stats = match ota_storage::upload_storage_stats_for_dir(&upload_dir).await {
        Ok(stats) => stats,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(UploadUpdateError { error: format!("failed to inspect OTA upload storage: {err}") })).into_response(),
    };

    match (fetch_updater_state(&state).await, fetch_updater_storage(&state).await) {
        (Ok((state, cache_usage_bytes)), Ok(storage)) => {
            let state = state.and_then(|s| serde_json::to_value(&s).ok());
            let storage = OtaStorageReport {
                uploads: OtaUploadStorageReport {
                    path: upload_stats.root.display().to_string(),
                    usage_bytes: upload_stats.usage_bytes,
                    quota_bytes: upload_stats.quota_bytes,
                    available_bytes: upload_stats.available_bytes,
                    remaining_bytes: upload_stats.remaining_bytes,
                },
                cache: ota_directory_storage_report(storage.cache),
                work: ota_directory_storage_report(storage.work),
                service_releases: ota_directory_storage_report(storage.service_releases),
                frontend_releases: ota_directory_storage_report(storage.frontend_releases),
            };
            (StatusCode::OK, Json(UpdateStateResponse { state, cache_usage_bytes, storage })).into_response()
        }
        (_, Err(err)) => err.into_response(),
        (Err(err), _) => err.into_response(),
    }
}

pub(super) async fn fetch_updater_state(state: &AppState) -> Result<(Option<UpdateState>, u64), UploadUpdateError> {
    state.services.updater.fetch_updater_state(state).await.map_err(|error| UploadUpdateError { error })
}

async fn fetch_updater_storage(state: &AppState) -> Result<UpdaterStorageReport, UploadUpdateError> {
    state.services.updater.fetch_updater_storage(state).await.map_err(|error| UploadUpdateError { error })
}

fn ota_directory_storage_report(report: helios_updater::ipc::StorageDirectoryReport) -> OtaDirectoryStorageReport {
    OtaDirectoryStorageReport { path: report.path, usage_bytes: report.usage_bytes, available_bytes: report.available_bytes }
}
