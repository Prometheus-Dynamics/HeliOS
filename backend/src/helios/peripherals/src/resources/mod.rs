mod builder;
mod records;
mod service;
mod styx;

pub use builder::ResourceBuilder;
pub use records::{DiscoveryContext, DiscoveryError, DiscoveryProbe, DiscoverySnapshot, ProbeReport};
pub use service::{InventoryRefreshReport, PeripheralInventoryService};
pub use styx::{CaptureProbe, probe_capture_devices};

/// A name readable as one id segment: ASCII letters and digits in lower case, `/ . _ : ` and
/// spaces as one `-` each, everything else dropped; `fallback` when nothing is left.
pub(crate) fn sanitize_local_component(value: &str, fallback: &str) -> String {
    let mut cleaned = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            cleaned.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '/' | '.' | '_' | ':' | ' ') && !cleaned.ends_with('-') {
            cleaned.push('-');
        }
    }
    let cleaned = cleaned.trim_matches('-');
    if cleaned.is_empty() { fallback.into() } else { cleaned.into() }
}
