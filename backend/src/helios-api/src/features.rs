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
/// Default: disabled (so we can ship the rest of recording without the shadow buffer UX).
/// Enable by setting `HELIOS_ENABLE_SHADOW_RECORDER=1`.
pub fn shadow_recorder_enabled() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| env_flag_enabled("HELIOS_ENABLE_SHADOW_RECORDER", false))
}
