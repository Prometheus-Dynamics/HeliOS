use std::path::PathBuf;

use daedalus::{
    PluginRegistry, plugin,
    runtime::plugins::{PluginError, RegistryPluginExt},
};

use super::resource::{BoundResource, ResourceAction, ResourceActionOutcome, ResourceConfigField, ResourceLease, ResourceValue};
use crate::model::LoadedPlugin;

/// Id of [`HeliosPlugin`].
pub const HELIOS_PLUGIN_ID: &str = "helios.builtin";

/// The engine's own types: the bound resource a resource binding pushes into its host input, with
/// value serializers so outputs and the UI can inspect it.
#[plugin(id = "helios.builtin", values(BoundResource, ResourceAction, ResourceConfigField, ResourceValue, ResourceLease, ResourceActionOutcome))]
pub struct HeliosPlugin;

/// The engine's own plugins; with `builtin_eidos` (and the `static-eidos` feature) also Eidos's,
/// linked in, so no `libhelios_eidos_plugin.so` is loaded.
pub fn install_builtin_plugins(registry: &mut PluginRegistry, builtin_eidos: bool) -> Result<Vec<LoadedPlugin>, PluginError> {
    let plugin = HeliosPlugin::new();
    registry.install_plugin(&plugin)?;
    #[allow(unused_mut)]
    let mut builtins = vec![LoadedPlugin { path: PathBuf::from("<builtin>/helios"), plugin_name: Some(HELIOS_PLUGIN_ID.into()), plugin_version: None, daedalus_version: None }];
    #[cfg(feature = "static-eidos")]
    if builtin_eidos {
        let eidos = eidos_daedalus::EidosPlugin::new();
        registry.install(&eidos)?;
        builtins.push(LoadedPlugin {
            path: PathBuf::from("<builtin>/eidos"),
            plugin_name: Some(daedalus::runtime::plugins::Plugin::id(&eidos).into()),
            plugin_version: registry.installed_plugin_version(daedalus::runtime::plugins::Plugin::id(&eidos)).flatten().map(ToOwned::to_owned),
            daedalus_version: None,
        });
    }
    #[cfg(not(feature = "static-eidos"))]
    let _ = builtin_eidos;
    Ok(builtins)
}
