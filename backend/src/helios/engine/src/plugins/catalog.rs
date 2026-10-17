//! The node catalog: every node the loaded plugins declare, as Daedalus `NodeDecl`s grouped by
//! the plugin that provides them, for helios-api's `/v1/catalog` and the UI's node palette.

use std::{
    io,
    path::{Path, PathBuf},
};

use daedalus::PluginRegistry;
use daedalus::registry::capability::{NodeDecl, PortDecl};
use serde_json::{Map, Value, json};

pub const CATALOG_FORMAT: &str = "helios.node-catalog";
pub const CATALOG_SCHEMA_VERSION: u32 = 1;

pub fn catalog(registry: &PluginRegistry) -> Value {
    let capabilities = &registry.transport_capabilities;
    let plugins = capabilities.plugins().values().map(|manifest| json!({ "id": manifest.id, "version": manifest.version })).collect::<Vec<_>>();
    let nodes = capabilities
        .nodes()
        .values()
        .map(|decl| {
            let plugin = capabilities
                .plugins()
                .values()
                .find(|manifest| manifest.provided_nodes.contains(&decl.id))
                .map(|manifest| manifest.id.clone())
                // HeliOS's builtins have no manifest; their ids are `<plugin>:<node>`.
                .or_else(|| decl.id.0.split_once(':').map(|(plugin, _)| plugin.to_string()));
            node_entry(registry, decl, plugin)
        })
        .collect::<Vec<_>>();
    json!({ "format": CATALOG_FORMAT, "schema_version": CATALOG_SCHEMA_VERSION, "plugins": plugins, "nodes": nodes })
}

/// A node as the palette needs it: its ports, and for each constant input its default, range,
/// choices and description. A node-group's embedded graph stays out (it is most of a
/// `NodeDecl`'s size); `group` says it has one.
fn node_entry(registry: &PluginRegistry, decl: &NodeDecl, plugin: Option<String>) -> Value {
    let meta = |key: &str| decl.metadata_json.get(key).and_then(|raw| serde_json::from_str::<Value>(raw).ok()).map(plain);
    let port = |port: &PortDecl, input: bool| {
        let mut entry = Map::new();
        entry.insert("name".into(), port.name.clone().into());
        entry.insert("type_key".into(), port.type_key.as_str().into());
        let schema = port.schema.as_ref().map(|schema| serde_json::to_value(schema).unwrap_or(Value::Null));
        if port.optional {
            entry.insert("optional".into(), true.into());
        }
        if input {
            if let Some(default) = port.const_value_json.as_deref().and_then(|raw| serde_json::from_str::<Value>(raw).ok()) {
                entry.insert("default".into(), plain(default));
            }
            let declared = registry.transport_capabilities.type_decl(&port.type_key).and_then(|decl| decl.schema.as_ref()).map(|schema| serde_json::to_value(schema).unwrap_or(Value::Null));
            if let Some(options) = schema.as_ref().and_then(enum_options).or_else(|| declared.as_ref().and_then(enum_options)) {
                entry.insert("options".into(), options.into());
            }
            for key in ["description", "min", "max", "step"] {
                if let Some(value) = meta(&format!("inputs.{}.{key}", port.name)) {
                    entry.insert(key.into(), value);
                }
            }
        }
        // A `typeexpr:` key spells its schema out, an opaque type's is its key, and an enum's is
        // its options.
        let redundant =
            |schema: &Value| port.type_key.as_str().starts_with("typeexpr:") || entry.contains_key("options") || schema.get("Opaque").and_then(Value::as_str) == Some(port.type_key.as_str());
        if let Some(schema) = schema.filter(|schema| !redundant(schema)) {
            entry.insert("schema".into(), schema);
        }
        Value::Object(entry)
    };
    let mut node = Map::new();
    node.insert("id".into(), decl.id.0.clone().into());
    if let Some(plugin) = plugin {
        node.insert("plugin".into(), plugin.into());
    }
    if let Some(label) = &decl.label {
        node.insert("label".into(), label.clone().into());
    }
    if let Some(summary) = meta("summary") {
        node.insert("summary".into(), summary);
    }
    if decl.metadata_json.contains_key("daedalus.embedded_graph") {
        node.insert("group".into(), true.into());
    }
    node.insert("inputs".into(), decl.inputs.iter().map(|p| port(p, true)).collect::<Vec<_>>().into());
    node.insert("outputs".into(), decl.outputs.iter().map(|p| port(p, false)).collect::<Vec<_>>().into());
    Value::Object(node)
}

/// A Daedalus `Value` (`{"type":"Int","value":7}`) as plain JSON (`7`).
fn plain(value: Value) -> Value {
    match value {
        Value::Object(mut fields) if fields.len() == 2 && fields.get("type").is_some_and(Value::is_string) && fields.contains_key("value") => fields.remove("value").unwrap_or(Value::Null),
        other => other,
    }
}

/// The variant names of an enum `TypeExpr` (`{"Enum":[{"name":..}, ..]}`).
fn enum_options(schema: &Value) -> Option<Vec<Value>> {
    let variants = schema.get("Enum")?.as_array()?;
    Some(variants.iter().filter_map(|variant| variant.get("name").cloned()).collect())
}

/// Write `registry`'s catalog to `path` (through a temporary file, so a reader never sees half
/// of one).
pub fn write_catalog(registry: &PluginRegistry, path: &Path) -> io::Result<PathBuf> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, serde_json::to_vec(&catalog(registry))?)?;
    std::fs::rename(&partial, path)?;
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_lists_builtin_nodes_with_their_plugin() {
        let loaded = crate::plugins::load_plugins(&[]).expect("builtin plugins load");
        let catalog = catalog(&loaded.registry);
        assert_eq!(catalog["format"], CATALOG_FORMAT);
        let nodes = catalog["nodes"].as_array().expect("nodes");
        assert!(!nodes.is_empty());
        assert!(nodes.iter().all(|node| node["id"].is_string()));
        assert!(nodes.iter().any(|node| node["plugin"].is_string()));
        let dir = std::env::temp_dir().join(format!("helios-catalog-{}", std::process::id()));
        let path = write_catalog(&loaded.registry, &dir.join("catalog.json")).expect("write");
        let read: Value = serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
        assert_eq!(read, catalog);
        let _ = std::fs::remove_dir_all(dir);
    }
}
