use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeScratchMetric {
    pub name: &'static str,
    pub high_water_bytes: usize,
}

fn high_water_registry() -> &'static Mutex<BTreeMap<&'static str, usize>> {
    static VALUE: OnceLock<Mutex<BTreeMap<&'static str, usize>>> = OnceLock::new();
    VALUE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub(crate) fn record_high_water(name: &'static str, bytes: usize) {
    let Ok(mut registry) = high_water_registry().lock() else {
        return;
    };
    registry
        .entry(name)
        .and_modify(|existing| {
            if bytes > *existing {
                *existing = bytes;
            }
        })
        .or_insert(bytes);
}

pub fn snapshot_high_water() -> Vec<RuntimeScratchMetric> {
    let Ok(registry) = high_water_registry().lock() else {
        return Vec::new();
    };
    registry.iter().map(|(name, high_water_bytes)| RuntimeScratchMetric { name, high_water_bytes: *high_water_bytes }).collect()
}

pub fn compact_after_frame() {
    compact_current_thread();
    if should_broadcast_compaction() {
        rayon::broadcast(|_| {
            compact_current_thread();
        });
    }
}

pub fn release_on_idle() {
    release_current_thread();
    if should_broadcast_compaction() {
        rayon::broadcast(|_| {
            release_current_thread();
        });
    }
}

fn compact_current_thread() {
    #[cfg(feature = "aruco")]
    crate::modules::aruco::compact_runtime_scratch_after_frame();
    crate::modules::image::compact_runtime_scratch_after_frame();
    #[cfg(feature = "contour")]
    crate::modules::contour::compact_runtime_scratch_after_frame();
    crate::ops::compact_runtime_scratch_after_frame();
}

fn release_current_thread() {
    compact_current_thread();
    crate::modules::image::release_runtime_scratch_on_idle();
    #[cfg(feature = "aruco")]
    crate::modules::aruco::release_runtime_scratch_on_idle();
    #[cfg(feature = "contour")]
    crate::modules::contour::release_runtime_scratch_on_idle();
}

fn should_broadcast_compaction() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("RAYON_NUM_THREADS")
            .ok()
            .and_then(|raw| raw.trim().parse::<usize>().ok())
            .map(|threads| threads > 1)
            .unwrap_or_else(|| std::thread::available_parallelism().map(|threads| threads.get() > 1).unwrap_or(false))
    })
}

#[cfg(test)]
mod tests {
    use super::{record_high_water, snapshot_high_water};

    #[test]
    fn scratch_snapshot_keeps_high_water_per_metric() {
        record_high_water("runtime.a", 16);
        record_high_water("runtime.a", 8);
        record_high_water("runtime.b", 32);

        let snapshot = snapshot_high_water();
        assert!(snapshot.iter().any(|metric| metric.name == "runtime.a" && metric.high_water_bytes == 16));
        assert!(snapshot.iter().any(|metric| metric.name == "runtime.b" && metric.high_water_bytes == 32));
    }
}
