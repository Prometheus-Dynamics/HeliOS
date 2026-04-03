use super::*;

pub(super) async fn update_manifest_pose_by_camera_id(camera_id: &str, pose: Option<RigPose>) -> std::io::Result<bool> {
    let path = record_path(camera_id).await?;
    let Some(mut record) = load_record_from_path(path.clone(), Some(camera_id)).await? else {
        return Ok(false);
    };
    if record.camera_id.is_empty() {
        record.camera_id = camera_id.to_string();
    }
    let Some(mut resolved) = record.resolved_config.take() else {
        return Ok(false);
    };
    resolved.pose = pose;
    record.resolved_config = Some(resolved);
    record.updated_at = Some(now_rfc3339());

    json_store::write_json(path, &record.canonicalize_for_write()).await?;
    Ok(true)
}

pub(super) async fn list_pose_map() -> HashMap<String, RigPose> {
    let records = list_persisted_records().await;
    let mut out = HashMap::new();
    for record in records {
        if record.resolved_config.as_ref().map(|resolved| resolved.internal).unwrap_or(false) {
            continue;
        }
        if let Some(pose) = record.resolved_config.and_then(|resolved| resolved.pose) {
            out.insert(record.camera_id.clone(), pose);
        }
    }
    out
}

pub(super) async fn pose_for_stream_id(stream_id: Uuid) -> Option<RigPose> {
    for record in list_persisted_records().await {
        let Some(resolved) = record.resolved_config else {
            continue;
        };
        if resolved.internal {
            continue;
        }
        let matches = resolved.identity.id == Some(stream_id) || record.last_stream_id == Some(stream_id) || derived_stream_id(&record.camera_id) == stream_id;
        if !matches {
            continue;
        }
        return resolved.pose;
    }
    None
}
