use std::path::{Path, PathBuf};

use daedalus::{
    FfiPluginError, HostBridgeInstallError, PluginLibrary, PluginRegistry,
    host_bridge::install_host_bridge,
    runtime::{HostBridgeManager, plugins::PluginError},
};

use crate::model::LoadedPlugin;
use crate::plugins::builtin::install_builtin_plugins;

#[derive(Debug, thiserror::Error)]
pub enum PluginLoadError {
    #[error("failed to install host bridge: {0}")]
    HostBridge(#[from] HostBridgeInstallError),
    #[error("failed to install built-in plugin: {0}")]
    Builtin(#[from] PluginError),
    #[error("failed to load Daedalus plugin library {path}: {source}")]
    Library { path: PathBuf, source: FfiPluginError },
}

pub struct LoadedPluginLibrary {
    metadata: LoadedPlugin,
    _library: PluginLibrary,
}

impl LoadedPluginLibrary {
    pub fn metadata(&self) -> &LoadedPlugin {
        &self.metadata
    }
}

pub struct PluginLoadResult {
    pub builtins: Vec<LoadedPlugin>,
    pub libraries: Vec<LoadedPluginLibrary>,
    pub registry: PluginRegistry,
    pub host_manager: HostBridgeManager,
}

pub fn load_plugins(paths: &[PathBuf]) -> Result<PluginLoadResult, PluginLoadError> {
    let mut registry = PluginRegistry::new();
    let host_manager = HostBridgeManager::new();
    install_host_bridge(&mut registry, host_manager.clone())?;
    let builtins = install_builtin_plugins(&mut registry)?;
    let libraries = paths.iter().map(|path| load_plugin_library(path, &mut registry)).collect::<Result<Vec<_>, _>>()?;

    Ok(PluginLoadResult { builtins, libraries, registry, host_manager })
}

fn load_plugin_library(path: &Path, registry: &mut PluginRegistry) -> Result<LoadedPluginLibrary, PluginLoadError> {
    #[allow(unsafe_code)]
    let library = unsafe { PluginLibrary::load(path) }.map_err(|source| PluginLoadError::Library { path: path.to_path_buf(), source })?;
    library.install_into(registry).map_err(|source| PluginLoadError::Library { path: path.to_path_buf(), source })?;

    let info = library.info();
    let metadata = LoadedPlugin {
        path: path.to_path_buf(),
        plugin_name: info.as_ref().and_then(|info| info.plugin_name.as_str().map(ToOwned::to_owned)),
        plugin_version: info.as_ref().and_then(|info| info.plugin_version.as_str().map(ToOwned::to_owned)),
        abi_version: library.abi_version(),
    };

    Ok(LoadedPluginLibrary { metadata, _library: library })
}
