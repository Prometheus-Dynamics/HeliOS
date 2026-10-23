//! The in-process bus of pipeline outputs.
//!
//! A producing pipeline's driver publishes every tick's host outputs here, as the shared
//! `Payload`s it already hands to NetworkTables and the live socket (no copy). A consuming
//! pipeline's graph takes one as an input (`source: "pipeline"`, see `pipeline_inputs`). Both
//! sides hold the bus's lock only for a map insert or an `Arc` clone, so neither waits on the
//! other for longer than that.
//!
//! Each pipeline has a slot: the run that produced it (a new driver is a new run and replaces the
//! slot, so a restarted producer's old values are gone), its progress (the frame timestamp and the
//! sequence of its latest tick, whichever outputs that tick had), and the latest value of each of
//! its ports with the frame it came from.

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
};

use daedalus::transport::Payload;

static NEXT_RUN: AtomicU64 = AtomicU64::new(1);

/// A new run id for a driver: unique in this process, so a restart is always visible on the bus.
pub(crate) fn next_run_id() -> u64 {
    NEXT_RUN.fetch_add(1, Ordering::Relaxed)
}

/// The latest value of one producer port.
#[derive(Clone)]
pub(crate) struct PublishedPort {
    /// The producer's tick that published it.
    pub sequence: u64,
    /// The frame timestamp of that tick (`None` for a tick without a frame).
    pub frame_ts_ns: Option<u64>,
    pub payload: Payload,
}

/// What a producer's latest tick was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Progress {
    pub run: u64,
    pub sequence: u64,
    pub frame_ts_ns: Option<u64>,
}

#[derive(Default)]
struct Slot {
    run: u64,
    sequence: u64,
    frame_ts_ns: Option<u64>,
    ports: BTreeMap<String, PublishedPort>,
}

/// The pipelines' latest outputs, by pipeline id. Cloning shares the bus.
#[derive(Clone, Default)]
pub struct PipelineBus {
    slots: Arc<Mutex<BTreeMap<String, Slot>>>,
}

impl PipelineBus {
    /// Record one tick of `pipeline` (run `run`, frame `frame_ts_ns`) with its payload outputs. A
    /// different run than the slot's replaces the slot.
    pub(crate) fn publish_tick(&self, pipeline: &str, run: u64, frame_ts_ns: Option<u64>, outputs: Vec<(String, Payload)>) {
        let mut slots = lock(&self.slots);
        let slot = slots.entry(pipeline.to_string()).or_default();
        if slot.run != run {
            *slot = Slot { run, ..Slot::default() };
        }
        slot.sequence += 1;
        slot.frame_ts_ns = frame_ts_ns;
        let sequence = slot.sequence;
        for (port, payload) in outputs {
            slot.ports.insert(port, PublishedPort { sequence, frame_ts_ns, payload });
        }
    }

    /// Start `pipeline`'s slot for run `run` before its first tick (sequence 0): consumers see the
    /// producer as running, not absent.
    pub(crate) fn register(&self, pipeline: &str, run: u64) {
        lock(&self.slots).insert(pipeline.to_string(), Slot { run, ..Slot::default() });
    }

    /// Drop `pipeline`'s slot when it is still run `run` (its driver stopped).
    pub(crate) fn withdraw(&self, pipeline: &str, run: u64) {
        let mut slots = lock(&self.slots);
        if slots.get(pipeline).is_some_and(|slot| slot.run == run) {
            slots.remove(pipeline);
        }
    }

    /// The latest tick of `pipeline`, if it has run.
    pub(crate) fn progress(&self, pipeline: &str) -> Option<Progress> {
        lock(&self.slots).get(pipeline).map(|slot| Progress { run: slot.run, sequence: slot.sequence, frame_ts_ns: slot.frame_ts_ns })
    }

    /// The latest value of `pipeline`'s `port`, with the run that published it.
    pub(crate) fn latest(&self, pipeline: &str, port: &str) -> Option<(u64, PublishedPort)> {
        lock(&self.slots).get(pipeline).and_then(|slot| slot.ports.get(port).map(|published| (slot.run, published.clone())))
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
