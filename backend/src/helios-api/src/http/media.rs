mod archive;
mod assets;
mod attachments;
mod catalog;
mod models;
mod preview;
mod support;
#[cfg(test)]
mod tests;
mod transforms;
mod types;

pub(crate) use assets::{__path_delete_media, __path_fetch_media, __path_update_metadata, delete_media, fetch_media, update_metadata};
pub(crate) use attachments::{
    __path_attach_label, __path_attach_media_imu, __path_delete_media_imu, __path_fetch_label, __path_fetch_media_imu, attach_label, attach_media_imu, delete_media_imu, fetch_label, fetch_media_imu,
};
pub(crate) use catalog::{__path_list_media, __path_upload_media, list_media, upload_media};
pub(crate) use support::write_media_metadata;
pub(crate) use transforms::{__path_apply_image_edits, apply_image_edits};
pub use types::MediaItem;
pub(crate) use types::MediaMetadata;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, patch, post},
};

use self::{
    catalog::download_media_archive,
    transforms::{fetch_media_preview, fetch_media_thumbnail},
};
use super::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_media).post(upload_media))
        .route("/download.zip", get(download_media_archive))
        .route("/{name}/preview", get(fetch_media_preview))
        .route("/{name}/thumbnail", get(fetch_media_thumbnail))
        .route("/{name}/imu", get(fetch_media_imu).post(attach_media_imu).delete(delete_media_imu))
        .route("/{name}", get(fetch_media).delete(delete_media))
        .route("/{name}/metadata", patch(update_metadata))
        .route("/{name}/label", get(fetch_label).post(attach_label))
        .route("/{name}/image/edits", post(apply_image_edits))
        .route_layer(DefaultBodyLimit::disable())
}
