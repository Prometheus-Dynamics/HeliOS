use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use utoipa::ToSchema;

use crate::capture::CaptureStageMetrics;

#[cfg(feature = "runtime")]
pub use runner::StreamRunner;
#[cfg(feature = "runtime")]
pub use runner::StreamRunnerConfig;
#[cfg(feature = "runtime")]
pub use shmem::read_latest_frame_async;
#[cfg(feature = "runtime")]
pub use shmem::{cleanup_all_stream_files, cleanup_stream_files};
#[cfg(any(feature = "runtime", feature = "dto"))]
pub use shmem::{
    preview_active_recently, preview_heartbeat_path, read_latest_frame, read_latest_frame_with_header, read_latest_header, shmem_path, touch_stream_preview, touch_stream_viewer,
    viewer_active_recently, viewer_heartbeat_path, ShmemFrameHeader, ShmemWriter,
};

#[cfg(feature = "runtime")]
mod capture;
#[cfg(feature = "runtime")]
mod encode;
#[cfg(feature = "runtime")]
mod encoder_worker;
#[cfg(feature = "runtime")]
mod runner;
#[cfg(any(feature = "runtime", feature = "dto"))]
mod shmem;

#[derive(Debug, Clone)]
pub struct EncodedFrame {
    pub data: Arc<[u8]>,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct StreamMetrics {
    pub capture: CaptureStageMetrics,
    pub host: CaptureStageMetrics,
    #[serde(default)]
    pub encoder: Option<CodecMetrics>,
    #[serde(default)]
    pub decoder: Option<CodecMetrics>,
    #[serde(default)]
    pub pipeline: Option<PipelineGraphMetrics>,
    #[serde(default)]
    pub pipeline_instances: Option<BTreeMap<String, PipelineGraphMetrics>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct PipelineNodeMetrics {
    #[serde(default)]
    pub average_time_ms: f64,
    #[serde(default)]
    pub average_fps: f64,
    #[serde(default)]
    pub sample_count: u64,
    #[serde(default)]
    pub window_size: u64,
    #[serde(default)]
    pub last_sample_age_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct PipelineNodePerfMetrics {
    /// Average cache misses per node call (rolling window).
    #[serde(default)]
    pub average_cache_misses: f64,
    /// Average branch instructions per node call (rolling window).
    #[serde(default)]
    pub average_branch_instructions: f64,
    /// Average branch misses per node call (rolling window).
    #[serde(default)]
    pub average_branch_misses: f64,
    #[serde(default)]
    pub sample_count: u64,
    #[serde(default)]
    pub window_size: u64,
    #[serde(default)]
    pub last_sample_age_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct PipelineNodeRuntimeMetrics {
    pub metrics: PipelineNodeMetrics,
    #[serde(default)]
    pub perf: Option<PipelineNodePerfMetrics>,
    /// Nested metrics for grouped nodes, keyed by node/group id.
    #[schema(no_recursion)]
    #[serde(default)]
    pub children: Option<BTreeMap<String, PipelineNodeRuntimeMetrics>>,
    /// Node type identifier (Daedalus node id), e.g. `cv:contour:contours`.
    #[serde(default)]
    pub node_type: Option<String>,
    /// Optional node label from the planned graph.
    #[serde(default)]
    pub node_label: Option<String>,
    /// Node index within the planned graph (`NodeRef.0`).
    #[serde(default)]
    pub node_index: Option<u64>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_error_at: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct PipelineGraphMetrics {
    #[serde(default)]
    pub nodes: BTreeMap<String, PipelineNodeRuntimeMetrics>,
    /// Optional aggregated node-group metrics (e.g. embedded graphs).
    #[serde(default)]
    pub groups: Option<BTreeMap<String, PipelineNodeRuntimeMetrics>>,
    #[serde(default)]
    pub perf: Option<PipelinePerfMetrics>,
    #[serde(default)]
    pub flamegraph: Option<PipelineFlamegraphMetrics>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct PipelinePerfMetrics {
    /// Average cache misses per graph run (rolling window).
    #[serde(default)]
    pub average_cache_misses: f64,
    /// Average branch instructions per graph run (rolling window).
    #[serde(default)]
    pub average_branch_instructions: f64,
    /// Average branch misses per graph run (rolling window).
    #[serde(default)]
    pub average_branch_misses: f64,
    #[serde(default)]
    pub sample_count: u64,
    #[serde(default)]
    pub window_size: u64,
    #[serde(default)]
    pub last_sample_age_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct PipelineFlamegraphMetrics {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub size_bytes: u64,
    /// Unix timestamp (ms) when the flamegraph was captured.
    #[serde(default)]
    pub captured_at_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct CodecMetrics {
    #[serde(default)]
    pub processed: u64,
    #[serde(default)]
    pub errors: u64,
    #[serde(default)]
    pub backpressure: u64,
    /// Average inter-frame time at this stage (derived from throughput).
    #[serde(default)]
    pub average_time_ms: f64,
    /// Average throughput at this stage.
    #[serde(default)]
    pub fps: f64,
    #[serde(default)]
    pub sample_count: u64,
    /// Last observed inter-frame time at this stage (may be 0 if unknown).
    #[serde(default)]
    pub last_time_ms: f64,
    /// Average processing time spent inside this stage (work time), independent of downstream waits.
    #[serde(default)]
    pub work_average_time_ms: f64,
    /// Last processing time spent inside this stage (work time), independent of downstream waits.
    #[serde(default)]
    pub work_last_time_ms: f64,
}
