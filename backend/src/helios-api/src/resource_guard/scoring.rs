use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use helios_engine::ipc::{EngineEvent, StreamSummary};
use helios_engine::stream::{CodecMetrics, PipelineGraphMetrics, PipelineNodeRuntimeMetrics, StreamMetrics};
use uuid::Uuid;

use crate::ipc::IpcHandles;

use super::{DegradedStream, ReliefAction, ResourceGuardStage};

fn decoder_available(stream: &StreamSummary) -> bool {
    stream.runtime.codecs.decoder_impl.is_some() || stream.manifest.decoder_id().is_some()
}

fn encoder_available(stream: &StreamSummary) -> bool {
    stream.runtime.codecs.encoder_impl.is_some() || stream.manifest.encoder_id().is_some()
}

fn pipeline_enabled(stream: &StreamSummary) -> bool {
    stream.runtime.pipeline.enabled || stream.manifest.pipeline_enabled
}

fn pipeline_count(stream: &StreamSummary) -> usize {
    let runtime_count = stream.runtime.pipeline.pipeline_count as usize;
    if runtime_count > 0 { runtime_count } else { stream.manifest.pipelines.len() }
}

fn recording_active(stream: &StreamSummary) -> bool {
    stream.runtime.recording.state == helios_engine::ipc::StreamRecordingState::Active || stream.status.recording_active
}

pub(super) fn relief_action_for_stream(stream: &StreamSummary, degraded: &HashMap<Uuid, DegradedStream>, allow_stop_fallback: bool) -> Option<ReliefAction> {
    match degraded.get(&stream.stream_id).map(|state| state.stage) {
        None => {
            if decoder_available(stream) {
                Some(ReliefAction::DisableDecoder)
            } else if encoder_available(stream) {
                Some(ReliefAction::DisableAllCodecs)
            } else if allow_stop_fallback {
                Some(ReliefAction::StopStream)
            } else {
                None
            }
        }
        Some(ResourceGuardStage::DecoderDisabled) => {
            if encoder_available(stream) {
                Some(ReliefAction::DisableAllCodecs)
            } else if allow_stop_fallback {
                Some(ReliefAction::StopStream)
            } else {
                None
            }
        }
        Some(ResourceGuardStage::CodecsDisabled) => allow_stop_fallback.then_some(ReliefAction::StopStream),
    }
}

pub(super) fn rough_stream_score(stream: &StreamSummary) -> f64 {
    let mut score = 0.0;
    if stream.manifest.capture.backend == styx::BackendKind::File {
        score += 140.0;
    }
    if decoder_available(stream) {
        score += 220.0;
    }
    if encoder_available(stream) {
        score += 140.0;
    }
    if pipeline_enabled(stream) {
        score += 180.0;
    }
    score += pipeline_count(stream) as f64 * 45.0;
    score += stream.manifest.host_buffer as f64 * 8.0;
    if recording_active(stream) {
        score += 80.0;
    }
    score
}

pub(super) async fn runtime_stream_score(handles: &Arc<IpcHandles>, stream_id: Uuid, timeout_ms: u64) -> Option<f64> {
    let response = tokio::time::timeout(Duration::from_millis(timeout_ms), handles.engine.get_metrics(stream_id)).await.ok()?;
    let event = response.ok()?;
    match event {
        EngineEvent::Metrics { metrics, .. } => Some(compute_runtime_score(&metrics)),
        EngineEvent::Nack { .. } => None,
        _ => None,
    }
}

fn compute_runtime_score(metrics: &StreamMetrics) -> f64 {
    let decoder = metrics.decoder.as_ref().map(codec_score).unwrap_or(0.0);
    let encoder = metrics.encoder.as_ref().map(codec_score).unwrap_or(0.0);

    let pipeline_primary = metrics.pipeline.as_ref().map(pipeline_score).unwrap_or(0.0);
    let pipeline_instances = if let Some(maps) = metrics.pipeline_instances.as_ref() { maps.values().map(pipeline_score).sum::<f64>() } else { 0.0 };

    let host_work_ms = if metrics.host.work_average_time_ms > 0.0 { metrics.host.work_average_time_ms } else { metrics.host.last_time_ms.max(metrics.host.average_time_ms) };
    let host_load = host_work_ms.max(0.0) * metrics.host.fps.max(0.0) * 0.05;

    decoder + encoder + pipeline_primary + pipeline_instances + host_load
}

fn codec_score(metrics: &CodecMetrics) -> f64 {
    let work_ms = if metrics.work_average_time_ms > 0.0 { metrics.work_average_time_ms } else { metrics.average_time_ms };
    work_ms.max(0.0) * metrics.fps.max(0.0)
}

fn pipeline_score(metrics: &PipelineGraphMetrics) -> f64 {
    let nodes = metrics.nodes.values().map(pipeline_node_score).sum::<f64>();
    let groups = if let Some(groups) = metrics.groups.as_ref() { groups.values().map(pipeline_node_score).sum::<f64>() } else { 0.0 };
    nodes + groups
}

fn pipeline_node_score(node: &PipelineNodeRuntimeMetrics) -> f64 {
    let base = node.metrics.average_time_ms.max(0.0) * node.metrics.average_fps.max(0.0);
    let children = if let Some(children) = node.children.as_ref() { children.values().map(pipeline_node_score).sum::<f64>() } else { 0.0 };
    base + children
}
