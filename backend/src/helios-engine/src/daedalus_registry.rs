use daedalus::data::model::Value as DaedalusValue;
use daedalus::ffi::{FFI_VERSION, PLUGIN_ABI_VERSION};
use daedalus::planner::Graph;
use daedalus::registry::store::NodeDescriptorBuilder;
use daedalus::runtime::handler_registry::HandlerRegistry as DaedalusHandlers;
use daedalus::runtime::host_bridge::HostBridgeManager as DaedalusBridgeManager;
use daedalus::runtime::host_bridge::{bridge_handler, HOST_BRIDGE_META_KEY};
use daedalus::runtime::plugins::PluginRegistry;
use daedalus::PluginLibrary;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tracing::warn;

use crate::ipc::PluginCompatibility;

pub struct DaedalusRuntimeRegistry {
    pub registry: PluginRegistry,
    pub plugins: Vec<String>,
    pub handlers: DaedalusHandlers,
}

impl DaedalusRuntimeRegistry {
    pub fn into_parts(self) -> (PluginRegistry, DaedalusHandlers, Vec<String>) {
        (self.registry, self.handlers, self.plugins)
    }
}

pub fn build_daedalus_runtime_registry(host_mgr: &DaedalusBridgeManager, graph: Option<&Graph>) -> Result<DaedalusRuntimeRegistry, &'static str> {
    let mut registry = PluginRegistry::new();
    let plugins = {
        let mut plugin_ids = vec!["host_bridge".into()];
        let installed_ids = install_dynamic_plugins_cached(&mut registry, graph)?;
        plugin_ids.extend(installed_ids);
        // Dynamic plugins are discovered/loaded via `DYNAMIC_PLUGIN_CACHE` so we don't redo
        // directory scans and `dlopen` work for every stream/graph build.
        //
        // NOTE: Upstream Daedalus `PluginLibrary::load()` keeps the dylib loaded for the
        // lifetime of the process, so callers do not need to retain `PluginLibrary` values to
        // prevent accidental unloading.
        plugin_ids
    };

    // Always install generic dynamic host-bridge nodes.
    //
    // IMPORTANT: do not declare per-port inputs/outputs here. Declaring them as `generic` prevents
    // Daedalus from inferring the real `dynamic_*_types` from edges, which is required for Helios
    // to poll image outputs (`overlay`, `clahe`, etc.) reliably.
    let _ = graph;
    install_typed_host_bridge(&mut registry, host_mgr.clone())?;
    install_typed_host_output(&mut registry, host_mgr.clone())?;

    registry.apply_type_compatibilities();
    let handlers = registry.take_handlers();
    Ok(DaedalusRuntimeRegistry { registry, plugins, handlers })
}

fn graph_plugin_namespaces(graph: Option<&Graph>) -> Option<BTreeSet<String>> {
    let graph = graph?;
    let mut namespaces = BTreeSet::new();
    for node in &graph.nodes {
        let id = node.id.0.as_str();
        let Some((namespace, _rest)) = id.split_once(':') else {
            continue;
        };
        match namespace {
            "cv" | "ai" | "nt4" | "led" => {
                namespaces.insert(namespace.to_string());
            }
            _ => {}
        }
    }
    Some(namespaces)
}

fn plugin_namespace_from_filename(name: &str) -> Option<&'static str> {
    if name.contains("_cv_") {
        return Some("cv");
    }
    if name.contains("_ai_") {
        return Some("ai");
    }
    if name.contains("_nt4_") {
        return Some("nt4");
    }
    if name.contains("_led_") {
        return Some("led");
    }
    None
}

fn plugin_requested_for_graph(name: &str, requested_namespaces: Option<&BTreeSet<String>>) -> bool {
    let Some(requested_namespaces) = requested_namespaces else {
        return true;
    };
    let Some(namespace) = plugin_namespace_from_filename(name) else {
        return true;
    };
    requested_namespaces.contains(namespace)
}

fn parse_plugin_dirs() -> Vec<PathBuf> {
    // Match helios-api behavior:
    // - If a single override dir is set, use it exclusively (so dev deployments can override
    //   system-installed plugins without being shadowed by duplicate filenames).
    // - Otherwise, fall back to the colon-separated search list.
    if let Ok(single) = env::var("HELIOS_DAEDALUS_PLUGIN_DIR") {
        let trimmed = single.trim();
        if !trimmed.is_empty() {
            return vec![PathBuf::from(trimmed)];
        }
    }

    if let Ok(list) = env::var("HELIOS_DAEDALUS_PLUGIN_DIRS") {
        let dirs: Vec<_> = env::split_paths(&list).collect();
        if !dirs.is_empty() {
            return dirs;
        }
    }

    vec![PathBuf::from("/var/lib/helios/plugins/daedalus"), PathBuf::from("/usr/lib/helios/plugins/daedalus")]
}

