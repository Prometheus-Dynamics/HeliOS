use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("HELIOS_LIBCV_SCRATCH_DIAGNOSTICS")
            .ok()
            .map(|raw| {
                let value = raw.trim().to_ascii_lowercase();
                matches!(value.as_str(), "1" | "true" | "yes" | "on")
            })
            .unwrap_or(false)
    })
}

fn high_waters() -> &'static Mutex<HashMap<&'static str, usize>> {
    static HIGH_WATERS: OnceLock<Mutex<HashMap<&'static str, usize>>> = OnceLock::new();
    HIGH_WATERS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn report_scratch_high_water(name: &'static str, bytes: usize) {
    if !enabled() {
        return;
    }
    let Ok(mut high_waters) = high_waters().lock() else {
        return;
    };
    let previous = high_waters.get(name).copied().unwrap_or(0);
    if bytes <= previous {
        return;
    }
    high_waters.insert(name, bytes);
    tracing::info!(
        target: "libcv::scratch",
        scratch = name,
        bytes,
        mib = bytes as f64 / (1024.0 * 1024.0),
        "libcv scratch high-water"
    );
    eprintln!("libcv scratch high-water scratch={} bytes={} mib={:.3}", name, bytes, bytes as f64 / (1024.0 * 1024.0));
}
