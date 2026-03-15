use std::sync::OnceLock;

fn env_flag_enabled(var: &str, default_value: bool) -> bool {
    let raw = match std::env::var(var) {
        Ok(v) => v,
        Err(_) => return default_value,
    };
    let v = raw.trim().to_ascii_lowercase();
    if v.is_empty() {
        return default_value;
    }
    matches!(v.as_str(), "1" | "true" | "yes" | "y" | "on" | "enabled")
}

/// Global gate for the shadow recorder feature.
///
/// Default: enabled.
/// Disable by setting `HELIOS_ENABLE_SHADOW_RECORDER=0`.
pub fn shadow_recorder_enabled() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| env_flag_enabled("HELIOS_ENABLE_SHADOW_RECORDER", true))
}

/// Optional warm-up for the pipeline registry at API startup.
///
/// Default: disabled.
/// Enable by setting `HELIOS_API_WARM_PIPELINE_REGISTRY=1`.
pub fn warm_pipeline_registry_enabled() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| env_flag_enabled("HELIOS_API_WARM_PIPELINE_REGISTRY", false))
}

/// Hint the UI that it may prefetch the pipeline registry in the background.
///
/// Default: disabled.
/// Enable by setting `HELIOS_UI_PREFETCH_PIPELINE_REGISTRY=1`.
pub fn prefetch_pipeline_registry_enabled() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| env_flag_enabled("HELIOS_UI_PREFETCH_PIPELINE_REGISTRY", false))
}

/// Optional startup-time injection of pipeline port metadata while seeding startup presets.
///
/// Default: disabled.
/// Enable by setting `HELIOS_API_STARTUP_INJECT_PIPELINE_METADATA=1`.
pub fn startup_pipeline_metadata_injection_enabled() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| env_flag_enabled("HELIOS_API_STARTUP_INJECT_PIPELINE_METADATA", false))
}
