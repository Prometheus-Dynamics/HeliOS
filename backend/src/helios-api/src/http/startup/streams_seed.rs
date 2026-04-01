use std::collections::HashSet;
use tracing::warn;

use super::StartupStreamPreset;
use crate::http::streams_persist::{self, PersistedStreamReconcileStatus};

pub(super) async fn seed_streams(presets: &[StartupStreamPreset]) -> usize {
    let mut seeded = 0usize;
    let mut seen_camera_ids: HashSet<String> = HashSet::new();

    for preset in presets {
        let camera_id = preset.camera_id.trim();
        if camera_id.is_empty() {
            warn!("startup stream preset has empty camera_id; skipping");
            continue;
        }

        if !seen_camera_ids.insert(camera_id.to_string()) {
            warn!(camera_id, "startup stream preset has duplicate camera_id; later entry will overwrite earlier stream record");
        }

        let mut manifest = preset.manifest.clone();
        manifest.internal = false;
        if let Some(stream_id) = preset.stream_id {
            manifest.identity.id = Some(stream_id);
        }

        let prepared = match streams_persist::prepare_manifest_for_persistence_checked(camera_id, manifest.identity.id, manifest).await {
            Ok(validated) => {
                if !validated.warnings.is_empty() {
                    warn!(
                        camera_id,
                        warning_count = validated.warnings.len(),
                        warnings = ?validated.warnings,
                        "startup stream preset required sanitization"
                    );
                }
                validated
            }
            Err(err) => {
                warn!(camera_id, error = %err, "startup stream preset failed semantic validation; skipping");
                continue;
            }
        };

        let stream_id = prepared.resolved.identity.id;
        if let Err(err) = streams_persist::persist_reconciled_resolved_config_checked(camera_id, stream_id, prepared.resolved, PersistedStreamReconcileStatus::Ready, None).await {
            warn!(camera_id, error = %err, "failed to persist startup stream preset");
            continue;
        }
        seeded += 1;
    }

    seeded
}