fn collect_shared_objects(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("so") {
            continue;
        }
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            out.push(path);
        }
    }
    out.sort();
    out
}

fn collect_disabled_plugins_with_paths(dir: &Path) -> BTreeMap<String, PathBuf> {
    let mut out = BTreeMap::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".so.disabled") {
            let trimmed = name.trim_end_matches(".disabled").to_string();
            out.insert(trimmed, entry.path());
        }
    }
    out
}

struct DynamicPluginCache {
    libs: Vec<(PluginLibrary, PathBuf, String)>,
    diagnostics: BTreeMap<String, PluginCompatibility>,
}

static DYNAMIC_PLUGIN_CACHE: OnceLock<Mutex<Result<DynamicPluginCache, String>>> = OnceLock::new();

fn init_dynamic_plugin_cache(requested_namespaces: Option<&BTreeSet<String>>) -> Result<DynamicPluginCache, String> {
    let dirs = parse_plugin_dirs();
    let mut disabled = BTreeMap::new();
    for dir in &dirs {
        if dir.exists() {
            disabled.extend(collect_disabled_plugins_with_paths(dir));
        }
    }

    let mut by_name: std::collections::BTreeMap<String, PathBuf> = std::collections::BTreeMap::new();
    for dir in dirs {
        if !dir.exists() {
            continue;
        }
        for path in collect_shared_objects(&dir) {
            let Some(name) = path.file_name().and_then(|v| v.to_str()).map(ToString::to_string) else {
                continue;
            };
            if disabled.contains_key(&name) {
                continue;
            }
            if !plugin_requested_for_graph(&name, requested_namespaces) {
                continue;
            }
            // Preserve first-seen directory priority so writable install dirs (e.g. /var/lib)
            // can intentionally override read-only system plugin copies (e.g. /usr/lib).
            by_name.entry(name).or_insert(path);
        }
    }

    let mut diagnostics = BTreeMap::new();
    for (name, path) in &disabled {
        diagnostics.insert(name.clone(), plugin_diag_disabled(name, path));
    }

    let mut libs = Vec::new();
    for (name, path) in by_name {
        #[allow(unsafe_code)]
        match unsafe { PluginLibrary::load(&path) } {
            Ok(lib) => {
                let diag = plugin_diag_from_lib(&lib, &path, &name);
                if diag.status == "ok" {
                    libs.push((lib, path, name.clone()));
                }
                diagnostics.insert(name, diag);
            }
            Err(err) => {
                warn!(path = ?path, error = %err, "daedalus plugin load failed");
                let diag = plugin_diag_load_failed(&name, &path, &err);
                diagnostics.insert(name, diag);
            }
        }
    }

    Ok(DynamicPluginCache { libs, diagnostics })
}

fn current_disabled_plugins() -> BTreeMap<String, PathBuf> {
    let mut out = BTreeMap::new();
    for dir in parse_plugin_dirs() {
        if dir.exists() {
            out.extend(collect_disabled_plugins_with_paths(&dir));
        }
    }
    out
}

fn refresh_dynamic_plugin_cache(cache: &mut DynamicPluginCache, requested_namespaces: Option<&BTreeSet<String>>) {
    let mut loaded_paths = BTreeSet::new();
    for (_, path, _) in &cache.libs {
        loaded_paths.insert(path.clone());
    }

    let disabled = current_disabled_plugins();
    let mut seen_names: BTreeSet<String> = disabled.keys().cloned().collect();
    for (lib, path, name) in &cache.libs {
        seen_names.insert(name.clone());
        if !cache.diagnostics.contains_key(name) {
            let diag = plugin_diag_from_lib(lib, path, name);
            cache.diagnostics.insert(name.clone(), diag);
        }
    }
    for (name, path) in &disabled {
        cache.diagnostics.insert(name.clone(), plugin_diag_disabled(name, path));
    }
    for dir in parse_plugin_dirs() {
        if !dir.exists() {
            continue;
        }
        for path in collect_shared_objects(&dir) {
            if loaded_paths.contains(&path) {
                continue;
            }
            let Some(name) = path.file_name().and_then(|v| v.to_str()).map(ToString::to_string) else {
                continue;
            };
            if !plugin_requested_for_graph(&name, requested_namespaces) {
                continue;
            }
            // Preserve first-seen directory priority across refreshes as well. A plugin copied into
            // the writable deploy dir should continue to win over same-named system copies instead
            // of being double-loaded and re-registered later in the process.
            if seen_names.contains(&name) {
                continue;
            }
            seen_names.insert(name.clone());
            if disabled.contains_key(&name) {
                continue;
            }
            #[allow(unsafe_code)]
            match unsafe { PluginLibrary::load(&path) } {
                Ok(lib) => {
                    let diag = plugin_diag_from_lib(&lib, &path, &name);
                    if diag.status == "ok" {
                        loaded_paths.insert(path.clone());
                        cache.libs.push((lib, path, name.clone()));
                    }
                    cache.diagnostics.insert(name, diag);
                }
                Err(err) => {
                    warn!(path = ?path, error = %err, "daedalus plugin load failed");
                    let diag = plugin_diag_load_failed(&name, &path, &err);
                    cache.diagnostics.insert(name, diag);
                }
            }
        }
    }

    cache.diagnostics.retain(|name, _| seen_names.contains(name));
}

