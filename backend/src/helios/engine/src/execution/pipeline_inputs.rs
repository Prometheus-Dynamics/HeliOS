//! Graph inputs fed by other pipelines (`source: "pipeline"` bindings).
//!
//! A consumer pipeline names a producer's host output as one of its graph's inputs. The input is
//! a held host input (like a camera's context), so pushing it never ticks the graph: the consumer
//! still ticks on its own camera frames, and each tick sees the latest value.
//!
//! Before a frame ticks the graph, the consumer syncs its pipeline inputs from the bus:
//!
//! - **Pairing by frame time.** Both pipelines read the same camera, so a value is for the frame
//!   with the same timestamp (`frame.meta().timestamp`, nanoseconds). A frame waits, at most
//!   `max_wait_ms`, while a producer is still on an older frame (the producer's latest tick has an
//!   earlier frame timestamp), so the producer's value for this frame can land first. After that,
//!   or when the producer is already past the frame (it skipped this one), the graph ticks with the
//!   producer's latest value and the tick is `lagged` (with the lag in `lag_ms`). The waiting
//!   thread is the consumer's own; the producer never waits on the consumer.
//! - **Push on change.** A value is pushed only when its producer run and tick changed. A producer
//!   that has not published, or that was stopped or removed, is `missing`; the held input keeps its
//!   last value (a held input cannot be emptied), and the telemetry says so.
//!
//! The waiting frame is the only state a feed adds to the poll loop (`execution::driver`).

use std::time::Duration;

use daedalus::{runtime::host_bridge::HostBridgeHandle, transport::FeedOutcome};

use super::pipeline_bus::PipelineBus;
use crate::model::ExecutionBinding;

/// How a pipeline input stands at the latest tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeedState {
    /// The producer has no value on the bus (not started, stopped or removed).
    Missing,
    /// The value is the producer's output for this frame.
    Aligned,
    /// The value is the producer's output for an earlier frame (or it is ahead of this one).
    Lagged,
}

impl FeedState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Aligned => "aligned",
            Self::Lagged => "lagged",
        }
    }
}

/// One pipeline input's state, for the workload's telemetry.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PipelineInputStats {
    pub input: String,
    pub pipeline: String,
    pub port: String,
    pub state: FeedState,
    /// The producer tick whose value is held.
    pub sequence: Option<u64>,
    /// The frame timestamp gap to the value's frame, in milliseconds (positive: the value is older).
    pub lag_ms: Option<f64>,
    pub aligned_ticks: u64,
    pub lagged_ticks: u64,
    pub missing_ticks: u64,
}

impl PipelineInputStats {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "input": self.input,
            "pipeline": self.pipeline,
            "port": self.port,
            "state": self.state.as_str(),
            "sequence": self.sequence,
            "lag_ms": self.lag_ms.map(|lag| (lag * 100.0).round() / 100.0),
            "aligned_ticks": self.aligned_ticks,
            "lagged_ticks": self.lagged_ticks,
            "missing_ticks": self.missing_ticks,
        })
    }
}

struct Feed {
    input: String,
    pipeline: String,
    port: String,
    /// The (run, tick) of the value last pushed into the input.
    pushed: Option<(u64, u64)>,
    stats: PipelineInputStats,
}

/// The pipeline inputs of one workload.
#[derive(Default)]
pub(crate) struct PipelineFeeds {
    bus: PipelineBus,
    feeds: Vec<Feed>,
    max_wait: Duration,
}

impl PipelineFeeds {
    /// The feeds of `bindings` (the pipeline bindings among them, in input order).
    pub fn new(bus: PipelineBus, bindings: &[ExecutionBinding]) -> Self {
        let mut max_wait_ms = 0;
        let feeds = bindings
            .iter()
            .filter_map(|binding| binding.pipeline.as_ref().map(|source| (binding, source)))
            .map(|(binding, source)| {
                max_wait_ms = max_wait_ms.max(source.max_wait_ms);
                Feed {
                    input: binding.input.clone(),
                    pipeline: source.pipeline.clone(),
                    port: source.port.clone(),
                    pushed: None,
                    stats: PipelineInputStats {
                        input: binding.input.clone(),
                        pipeline: source.pipeline.clone(),
                        port: source.port.clone(),
                        state: FeedState::Missing,
                        sequence: None,
                        lag_ms: None,
                        aligned_ticks: 0,
                        lagged_ticks: 0,
                        missing_ticks: 0,
                    },
                }
            })
            .collect();
        Self { bus, feeds, max_wait: Duration::from_millis(u64::from(max_wait_ms)) }
    }

    pub fn is_empty(&self) -> bool {
        self.feeds.is_empty()
    }

    /// How long a frame may wait for the producers (the longest of the inputs' `max_wait_ms`).
    pub fn max_wait(&self) -> Duration {
        self.max_wait
    }

    /// Whether a frame stamped `frame_ts_ns` may tick now: no producer is still on an older frame,
    /// or the frame has waited `max_wait` (`waited_out`).
    pub fn ready(&self, frame_ts_ns: u64, waited_out: bool) -> bool {
        waited_out || !self.feeds.iter().any(|feed| self.producer_behind(&feed.pipeline, frame_ts_ns))
    }

    /// A producer is behind a frame when it runs but has not ticked yet, or its latest tick is for
    /// an earlier frame. An absent producer (not started, stopped) is not waited for.
    fn producer_behind(&self, pipeline: &str, frame_ts_ns: u64) -> bool {
        self.bus.progress(pipeline).is_some_and(|progress| progress.sequence == 0 || progress.frame_ts_ns.is_some_and(|ts| ts < frame_ts_ns))
    }

    /// Push each input's value that changed, and record the state of every input for the tick of
    /// the frame `frame_ts_ns`. Returns what the host rejected, one message per input.
    pub fn sync(&mut self, host: &HostBridgeHandle, frame_ts_ns: u64) -> Vec<String> {
        let mut rejected = Vec::new();
        for feed in &mut self.feeds {
            let Some((run, published)) = self.bus.latest(&feed.pipeline, &feed.port) else {
                feed.stats.state = FeedState::Missing;
                feed.stats.sequence = None;
                feed.stats.lag_ms = None;
                feed.stats.missing_ticks += 1;
                continue;
            };
            if feed.pushed != Some((run, published.sequence)) {
                match host.feed_payload(feed.input.clone(), published.payload.clone()) {
                    FeedOutcome::Rejected(reason) => rejected.push(format!("pipeline input {} from {}.{} rejected: {reason:?}", feed.input, feed.pipeline, feed.port)),
                    _ => feed.pushed = Some((run, published.sequence)),
                }
            }
            let aligned = published.frame_ts_ns == Some(frame_ts_ns);
            feed.stats.state = if aligned { FeedState::Aligned } else { FeedState::Lagged };
            feed.stats.sequence = Some(published.sequence);
            feed.stats.lag_ms = published.frame_ts_ns.map(|ts| (frame_ts_ns as f64 - ts as f64) / 1e6);
            if aligned {
                feed.stats.aligned_ticks += 1;
            } else {
                feed.stats.lagged_ticks += 1;
            }
        }
        rejected
    }

    pub fn stats(&self) -> Vec<PipelineInputStats> {
        self.feeds.iter().map(|feed| feed.stats.clone()).collect()
    }
}
