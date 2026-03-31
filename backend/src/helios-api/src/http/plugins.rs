mod catalog;
mod file_ops;
mod support;
#[cfg(test)]
mod tests;
mod types;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, post},
};

use super::AppState;

pub use types::*;

pub(crate) use catalog::{__path_list_plugins, list_plugins};
pub(crate) use file_ops::{
    __path_delete_plugin, __path_disable_plugin, __path_download_disabled, __path_download_plugin, __path_download_upload, __path_enable_plugin, __path_install_plugin, __path_upload_plugin,
    delete_plugin, disable_plugin, download_disabled, download_plugin, download_upload, enable_plugin, install_plugin, upload_plugin,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_plugins))
        .route("/upload", post(upload_plugin))
        .route("/install", post(install_plugin))
        .route("/disabled/{name}", get(download_disabled))
        .route("/uploads/{name}", get(download_upload))
        .route("/{name}/disable", post(disable_plugin))
        .route("/{name}/enable", post(enable_plugin))
        .route("/{name}", get(download_plugin).delete(delete_plugin))
        .route_layer(DefaultBodyLimit::disable())
}