fn install_dynamic_plugins_cached(registry: &mut PluginRegistry, graph: Option<&Graph>) -> Result<Vec<String>, &'static str> {
    let requested_namespaces = graph_plugin_namespaces(graph);
    let cache = DYNAMIC_PLUGIN_CACHE.get_or_init(|| Mutex::new(init_dynamic_plugin_cache(requested_namespaces.as_ref())));
    let mut guard = cache.lock().map_err(|_| "dynamic plugin cache lock poisoned")?;
    let cache = guard.as_mut().map_err(|_| "dynamic plugin load failed")?;
    refresh_dynamic_plugin_cache(cache, requested_namespaces.as_ref());
    let disabled = current_disabled_plugins();
    tracing::info!(plugins = cache.libs.len(), disabled = disabled.len(), requested_namespaces = ?requested_namespaces, "daedalus plugin scan complete");

    let mut names = Vec::new();
    for (lib, path, name) in &cache.libs {
        if disabled.contains_key(name) {
            continue;
        }
        let status = cache.diagnostics.get(name).map(|diag| diag.status.as_str()).unwrap_or("missing");
        if status != "ok" {
            tracing::info!(plugin = %name, status, "daedalus plugin skipped");
            continue;
        }
        tracing::info!(plugin = %name, "daedalus plugin installing");
        if let Err(err) = lib.install_into(registry) {
            warn!(path = ?path, error = %err, "daedalus plugin install failed");
            let mut diag = cache.diagnostics.get(name).cloned().unwrap_or_else(|| plugin_diag_from_lib(lib, path, name));
            diag.status = "install_failed".to_string();
            diag.reason = Some(err.to_string());
            cache.diagnostics.insert(name.clone(), diag);
            continue;
        }
        tracing::info!(plugin = %name, "daedalus plugin installed");
        names.push(name.clone());
    }
    Ok(names)
}

pub fn plugin_diagnostics() -> Vec<PluginCompatibility> {
    let cache = DYNAMIC_PLUGIN_CACHE.get_or_init(|| Mutex::new(init_dynamic_plugin_cache(None)));
    let mut guard = match cache.lock() {
        Ok(guard) => guard,
        Err(_) => return Vec::new(),
    };
    let cache = match guard.as_mut() {
        Ok(cache) => cache,
        Err(_) => return Vec::new(),
    };
    refresh_dynamic_plugin_cache(cache, None);
    cache.diagnostics.values().cloned().collect()
}

fn plugin_diag_base(filename: &str, path: &Path) -> PluginCompatibility {
    PluginCompatibility {
        filename: filename.to_string(),
        plugin_name: None,
        plugin_version: None,
        status: "ok".to_string(),
        reason: None,
        expected_daedalus_version: daedalus::version().to_string(),
        daedalus_version: None,
        expected_ffi_version: FFI_VERSION.to_string(),
        ffi_version: None,
        expected_abi_version: PLUGIN_ABI_VERSION,
        abi_version: None,
        path: path.display().to_string(),
    }
}

fn plugin_diag_disabled(filename: &str, path: &Path) -> PluginCompatibility {
    let mut diag = plugin_diag_base(filename, path);
    diag.status = "disabled".to_string();
    diag.reason = Some("plugin disabled".to_string());
    diag
}

fn plugin_diag_load_failed(filename: &str, path: &Path, err: &dyn std::fmt::Display) -> PluginCompatibility {
    let mut diag = plugin_diag_base(filename, path);
    diag.status = "load_failed".to_string();
    diag.reason = Some(err.to_string());
    diag
}

