//! Saved maps in a live-map graph (`eidos:mapping.live_map`): the file a graph starts from and the
//! save request, both through the graph's host inputs (ports and wiring: `helios_maps`).
//!
//! - **Load.** A workload with a map (`ExecutionWorkload::map`) starts its graph with the map
//!   file's bytes in the held `map` input, pushed before the graph thread runs, so the first tick
//!   already has it. The buffer is pushed once, as one shared `Arc`: the mapper compares it by
//!   identity, so a restart (a new driver) is a new run and nothing else is.
//! - **Save.** The `save` input is a bool, saved on its rising edge. A new save request (a new
//!   value of `ExecutionWorkload::map_save`) raises it; the node saves on the next tick, gives
//!   `saved_map` on that tick, and the driver lowers `save` then. Requests that arrive while one is
//!   pending share its edge. Lowering after each save is what makes the next request an edge.

use std::{path::Path, sync::Arc};

use eidos_daedalus::types::{BYTES_TYPE_KEY, Bytes};

pub(crate) use helios_maps::{MAP_PORT, SAVE_PORT, SAVED_MAP_PORT};

/// The type key the map input takes (`eidos:bytes`).
pub(crate) const MAP_TYPE_KEY: &str = BYTES_TYPE_KEY;

/// The bytes of the map file at `path`, shared.
pub(crate) fn load_map(path: &Path) -> Result<Arc<Bytes>, String> {
    std::fs::read(path).map(|bytes| Arc::new(Bytes { bytes })).map_err(|error| format!("map {}: {error}", path.display()))
}

/// The `save` input's state, as the driver drives it: the last request it saw and whether it
/// holds `true` (raised, no `saved_map` yet).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SavePulse {
    seen: u64,
    raised: bool,
}

impl SavePulse {
    /// The workload's save counter is `count`. A new request raises `save`: `Some(true)` is the
    /// value to push. A request while one is pending, or no new request, pushes nothing.
    pub fn request(&mut self, count: u64) -> Option<bool> {
        if count <= self.seen {
            return None;
        }
        self.seen = count;
        if self.raised {
            return None;
        }
        self.raised = true;
        Some(true)
    }

    /// The graph gave `saved_map` on a tick (the edge was taken): `Some(false)` is the value to
    /// push to lower `save`, once.
    pub fn saved(&mut self) -> Option<bool> {
        std::mem::take(&mut self.raised).then_some(false)
    }

    /// Whether `save` is raised now.
    #[cfg(test)]
    pub fn raised(&self) -> bool {
        self.raised
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_request_is_one_rising_edge() {
        let mut pulse = SavePulse::default();
        assert_eq!(pulse.request(0), None, "no request yet");
        assert_eq!(pulse.request(1), Some(true), "a new request raises save");
        assert_eq!(pulse.request(1), None, "the same request does not raise it again");
        assert_eq!(pulse.saved(), Some(false), "the saved map lowers it, once");
        assert_eq!(pulse.saved(), None, "a later tick without a save changes nothing");
        assert!(!pulse.raised());
    }

    #[test]
    fn requests_while_one_is_pending_share_its_edge() {
        let mut pulse = SavePulse::default();
        assert_eq!(pulse.request(1), Some(true));
        assert_eq!(pulse.request(2), None, "a second request waits for the first edge");
        assert_eq!(pulse.request(3), None);
        assert_eq!(pulse.saved(), Some(false));
        assert_eq!(pulse.request(3), None, "already answered: nothing new");
    }

    #[test]
    fn a_later_request_after_the_lowering_is_a_new_edge() {
        let mut pulse = SavePulse::default();
        assert_eq!(pulse.request(1), Some(true));
        assert_eq!(pulse.saved(), Some(false));
        assert_eq!(pulse.request(2), Some(true));
        assert_eq!(pulse.saved(), Some(false));
    }

    #[test]
    fn a_save_without_a_request_is_ignored() {
        let mut pulse = SavePulse::default();
        assert_eq!(pulse.saved(), None);
        assert_eq!(pulse.request(1), Some(true));
    }

    #[test]
    fn a_missing_map_file_is_named_in_the_error() {
        let error = load_map(Path::new("/nonexistent/helios/map.eidmap")).expect_err("missing");
        assert!(error.contains("/nonexistent/helios/map.eidmap"), "{error}");
    }

    #[test]
    fn a_map_file_loads_its_bytes() {
        let dir = tempfile::tempdir().expect("a directory");
        let path = dir.path().join("map.eidmap");
        std::fs::write(&path, b"EIDMAP\0\0").expect("write");
        let map = load_map(&path).expect("load");
        assert_eq!(map.bytes, b"EIDMAP\0\0");
    }
}
