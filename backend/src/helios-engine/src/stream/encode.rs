#[cfg(feature = "runtime")]
use super::StreamRunner;
#[cfg(feature = "runtime")]
use crate::capture::CaptureStageMetrics;
#[cfg(feature = "runtime")]
use crate::error::{Error, Result};
#[cfg(feature = "runtime")]
use crate::stream::CodecMetrics;
#[cfg(feature = "runtime")]
use std::sync::OnceLock;
#[cfg(feature = "runtime")]
use styx::codec::{CodecPolicy, CodecRegistry, CodecRegistryHandle};
#[cfg(feature = "runtime")]
use styx::prelude::{FourCc, StageMetrics};
#[cfg(feature = "runtime")]
use tracing::warn;

#[cfg(feature = "runtime")]
impl StreamRunner {
    pub(super) fn ensure_codecs_for_decode(&mut self) -> Result<CodecRegistryHandle> {
        if self.codecs.is_none() {
            static GLOBAL: OnceLock<CodecRegistryHandle> = OnceLock::new();
            let handle = GLOBAL
                .get_or_init(|| {
                    let registry = CodecRegistry::with_enabled_codecs().expect("codec registry init failed");
                    registry.handle()
                })
                .clone();
            // Apply decoder preference if provided.
            if let Some(impl_name) = self.decoder_id.as_ref() {
                if let Some(fourcc) = self.capture_input_fourcc() {
                    let dec_policy = CodecPolicy::builder(fourcc).ordered_impls([impl_name.clone()]).build();
                    handle.set_policy(dec_policy);
                }
            }
            self.configure_decoder_threads(&handle);
            // Do not dynamically re-register ffmpeg codecs here. The codec registry is global and
            // `register_dynamic()` would accumulate instances across streams/runs.
            self.codecs = Some(handle);
        }
        self.codecs.clone().ok_or(Error::InvalidState("codec registry init failed"))
    }

    fn configure_decoder_threads(&self, _handle: &CodecRegistryHandle) {
        let Some(threads) = self.decoder_settings.as_ref().and_then(|s| s.thread_count).filter(|t| *t > 0) else {
            return;
        };
        warn!(threads, "decoder thread_count requested but dynamic codec reconfiguration is disabled");
    }

    pub(super) fn configure_encoder(&mut self, handle: &CodecRegistryHandle, encode_input: FourCc, target_output: FourCc) -> Result<()> {
        // Avoid `register_dynamic()` here: the codec registry is global and dynamic registration
        // would accumulate instances across streams/runs.
        //
        // Encoder settings (bitrate, gop, etc) are still respected when the chosen codec supports
        // them, but we do not create per-stream ffmpeg instances.
        let _ = (target_output, self.encoder_settings.as_ref());
        if let Some(impl_name) = self.encoder_id.as_ref() {
            let policy = CodecPolicy::builder(encode_input).ordered_impls([impl_name.clone()]).prefer_hardware(true).build();
            handle.set_policy(policy);
        } else {
            let policy = CodecPolicy::builder(encode_input).ordered_impls([String::from("ffmpeg")]).prefer_hardware(true).build();
            handle.set_policy(policy);
        }
        Ok(())
    }
}

#[cfg(feature = "runtime")]
pub(super) fn to_codec_metrics(stats: &styx::codec::CodecStats) -> CodecMetrics {
    // `fps` is stage throughput; `average_time_ms` is cadence derived from throughput so the UI
    // stays internally consistent (avg_ms ~= 1000/fps). Separate work-time fields are provided
    // for actual codec processing duration.
    let fps = stats.fps().unwrap_or(0.0);
    let cadence_ms = if fps > 0.0 { 1000.0 / fps } else { 0.0 };
    let work_avg_ms = stats.avg_millis().unwrap_or(0.0);
    let work_last_ms = stats.last_millis().unwrap_or(0.0);
    CodecMetrics {
        processed: stats.processed(),
        errors: stats.errors(),
        backpressure: stats.backpressure(),
        average_time_ms: cadence_ms,
        fps,
        sample_count: stats.samples(),
        // We don't currently track per-frame cadence precisely; approximate with the average cadence.
        last_time_ms: cadence_ms,
        work_average_time_ms: work_avg_ms,
        work_last_time_ms: work_last_ms,
    }
}

#[cfg(feature = "runtime")]
pub(super) fn stage_to_capture_metrics(stage: &StageMetrics) -> CaptureStageMetrics {
    let last_ms = stage.last_millis().unwrap_or(0.0);
    let fps = stage.fps().unwrap_or(0.0);
    let avg_ms = if fps > 0.0 { 1000.0 / fps } else { 0.0 };
    CaptureStageMetrics { average_time_ms: avg_ms, fps, sample_count: stage.samples(), last_time_ms: last_ms }
}