fn plugin_diag_from_lib(lib: &PluginLibrary, path: &Path, filename: &str) -> PluginCompatibility {
    let mut diag = plugin_diag_base(filename, path);
    diag.abi_version = lib.abi_version();
    if diag.abi_version.is_none() {
        diag.status = "missing_abi".to_string();
        diag.reason = Some("missing ABI version symbol".to_string());
        warn!(path = ?path, "daedalus plugin missing ABI version symbol; skipping");
        return diag;
    }
    if diag.abi_version != Some(PLUGIN_ABI_VERSION) {
        diag.status = "abi_mismatch".to_string();
        diag.reason = Some(format!("ABI mismatch (expected {}, got {})", PLUGIN_ABI_VERSION, diag.abi_version.unwrap_or_default()));
        warn!(
            path = ?path,
            abi = ?diag.abi_version,
            expected = PLUGIN_ABI_VERSION,
            "daedalus plugin ABI mismatch; skipping"
        );
        return diag;
    }

    let info = lib.info();
    if let Some(info) = info.as_ref() {
        diag.plugin_name = info.plugin_name.as_str().map(|v| v.to_string());
        diag.plugin_version = info.plugin_version.as_str().map(|v| v.to_string());
        diag.ffi_version = info.ffi_version.as_str().map(|v| v.to_string());
        diag.daedalus_version = info.daedalus_version.as_str().map(|v| v.to_string());
    } else {
        diag.status = "missing_info".to_string();
        diag.reason = Some("missing plugin info symbol".to_string());
        warn!(path = ?path, "daedalus plugin missing info symbol; skipping");
        return diag;
    }

    if diag.ffi_version.as_deref() != Some(FFI_VERSION) {
        let actual = diag.ffi_version.clone().unwrap_or_else(|| "unknown".to_string());
        diag.status = "ffi_mismatch".to_string();
        diag.reason = Some(format!("FFI mismatch (expected {FFI_VERSION}, got {actual})"));
        warn!(
            path = ?path,
            plugin = ?diag.plugin_name,
            plugin_version = ?diag.plugin_version,
            ffi_version = ?diag.ffi_version,
            expected = FFI_VERSION,
            "daedalus plugin FFI version mismatch; skipping"
        );
        return diag;
    }

    if diag.daedalus_version.as_deref() != Some(daedalus::version()) {
        let actual = diag.daedalus_version.clone().unwrap_or_else(|| "unknown".to_string());
        diag.status = "daedalus_mismatch".to_string();
        diag.reason = Some(format!("Daedalus version mismatch (expected {}, got {})", daedalus::version(), actual));
        warn!(
            path = ?path,
            plugin = ?diag.plugin_name,
            plugin_version = ?diag.plugin_version,
            daedalus_version = ?diag.daedalus_version,
            expected = daedalus::version(),
            "daedalus plugin version mismatch; skipping"
        );
        return diag;
    }

    diag
}

fn install_typed_host_bridge(registry: &mut PluginRegistry, manager: DaedalusBridgeManager) -> Result<(), &'static str> {
    let qualified_id = if let Some(prefix) = registry.current_prefix.clone() { format!("{prefix}:io.host_bridge") } else { "io.host_bridge".to_string() };

    let desc = NodeDescriptorBuilder::new(&qualified_id)
        .metadata(HOST_BRIDGE_META_KEY, DaedalusValue::Bool(true))
        .metadata("dynamic_inputs", DaedalusValue::String("generic".into()))
        .metadata("dynamic_outputs", DaedalusValue::String("generic".into()))
        .build()
        .map_err(|_| "host bridge descriptor build failed")?;
    registry.registry.register_node(desc).map_err(|_| "host bridge descriptor register failed")?;

    let mut handler = bridge_handler(manager);
    registry.handlers.on_stateful(&qualified_id, move |node, ctx, io| handler(node, ctx, io));
    Ok(())
}

fn install_typed_host_output(registry: &mut PluginRegistry, manager: DaedalusBridgeManager) -> Result<(), &'static str> {
    let qualified_id = if let Some(prefix) = registry.current_prefix.clone() { format!("{prefix}:io.host_output") } else { "io.host_output".to_string() };

    let desc = NodeDescriptorBuilder::new(&qualified_id)
        .metadata(HOST_BRIDGE_META_KEY, DaedalusValue::Bool(true))
        .metadata("dynamic_inputs", DaedalusValue::String("generic".into()))
        .metadata("dynamic_outputs", DaedalusValue::String("generic".into()))
        .build()
        .map_err(|_| "host output descriptor build failed")?;
    registry.registry.register_node(desc).map_err(|_| "host output descriptor register failed")?;

    let mut handler = bridge_handler(manager);
    registry.handlers.on_stateful(&qualified_id, move |node, ctx, io| handler(node, ctx, io));
    Ok(())
}
