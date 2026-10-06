use std::path::{Path, PathBuf};

use daedalus::{PluginLibrary, PluginLibraryError, PluginRegistry, dylib::InstallPath, runtime::plugins::PluginError};
use styx::core::daedalus::StyxFramesPlugin;

use crate::model::LoadedPlugin;
use crate::plugins::builtin::install_builtin_plugins;

#[derive(Debug, thiserror::Error)]
pub enum PluginLoadError {
    #[error("failed to install built-in plugin: {0}")]
    Builtin(#[from] PluginError),
    #[error("failed to load Daedalus plugin library {path}: {source}")]
    Library { path: PathBuf, source: Box<PluginLibraryError> },
}

pub struct LoadedPluginLibrary {
    metadata: LoadedPlugin,
    install_path: InstallPath,
    _library: PluginLibrary,
}

impl LoadedPluginLibrary {
    pub fn metadata(&self) -> &LoadedPlugin {
        &self.metadata
    }

    /// How the plugin was installed: `RustAbi` for plugins from the engine's own cargo build.
    pub fn install_path(&self) -> InstallPath {
        self.install_path
    }
}

pub struct PluginLoadResult {
    pub builtins: Vec<LoadedPlugin>,
    pub libraries: Vec<LoadedPluginLibrary>,
    pub registry: PluginRegistry,
}

pub fn load_plugins(paths: &[PathBuf]) -> Result<PluginLoadResult, PluginLoadError> {
    // Host bridges are per workload: each compiled graph gets its own `HostBridgeManager`.
    let mut registry = PluginRegistry::new();
    // Styx frames first: every plugin taking a `FrameLease` relies on its type key.
    registry.install(&StyxFramesPlugin::new())?;
    let builtins = install_builtin_plugins(&mut registry)?;
    let libraries = paths.iter().map(|path| load_plugin_library(path, &mut registry)).collect::<Result<Vec<_>, _>>()?;

    Ok(PluginLoadResult { builtins, libraries, registry })
}

fn load_plugin_library(path: &Path, registry: &mut PluginRegistry) -> Result<LoadedPluginLibrary, PluginLoadError> {
    #[allow(unsafe_code)]
    let library = unsafe { PluginLibrary::load(path) }.map_err(|source| PluginLoadError::Library { path: path.to_path_buf(), source: Box::new(source) })?;
    // Plugins from the engine's own cargo build install through the Rust ABI, with their Rust
    // types (`FrameLease`, the plugin's hand-off types). A plugin from another toolchain falls
    // back to Daedalus's stable path, which carries schema nodes and `Value`s but no Rust types.
    let install_path = library.install_into(registry).map_err(|source| PluginLoadError::Library { path: path.to_path_buf(), source: Box::new(source) })?;
    if install_path != InstallPath::RustAbi {
        tracing::warn!(path = %path.display(), ?install_path, reason = ?library.rust_abi().err(), "plugin was not built with this engine; frame and other Rust-typed ports will not work through the stable path");
    }

    let info = library.info();
    // The plugin id (what `GraphDocument::requires` and workload `plugin.N.name` name), not the
    // crate name the library info carries.
    let plugin_id = Some(library.schema().plugin.name.clone()).filter(|name| !name.is_empty());
    let metadata = LoadedPlugin {
        path: path.to_path_buf(),
        plugin_name: plugin_id.or_else(|| info.plugin_name.as_str().map(ToOwned::to_owned)),
        plugin_version: info.plugin_version.as_str().map(ToOwned::to_owned),
        daedalus_version: info.daedalus_version.as_str().map(ToOwned::to_owned),
    };

    Ok(LoadedPluginLibrary { metadata, install_path, _library: library })
}
