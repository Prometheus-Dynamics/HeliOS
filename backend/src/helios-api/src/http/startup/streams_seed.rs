use std::collections::HashSet;
use tracing::warn;

use super::StartupStreamPreset;
use crate::http::{streams, streams_persist};

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

        let validation = streams::validation::validate_stream_manifest(manifest).await;
        let manifest = match validation {
            Ok(validated) => {
                if !validated.warnings.is_empty() {
                    warn!(
                        camera_id,
                        warning_count = validated.warnings.len(),
                        warnings = ?validated.warnings,
                        "startup stream preset required sanitization"
                    );
                }
                validated.manifest
            }
            Err(err) => {
                warn!(
                    camera_id,
                    issue_count = err.issues.len(),
                    warning_count = err.warnings.len(),
                    issues = ?err.issues,
                    warnings = ?err.warnings,
                    "startup stream preset failed semantic validation; skipping"
                );
                continue;
            }
        };

        let stream_id = manifest.identity.id;
        streams_persist::persist_manifest(camera_id, stream_id, manifest).await;
        seeded += 1;
    }

    seeded
}
