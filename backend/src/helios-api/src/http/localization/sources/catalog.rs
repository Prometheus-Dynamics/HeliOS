use axum::{Json, extract::State};

use super::profile::build_profile_output_sources;
use super::{IMU_EXTERNAL_ID, PROFILE_STREAM_PREFIX};
use crate::http::error::{ApiError, ApiResult};
use crate::http::{AppState, peers, pipelines};
use helios_engine::ipc::EngineEvent;
use helios_engine::localization::types::{LocalizationPipelineSource, LocalizationSourceKind};
use serde_json::Value as JsonValue;

pub(super) async fn list_sources(State(state): State<AppState>) -> ApiResult<Json<Vec<LocalizationPipelineSource>>> {
    let streams = state.engine.list_streams().await.map_err(|err| ApiError::bad_gateway(err.to_string()))?;
    let media_meta_dir = crate::http::storage::ensure_subdir_async("media-meta").await.ok();

    let mut out: Vec<LocalizationPipelineSource> = Vec::new();
    for stream in streams {
        if stream.manifest.internal {
            continue;
        }
        if let Some(media_meta_dir) = media_meta_dir.as_deref()
            && let Some(media_imu_source) = super::super::media_imu::source_for_stream(&state, &stream, media_meta_dir).await
        {
            out.push(media_imu_source);
        }

        let stream_id = stream.stream_id;
        let outputs = match state.engine.list_graph_outputs_event(stream_id).await {
            Ok(EngineEvent::GraphOutputs { outputs, .. }) => outputs,
            Ok(EngineEvent::Nack { .. }) => Vec::new(),
            Ok(_) => Vec::new(),
            Err(_) => Vec::new(),
        };
        let stream_label = stream.manifest.identity.alias.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| stream_id.to_string());

        let camera_uid = stream.manifest.identity.hardware_id.clone().or(stream.manifest.identity.alias.clone()).unwrap_or_else(|| stream_id.to_string());

        let camera_path = match &stream.manifest.capture.handle {
            styx::BackendHandle::V4l2 { path } => path.clone(),
            styx::BackendHandle::Libcamera { id } => id.clone(),
            styx::BackendHandle::Netcam { url, .. } => url.clone(),
            styx::BackendHandle::File { paths, .. } => paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(","),
            other => format!("{other:?}"),
        };

        let active_pipeline_id = stream.manifest.active_pipeline_id;
        let (pipeline_id, pipeline_label) = if let Some(pipeline_id) = active_pipeline_id {
            let label = match pipelines::load_graph_document(pipeline_id).await {
                Ok(doc) => doc.name.unwrap_or_else(|| pipeline_id.to_string()),
                Err(_) => pipeline_id.to_string(),
            };
            (pipeline_id.to_string(), label)
        } else if let Some(binding) = stream.manifest.pipelines.first() {
            let label = match pipelines::load_graph_document(binding.pipeline_id).await {
                Ok(doc) => doc.name.unwrap_or_else(|| binding.pipeline_id.to_string()),
                Err(_) => binding.pipeline_id.to_string(),
            };
            (binding.pipeline_id.to_string(), label)
        } else {
            ("none".to_string(), "Raw".to_string())
        };

        for desc in outputs {
            let output_key = desc.name;
            let Some(localization_kind) = desc.localization_kind else {
                continue;
            };
            let data_type = desc.ty.map(Into::into);
            out.push(LocalizationPipelineSource {
                id: format!("{stream_id}:{output_key}"),
                stream_id: stream_id.to_string(),
                stream_label: stream_label.clone(),
                camera_uid: camera_uid.clone(),
                camera_path: camera_path.clone(),
                pipeline_id: pipeline_id.clone(),
                pipeline_label: pipeline_label.clone(),
                output_key,
                localization_kind,
                data_type,
            });
        }
    }

    if let Ok(localization_config) = super::super::config::load_config().await {
        let excluded_profile_stream = localization_config.active_profile_id.as_deref().map(str::trim).filter(|id| !id.is_empty()).map(|id| format!("{PROFILE_STREAM_PREFIX}{id}"));

        out.extend(build_profile_output_sources(&localization_config).into_iter().filter(|source| match excluded_profile_stream.as_deref() {
            Some(stream_id) => source.stream_id != stream_id,
            None => true,
        }));
    }

    let peers = peers::snapshot_peers(&state).await;
    for peer in peers {
        out.extend(super::super::peers::sources::list_peer_sources(&peer).await);
    }

    let external_sources = super::super::external::list_external_sources_snapshot(&state).await;
    let has_external_imu = external_sources.iter().any(|source| source.id == IMU_EXTERNAL_ID);
    if !has_external_imu {
        out.push(LocalizationPipelineSource {
            id: "external:imu:imu_pose".to_string(),
            stream_id: "external:imu".to_string(),
            stream_label: "IMU".to_string(),
            camera_uid: "imu".to_string(),
            camera_path: "device:imu".to_string(),
            pipeline_id: "external".to_string(),
            pipeline_label: "IMU".to_string(),
            output_key: "imu_pose".to_string(),
            localization_kind: LocalizationSourceKind::Imu,
            data_type: None,
        });
    }

    for source in external_sources {
        let Some(localization_kind) = localization_kind_from_data_type(source.data_type.as_ref()) else {
            continue;
        };
        let source_id = source.id;
        let output_key = source.output_key;
        out.push(LocalizationPipelineSource {
            id: format!("external:{source_id}:{output_key}"),
            stream_id: format!("external:{source_id}"),
            stream_label: source.label,
            camera_uid: source.camera_uid.unwrap_or_else(|| source_id.clone()),
            camera_path: source.camera_path.unwrap_or_default(),
            pipeline_id: "external".to_string(),
            pipeline_label: source.pipeline_label.unwrap_or_else(|| "External".to_string()),
            output_key,
            localization_kind,
            data_type: source.data_type,
        });
    }

    Ok(Json(out))
}

fn localization_kind_from_data_type(data_type: Option<&JsonValue>) -> Option<LocalizationSourceKind> {
    let kind = data_type.and_then(JsonValue::as_object).and_then(|fields| fields.get("kind")).and_then(JsonValue::as_str)?;

    match kind {
        "localization_detection_pose" => Some(LocalizationSourceKind::Detection),
        "localization_pose" => Some(LocalizationSourceKind::Pose),
        "imu_pose" => Some(LocalizationSourceKind::Imu),
        _ => None,
    }
}
