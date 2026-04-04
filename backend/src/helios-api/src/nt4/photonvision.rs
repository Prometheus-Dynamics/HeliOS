use super::photonvision_packet;
use nt_client::{
    data::{NetworkTableData, RawData},
    subscribe::ReceivedMessage,
    subscribe::SubscriptionOptions,
};
use std::{collections::BTreeSet, time::Duration};
use tokio::time::timeout;

#[derive(Debug, Clone)]
pub struct PhotonvisionCameraSnapshot {
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct PhotonvisionNt4Snapshot {
    pub cameras: Vec<PhotonvisionCameraSnapshot>,
}

pub async fn snapshot(pool: &crate::nt4::pool::Nt4ClientPool, host: &str, port: u16, timeout_ms: u64) -> Result<PhotonvisionNt4Snapshot, String> {
    let settings = crate::http::device::nt4::load_settings().await;
    if !settings.subscriptions_enabled {
        return Err("nt4 subscriptions are disabled in device settings".into());
    }

    let timeout_duration = Duration::from_millis(timeout_ms.clamp(150, 10_000));
    let entry = pool.get_or_connect(host, port, "HeliOS-photonvision").await?;
    if let Err(err) = entry.wait_ready(timeout_duration.min(Duration::from_millis(1200))).await {
        let _ = pool.disconnect(host, port).await;
        return Err(err);
    }

    let topics = entry.list_topics_prefix("/photonvision/", Duration::from_millis(350)).await?;
    let cameras = parse_camera_names(&topics);

    let mut camera_snapshots = Vec::new();
    for camera in cameras {
        camera_snapshots.push(PhotonvisionCameraSnapshot { name: camera });
    }

    Ok(PhotonvisionNt4Snapshot { cameras: camera_snapshots })
}

pub async fn fetch_tag_poses(pool: &crate::nt4::pool::Nt4ClientPool, host: &str, port: u16, camera: &str, timeout_ms: u64) -> Result<serde_json::Value, String> {
    let settings = crate::http::device::nt4::load_settings().await;
    if !settings.subscriptions_enabled {
        return Err("nt4 subscriptions are disabled in device settings".into());
    }

    let timeout_duration = Duration::from_millis(timeout_ms.clamp(200, 10_000));
    let entry = pool.get_or_connect(host, port, "HeliOS-photonvision").await?;
    if let Err(err) = entry.wait_ready(timeout_duration.min(Duration::from_millis(1200))).await {
        let _ = pool.disconnect(host, port).await;
        return Err(err);
    }

    let raw_topic = format!("/photonvision/{camera}/rawBytes");
    let bytes = read_raw(entry.handle(), &raw_topic, timeout_duration.min(Duration::from_millis(900))).await?.ok_or_else(|| "no photonvision result received yet".to_string())?;

    let parsed = photonvision_packet::decode_photon_pipeline_result(&bytes)?;

    let latency_ms = (parsed.metadata.publish_timestamp_micros - parsed.metadata.capture_timestamp_micros) as f64 / 1e3;

    let detections = parsed
        .targets
        .into_iter()
        .filter(|target| target.fiducial_id >= 0)
        .map(|target| {
            let (tx, ty, tz) = target.best_camera_to_target.translation;
            // Heuristic mapping into Three.js camera basis:
            // PhotonVision/WPILib geometry is generally X forward, Y left, Z up.
            // Three.js camera looks down -Z, with +X right and +Y up.
            let translation = serde_json::json!({ "x": -ty, "y": tz, "z": -tx });
            let rotation = serde_json::json!({ "yaw": target.yaw, "pitch": target.pitch });
            serde_json::json!({
                "id": target.fiducial_id,
                "translation": translation,
                "rotation": rotation,
                "area": target.area,
                "skew": target.skew,
                "poseAmbiguity": target.pose_ambiguity,
            })
        })
        .collect::<Vec<_>>();

    Ok(serde_json::json!({
        "detections": detections,
        "stats": {
            "source": "photonvision",
            "host": host,
            "port": port,
            "camera": camera,
            "sequenceId": parsed.metadata.sequence_id,
            "latencyMs": latency_ms,
        }
    }))
}

fn parse_camera_names(topics: &[String]) -> Vec<String> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for name in topics {
        let trimmed = name.trim_matches('/');
        let mut parts = trimmed.split('/');
        let Some(root) = parts.next() else { continue };
        if root != "photonvision" {
            continue;
        }
        let Some(camera) = parts.next() else { continue };
        let has_leaf = parts.next().is_some();
        if !has_leaf {
            continue;
        }
        if camera == ".schema" {
            continue;
        }
        names.insert(camera.to_string());
    }
    names.into_iter().collect()
}

async fn read_raw(handle: &nt_client::ClientHandle, topic_name: &str, timeout_duration: Duration) -> Result<Option<Vec<u8>>, String> {
    let options = SubscriptionOptions { all: Some(true), periodic: Some(Duration::from_millis(50)), ..Default::default() };

    let topic = handle.topic(topic_name.to_string());
    let mut subscriber = topic.subscribe(options).await.map_err(|e| e.to_string())?;

    match timeout(timeout_duration, async {
        loop {
            match subscriber.recv().await {
                Ok(ReceivedMessage::Updated((_topic, value))) => {
                    let raw = RawData::from_value(value).map(|raw| raw.0);
                    return Ok(raw);
                }
                Ok(_) => {}
                Err(err) => return Err(err.to_string()),
            }
        }
    })
    .await
    {
        Ok(inner) => inner,
        Err(_) => Ok(None),
    }
}
