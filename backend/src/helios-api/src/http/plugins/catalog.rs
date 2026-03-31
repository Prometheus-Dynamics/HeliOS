use axum::{Json, extract::State, response::IntoResponse};
use std::{collections::BTreeMap, time::Duration};

use crate::http::{AppState, error::ApiResult};

use super::support::{DISABLED_SUFFIX, PLUGIN_SUFFIX, build_compatibility_map, install_dir, list_plugins_with_suffix, plugin_diag_from_path, plugin_dirs, resolve_plugin_path, upload_dir};
use super::types::{PluginFile, PluginListResponse};

#[utoipa::path(
    get,
    path = "/plugins",
    tag = "Plugins",
    responses((status = 200, description = "List plugins", body = PluginListResponse))
)]
pub(crate) async fn list_plugins(State(state): State<AppState>) -> ApiResult<impl IntoResponse> {
    let mut dirs = plugin_dirs();
    let install = install_dir();
    dirs.retain(|dir| dir != &install);
    dirs.insert(0, install);

    let (installed, disabled) = collect_plugin_inventory(&dirs).await?;
    let uploads = list_plugins_with_suffix(&upload_dir(), PLUGIN_SUFFIX).await?;

    const IPC_TIMEOUT: Duration = Duration::from_secs(3);
    let engine_available = state.engine.list_streams_with_timeout(IPC_TIMEOUT).await.is_ok();
    let compatibility = match state.services.pipelines.load_registry_snapshot_from_disk_or_helper().await {
        Ok(snapshot) => snapshot.plugin_compatibility,
        Err(_) => Vec::new(),
    };

    let compatibility = enrich_compatibility(&dirs, installed.as_slice(), disabled.as_slice(), compatibility).await;

    Ok(Json(PluginListResponse { installed, disabled, uploads, engine_available, compatibility }))
}

async fn collect_plugin_inventory(dirs: &[std::path::PathBuf]) -> ApiResult<(Vec<PluginFile>, Vec<PluginFile>)> {
    let mut installed_by_name = BTreeMap::<String, PluginFile>::new();
    let mut disabled_by_name = BTreeMap::<String, PluginFile>::new();
    for dir in dirs {
        for file in list_plugins_with_suffix(dir, PLUGIN_SUFFIX).await? {
            installed_by_name.entry(file.name.clone()).or_insert(file);
        }
        for file in list_plugins_with_suffix(dir, DISABLED_SUFFIX).await? {
            disabled_by_name.entry(file.name.clone()).or_insert(file);
        }
    }

    let mut disabled = Vec::new();
    for (name, file) in disabled_by_name {
        if installed_by_name.contains_key(&name) || file.size_bytes > 0 {
            disabled.push(file);
        }
    }

    let mut installed = Vec::new();
    for (name, file) in installed_by_name {
        if disabled.iter().any(|disabled_file| disabled_file.name == name) {
            continue;
        }
        installed.push(file);
    }

    Ok((installed, disabled))
}

async fn enrich_compatibility(
    dirs: &[std::path::PathBuf],
    installed: &[PluginFile],
    disabled: &[PluginFile],
    compatibility: Vec<helios_engine::ipc::PluginCompatibility>,
) -> Vec<helios_engine::ipc::PluginCompatibility> {
    let mut compatibility_map = build_compatibility_map(compatibility);
    fill_missing_compatibility(dirs, installed, false, &mut compatibility_map).await;
    fill_missing_compatibility(dirs, disabled, true, &mut compatibility_map).await;
    compatibility_map.into_values().collect()
}

async fn fill_missing_compatibility(dirs: &[std::path::PathBuf], plugins: &[PluginFile], disabled: bool, compatibility_map: &mut BTreeMap<String, helios_engine::ipc::PluginCompatibility>) {
    for plugin in plugins {
        if compatibility_map.contains_key(&plugin.name) {
            continue;
        }
        if let Some(path) = resolve_plugin_path(dirs, &plugin.name, disabled).await {
            let diag = plugin_diag_from_path(&plugin.name, &path, disabled);
            compatibility_map.insert(plugin.name.clone(), diag);
        }
    }
}
