//! Eidos's Daedalus plugin as the native plugin library HeliOS installs.
//!
//! `cargo build -p helios-engine -p helios-eidos-plugin` builds
//! `libhelios_eidos_plugin.so`; install it into one of the engine's plugin
//! directories (`HELIOS_DAEDALUS_PLUGIN_DIRS`). HeliOS adds nothing to it:
//! every node, type and template is `eidos_daedalus::EidosPlugin` (plugin id
//! `eidos`, node ids `eidos:*`). It depends on `styx.frames`, which the engine
//! installs before loading plugins, and links Styx's frame plugin so the
//! library's own schema extraction resolves the frame type.

// SAFETY: `export_plugin!` emits the two `#[unsafe(no_mangle)]` entry points
// Daedalus's loader looks up (`daedalus_plugin_abi_version` and
// `daedalus_plugin_descriptor`). This is their only definition in the library.
#[allow(unsafe_code)]
mod export {
    daedalus::export_plugin!(eidos_daedalus::EidosPlugin, deps[styx::core::daedalus::StyxFramesPlugin]);
}
