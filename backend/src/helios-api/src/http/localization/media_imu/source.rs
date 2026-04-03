use std::path::Path;

use crate::http::AppState;
use helios_engine::ipc::StreamSummary;
use helios_engine::localization::types::{LocalizationPipelineSource, LocalizationSourceKind};

use super::binding::{MEDIA_IMU_OUTPUT_KEY, parse_media_imu_stream_id, resolve_binding_for_stream};

pub(crate) fn is_media_imu_source_id(source_id: &str) -> bool {
    parse_media_imu_stream_id(source_id).is_some()
}

pub(crate) async fn source_for_stream(state: &AppState, stream: &StreamSummary, media_meta_dir: &Path) -> Option<LocalizationPipelineSource> {
    let binding = resolve_binding_for_stream(state, stream, media_meta_dir).await?;
    let stream_id = binding.stream_id.to_string();
    let camera_uid = stream.manifest.identity.hardware_id.clone().or(stream.manifest.identity.alias.clone()).unwrap_or_else(|| stream_id.clone());
    let camera_path = match &stream.manifest.capture.handle {
        styx::BackendHandle::V4l2 { path } => path.clone(),
        styx::BackendHandle::Libcamera { id } => id.clone(),
        styx::BackendHandle::Netcam { url, .. } => url.clone(),
        styx::BackendHandle::File { paths, .. } => paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(","),
        other => format!("{other:?}"),
    };
    Some(LocalizationPipelineSource {
        id: format!("{stream_id}:{MEDIA_IMU_OUTPUT_KEY}"),
        stream_id: stream_id.clone(),
        stream_label: binding.stream_label,
        camera_uid,
        camera_path,
        pipeline_id: "media-imu".to_string(),
        pipeline_label: "Media IMU".to_string(),
        output_key: MEDIA_IMU_OUTPUT_KEY.to_string(),
        localization_kind: LocalizationSourceKind::Imu,
        data_type: None,
    })
}
