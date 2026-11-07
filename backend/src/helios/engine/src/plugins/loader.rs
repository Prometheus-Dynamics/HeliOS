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
    /// Where the node catalog was written, once it was.
    pub catalog_path: Option<PathBuf>,
}

/// How plugin libraries install (`HELIOS_ENGINE_PLUGIN_INSTALL`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PluginInstall {
    /// Daedalus's choice: the Rust ABI for a plugin from the engine's own build, the stable path
    /// for one from another toolchain.
    #[default]
    Auto,
    /// The stable path for every plugin, also one from the engine's own build (Daedalus's
    /// `install_into_as`), to measure what the stable path costs a real graph.
    Stable,
}

/// `auto` or `stable` (case-insensitive).
pub fn parse_plugin_install(value: &str) -> Option<PluginInstall> {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" | "" => Some(PluginInstall::Auto),
        "stable" => Some(PluginInstall::Stable),
        _ => None,
    }
}

pub fn load_plugins(paths: &[PathBuf]) -> Result<PluginLoadResult, PluginLoadError> {
    load_plugins_as(paths, PluginInstall::Auto)
}

pub fn load_plugins_as(paths: &[PathBuf], install: PluginInstall) -> Result<PluginLoadResult, PluginLoadError> {
    load_plugins_with(paths, install, true)
}

/// As [`load_plugins_as`]; `builtin_eidos: false` takes Eidos from its library even in an engine
/// that has it built in (`HELIOS_ENGINE_BUILTIN_EIDOS=0`: the library form, for an A/B or a
/// newer plugin build).
pub fn load_plugins_with(paths: &[PathBuf], install: PluginInstall, builtin_eidos: bool) -> Result<PluginLoadResult, PluginLoadError> {
    // Host bridges are per workload: each compiled graph gets its own `HostBridgeManager`.
    let mut registry = PluginRegistry::new();
    // Styx frames first: every plugin taking a `FrameLease` relies on its type key.
    registry.install(&StyxFramesPlugin::new())?;
    let builtins = install_builtin_plugins(&mut registry, builtin_eidos)?;
    let libraries = paths.iter().filter_map(|path| load_plugin_library(path, &mut registry, install).transpose()).collect::<Result<Vec<_>, _>>()?;

    Ok(PluginLoadResult { builtins, libraries, registry, catalog_path: None })
}

/// Loads and installs one library; `None` for a plugin the engine already has built in.
fn load_plugin_library(path: &Path, registry: &mut PluginRegistry, install: PluginInstall) -> Result<Option<LoadedPluginLibrary>, PluginLoadError> {
    #[allow(unsafe_code)]
    let library = unsafe { PluginLibrary::load(path) }.map_err(|source| PluginLoadError::Library { path: path.to_path_buf(), source: Box::new(source) })?;
    let id = library.schema().plugin.name.clone();
    if registry.installed_plugin_version(&id).is_some() {
        tracing::info!(path = %path.display(), plugin = %id, "plugin library skipped: built into this engine");
        return Ok(None);
    }
    // Plugins from the engine's own cargo build install through the Rust ABI, with their Rust
    // types (`FrameLease`, the plugin's hand-off types). A plugin from another toolchain falls
    // back to Daedalus's stable path, which carries schema nodes and `Value`s but no Rust types.
    let error = |source| PluginLoadError::Library { path: path.to_path_buf(), source: Box::new(source) };
    let install_path = match install {
        PluginInstall::Auto => library.install_into(registry).map_err(error)?,
        PluginInstall::Stable => {
            library.install_into_as(registry, InstallPath::Stable).map_err(error)?;
            InstallPath::Stable
        }
    };
    if install == PluginInstall::Stable {
        tracing::info!(path = %path.display(), "plugin installed through the stable path (HELIOS_ENGINE_PLUGIN_INSTALL=stable)");
    } else if install_path != InstallPath::RustAbi {
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

    Ok(Some(LoadedPluginLibrary { metadata, install_path, _library: library }))
}
