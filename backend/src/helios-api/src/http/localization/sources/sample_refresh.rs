use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub(super) const LOCALIZATION_STREAM_SAMPLE_REFRESH_MS: u64 = 1_000;

#[derive(Default)]
struct LocalizationStreamSampleRefreshState {
    refreshed_at_ms: BTreeMap<String, u64>,
    refresh_grants: u64,
    throttled_requests: u64,
    pruned_entries: u64,
}

impl LocalizationStreamSampleRefreshState {
    fn needs_refresh(&mut self, stream_id: Uuid, output_key: &str, now_ms: u64) -> bool {
        let key = localization_stream_sample_key(stream_id, output_key);
        let before = self.refreshed_at_ms.len();
        self.refreshed_at_ms.retain(|_, refreshed_at_ms| now_ms.saturating_sub(*refreshed_at_ms) <= LOCALIZATION_STREAM_SAMPLE_REFRESH_MS.saturating_mul(8));
        self.pruned_entries += before.saturating_sub(self.refreshed_at_ms.len()) as u64;

        let needs_refresh = self.refreshed_at_ms.get(&key).copied().map(|refreshed_at_ms| now_ms.saturating_sub(refreshed_at_ms) >= LOCALIZATION_STREAM_SAMPLE_REFRESH_MS).unwrap_or(true);
        if needs_refresh {
            self.refreshed_at_ms.insert(key, now_ms);
            self.refresh_grants += 1;
        } else {
            self.throttled_requests += 1;
        }
        needs_refresh
    }

    fn snapshot(&self) -> crate::api_observability::LocalizationSampleRefreshSnapshot {
        crate::api_observability::LocalizationSampleRefreshSnapshot {
            tracked_outputs: self.refreshed_at_ms.len() as u64,
            refresh_grants: self.refresh_grants,
            throttled_requests: self.throttled_requests,
            pruned_entries: self.pruned_entries,
        }
    }
}

fn localization_stream_sample_key(stream_id: Uuid, output_key: &str) -> String {
    format!("{stream_id}:{}", output_key.trim().to_ascii_lowercase())
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|dur| dur.as_millis() as u64).unwrap_or(0)
}

#[derive(Default)]
pub(crate) struct LocalizationStreamSampleRefreshRuntime {
    state: Mutex<LocalizationStreamSampleRefreshState>,
}

impl LocalizationStreamSampleRefreshRuntime {
    pub(crate) fn needs_refresh(&self, stream_id: Uuid, output_key: &str) -> bool {
        self.state.lock().expect("localization stream sample refresh mutex poisoned").needs_refresh(stream_id, output_key, now_ms())
    }

    pub(crate) fn snapshot(&self) -> crate::api_observability::LocalizationSampleRefreshSnapshot {
        self.state.lock().expect("localization stream sample refresh mutex poisoned").snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::{LOCALIZATION_STREAM_SAMPLE_REFRESH_MS, LocalizationStreamSampleRefreshRuntime, LocalizationStreamSampleRefreshState, localization_stream_sample_key};
    use uuid::Uuid;

    #[test]
    fn repeated_sampling_is_throttled_per_stream_output_key() {
        let mut state = LocalizationStreamSampleRefreshState::default();
        let stream_id = Uuid::nil();

        assert!(state.needs_refresh(stream_id, "tag_poses", 1_000));
        assert!(!state.needs_refresh(stream_id, "TAG_POSES", 1_500));
        assert!(state.needs_refresh(stream_id, "tag_poses", 2_000));
        assert!(state.needs_refresh(stream_id, "other_output", 2_000));
    }

    #[test]
    fn repeated_sampling_prunes_stale_entries() {
        let mut state = LocalizationStreamSampleRefreshState::default();
        let old_stream = Uuid::nil();
        let fresh_stream = Uuid::from_u128(1);

        assert!(state.needs_refresh(old_stream, "tag_poses", 0));
        assert!(state.needs_refresh(fresh_stream, "tag_poses", LOCALIZATION_STREAM_SAMPLE_REFRESH_MS * 9));

        assert!(!state.refreshed_at_ms.contains_key(&localization_stream_sample_key(old_stream, "tag_poses")));
        assert!(state.refreshed_at_ms.contains_key(&localization_stream_sample_key(fresh_stream, "tag_poses")));
    }

    #[test]
    fn runtime_exposes_snapshot_and_throttle_state() {
        let runtime = LocalizationStreamSampleRefreshRuntime::default();
        let stream_id = Uuid::nil();

        assert!(runtime.needs_refresh(stream_id, "tag_poses"));
        assert!(!runtime.needs_refresh(stream_id, "tag_poses"));

        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.refresh_grants, 1);
        assert_eq!(snapshot.throttled_requests, 1);
    }
}
