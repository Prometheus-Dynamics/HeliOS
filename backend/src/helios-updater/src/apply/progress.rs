use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::sync::{RwLock, broadcast::Sender};

use crate::ipc::{UpdateStage, UpdaterEvent};
use crate::state::ServiceState;
use crate::util::ProgressUpdate;

use super::{APPLY_PROGRESS_END, APPLY_PROGRESS_START};

pub(super) async fn publish_snapshot(state: &Arc<RwLock<ServiceState>>, events: &Sender<UpdaterEvent>) {
    let snapshot = {
        let guard = state.read().await;
        let (active, cache_usage) = guard.snapshot();
        UpdaterEvent::StateSnapshot { active_update: active, cache_usage_bytes: cache_usage }
    };
    let _ = events.send(snapshot);
}

pub(super) fn start_apply_progress(state: Arc<RwLock<ServiceState>>, events: Sender<UpdaterEvent>) -> (mpsc::Sender<ProgressUpdate>, tokio::task::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(16);
    let handle = tokio::spawn(async move {
        drive_apply_progress(state, events, rx).await;
    });
    (tx, handle)
}

async fn drive_apply_progress(state: Arc<RwLock<ServiceState>>, events: Sender<UpdaterEvent>, mut rx: mpsc::Receiver<ProgressUpdate>) {
    let mut last_percent: Option<u8> = None;
    let mut last_sent = tokio::time::Instant::now().checked_sub(Duration::from_secs(1)).unwrap_or_else(tokio::time::Instant::now);
    while let Some(update) = rx.recv().await {
        let Some(percent) = apply_progress_percent(update.bytes_written, update.total_bytes) else {
            continue;
        };
        if let Some(prev) = last_percent
            && percent <= prev
        {
            continue;
        }
        let now = tokio::time::Instant::now();
        if now.duration_since(last_sent) < Duration::from_millis(300) {
            continue;
        }
        {
            let mut guard = state.write().await;
            guard.update_progress(UpdateStage::Applying, Some(percent), None);
        }
        publish_snapshot(&state, &events).await;
        last_percent = Some(percent);
        last_sent = now;
    }
}

fn apply_progress_percent(bytes_written: u64, total_bytes: Option<u64>) -> Option<u8> {
    let total = total_bytes?;
    if total == 0 {
        return None;
    }
    let ratio = (bytes_written as f64 / total as f64).clamp(0.0, 1.0);
    let range = (APPLY_PROGRESS_END.saturating_sub(APPLY_PROGRESS_START)) as f64;
    let raw = (APPLY_PROGRESS_START as f64) + range * ratio;
    let percent = raw.round().clamp(APPLY_PROGRESS_START as f64, APPLY_PROGRESS_END as f64) as u8;
    Some(percent)
}
