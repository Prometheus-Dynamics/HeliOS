pub mod builtin;
pub mod catalog;
pub mod discovery;
pub mod loader;
pub mod resource;

pub use builtin::install_builtin_plugins;
pub use discovery::discover_plugin_libraries;
pub use loader::{LoadedPluginLibrary, PluginInstall, PluginLoadError, PluginLoadResult, load_plugins, load_plugins_as, load_plugins_with, parse_plugin_install};
