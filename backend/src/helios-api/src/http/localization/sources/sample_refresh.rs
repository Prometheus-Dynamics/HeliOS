use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub(super) const LOCALIZATION_STREAM_SAMPLE_REFRESH_MS: u64 = 1_000;

#[derive(Default)]
struct LocalizationStreamSampleRefreshState {
    refreshed_at_ms: BTreeMap<String, u64>,
}

impl LocalizationStreamSampleRefreshState {
    fn needs_refresh(&mut self, stream_id: Uuid, output_key: &str, now_ms: u64) -> bool {
        let key = localization_stream_sample_key(stream_id, output_key);
        self.refreshed_at_ms.retain(|_, refreshed_at_ms| now_ms.saturating_sub(*refreshed_at_ms) <= LOCALIZATION_STREAM_SAMPLE_REFRESH_MS.saturating_mul(8));

        let needs_refresh = self.refreshed_at_ms.get(&key).copied().map(|refreshed_at_ms| now_ms.saturating_sub(refreshed_at_ms) >= LOCALIZATION_STREAM_SAMPLE_REFRESH_MS).unwrap_or(true);
        if needs_refresh {
            self.refreshed_at_ms.insert(key, now_ms);
        }
        needs_refresh
    }
}

fn localization_stream_sample_refreshes() -> &'static Mutex<LocalizationStreamSampleRefreshState> {
    static STATE: OnceLock<Mutex<LocalizationStreamSampleRefreshState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(LocalizationStreamSampleRefreshState::default()))
}

fn localization_stream_sample_key(stream_id: Uuid, output_key: &str) -> String {
    format!("{stream_id}:{}", output_key.trim().to_ascii_lowercase())
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|dur| dur.as_millis() as u64).unwrap_or(0)
}

pub(super) fn localization_stream_sample_needs_refresh(stream_id: Uuid, output_key: &str) -> bool {
    localization_stream_sample_refreshes().lock().expect("localization stream sample refresh mutex poisoned").needs_refresh(stream_id, output_key, now_ms())
}

#[cfg(test)]
mod tests {
    use super::{LOCALIZATION_STREAM_SAMPLE_REFRESH_MS, LocalizationStreamSampleRefreshState, localization_stream_sample_key};
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
}
