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
    pub memory: Option<StreamMemoryMetrics>,
    #[serde(default)]
    pub pipeline: Option<PipelineGraphMetrics>,
    #[serde(default)]
    pub pipeline_instances: Option<BTreeMap<String, PipelineGraphMetrics>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct StreamMemoryMetrics {
    #[serde(default)]
    pub process: Option<StreamProcessMemoryMetrics>,
    #[serde(default)]
    pub runner: Option<StreamRunnerMemoryMetrics>,
    #[serde(default)]
    pub capture_queue: Option<StreamQueueMemoryMetrics>,
    #[serde(default)]
    pub external_backings: Vec<StreamExternalBackingMetrics>,
    #[serde(default)]
    pub transform_pool: Option<StreamBufferPoolMetrics>,
    #[serde(default)]
    pub image_pool: Option<StreamBufferPoolMetrics>,
    #[serde(default)]
    pub packed_pools: Vec<StreamPackedPoolMetrics>,
    #[serde(default)]
    pub staging_copy: Option<StreamStagingCopyMetrics>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct StreamQueueMemoryMetrics {
    #[serde(default)]
    pub depth: u64,
    #[serde(default)]
    pub capacity: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct StreamExternalBackingMetrics {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub current_buffers: u64,
    #[serde(default)]
    pub current_bytes: u64,
    #[serde(default)]
    pub peak_buffers: u64,
    #[serde(default)]
    pub peak_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct StreamStagingCopyMetrics {
    #[serde(default)]
    pub copies: u64,
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub peak_copy_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct StreamProcessMemoryMetrics {
    #[serde(default)]
    pub sampled_at_ms: u64,
    #[serde(default)]
    pub rss_bytes: u64,
    #[serde(default)]
    pub pss_bytes: Option<u64>,
    #[serde(default)]
    pub rss_anon_bytes: u64,
    #[serde(default)]
    pub rss_file_bytes: u64,
    #[serde(default)]
    pub rss_shmem_bytes: u64,
    #[serde(default)]
    pub vm_data_bytes: u64,
    #[serde(default)]
    pub swap_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct StreamRunnerMemoryMetrics {
    #[serde(default)]
    pub current_decoded_frame_bytes: u64,
    #[serde(default)]
    pub peak_decoded_frame_bytes: u64,
    #[serde(default)]
    pub current_raw_clone_bytes: u64,
    #[serde(default)]
    pub peak_raw_clone_bytes: u64,
    #[serde(default)]
    pub current_processed_frame_bytes: u64,
    #[serde(default)]
    pub peak_processed_frame_bytes: u64,
    #[serde(default)]
    pub current_frame_working_set_bytes: u64,
    #[serde(default)]
    pub peak_frame_working_set_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct StreamPackedPoolMetrics {
    #[serde(default)]
    pub min_len_bytes: u64,
    pub pool: StreamBufferPoolMetrics,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct StreamBufferPoolMetrics {
    #[serde(default)]
    pub chunk_size_bytes: u64,
    #[serde(default)]
    pub free_buffers: u64,
    #[serde(default)]
    pub free_bytes: u64,
    #[serde(default)]
    pub max_free_buffers: u64,
    #[serde(default)]
    pub retained_buffers: u64,
    #[serde(default)]
    pub retained_bytes: u64,
    #[serde(default)]
    pub in_use_buffers: u64,
    #[serde(default)]
    pub in_use_bytes: u64,
    #[serde(default)]
    pub peak_in_use_buffers: u64,
    #[serde(default)]
    pub peak_in_use_bytes: u64,
    #[serde(default)]
    pub hits: u64,
    #[serde(default)]
    pub misses: u64,
    #[serde(default)]
    pub allocations: u64,
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
    #[serde(default)]
    pub average_input_payload_bytes: f64,
    #[serde(default)]
    pub average_output_payload_bytes: f64,
    #[serde(default)]
    pub peak_input_payload_bytes: u64,
    #[serde(default)]
    pub peak_output_payload_bytes: u64,
    #[serde(default)]
    pub peak_payload_working_set_bytes: u64,
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
    pub retained_output_sample_count: u64,
    #[serde(default)]
    pub retained_output_sample_bytes: u64,
    #[serde(default)]
    pub retained_output_ports: Option<BTreeMap<String, u64>>,
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
    pub edges: Option<BTreeMap<String, PipelineEdgeRuntimeMetrics>>,
    #[serde(default)]
    pub sample_cache: Option<PipelineSampleCacheMetrics>,
    #[serde(default)]
    pub image_working_set: Option<PipelineImageWorkingSetMetrics>,
    #[serde(default)]
    pub perf: Option<PipelinePerfMetrics>,
    #[serde(default)]
    pub flamegraph: Option<PipelineFlamegraphMetrics>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct PipelineImageWorkingSetMetrics {
    #[serde(default)]
    pub input_image_bytes: u64,
    #[serde(default)]
    pub host_output_image_bytes: u64,
    #[serde(default)]
    pub preview_image_bytes: u64,
    #[serde(default)]
    pub total_materialized_image_bytes: u64,
    #[serde(default)]
    pub peak_total_materialized_image_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct PipelineSampleCacheMetrics {
    #[serde(default)]
    pub image_sample_count: u64,
    #[serde(default)]
    pub image_sample_bytes: u64,
    #[serde(default)]
    pub json_sample_count: u64,
    #[serde(default)]
    pub json_sample_bytes: u64,
    #[serde(default)]
    pub value_sample_count: u64,
    #[serde(default)]
    pub value_sample_bytes: u64,
    #[serde(default)]
    pub image_ports: BTreeMap<String, u64>,
    #[serde(default)]
    pub json_ports: BTreeMap<String, u64>,
    #[serde(default)]
    pub value_ports: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct PipelineEdgeRuntimeMetrics {
    #[serde(default)]
    pub average_wait_ms: f64,
    #[serde(default)]
    pub wait_sample_count: u64,
    #[serde(default)]
    pub window_size: u64,
    #[serde(default)]
    pub last_sample_age_ms: Option<u64>,
    #[serde(default)]
    pub max_depth: u64,
    #[serde(default)]
    pub current_depth: u64,
    #[serde(default)]
    pub current_queue_bytes: u64,
    #[serde(default)]
    pub peak_queue_bytes: u64,
    #[serde(default)]
    pub capacity: Option<u64>,
    #[serde(default)]
    pub dropped: u64,
    #[serde(default)]
    pub payload_bytes: u64,
    #[serde(default)]
    pub payload_count: u64,
    #[serde(default)]
    pub average_payload_bytes: f64,
    #[serde(default)]
    pub gpu_uploads: u64,
    #[serde(default)]
    pub gpu_downloads: u64,
    #[serde(default)]
    pub edge_index: u64,
    #[serde(default)]
    pub from_node_index: Option<u64>,
    #[serde(default)]
    pub from_node_label: Option<String>,
    #[serde(default)]
    pub from_port: Option<String>,
    #[serde(default)]
    pub to_node_index: Option<u64>,
    #[serde(default)]
    pub to_node_label: Option<String>,
    #[serde(default)]
    pub to_port: Option<String>,
    #[serde(default)]
    pub policy: Option<String>,
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
