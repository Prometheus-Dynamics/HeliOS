use crate::http::AppState;
use axum::body::to_bytes;
use helios_engine::capture::CaptureControlValue;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc, watch};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct PendingControlUpdate {
    request_id: Option<String>,
    value: CaptureControlValue,
}

#[derive(Debug, Clone)]
pub(crate) struct StreamControlOutcome {
    pub request_id: Option<String>,
    pub result: Result<(), String>,
}

pub(crate) struct StreamControlsWorker {
    pending_controls: Arc<Mutex<BTreeMap<u32, Vec<PendingControlUpdate>>>>,
    responses_rx: mpsc::UnboundedReceiver<StreamControlOutcome>,
    shutdown_tx: watch::Sender<bool>,
}

pub(crate) fn spawn_stream_controls_worker(state: AppState, stream_id: Uuid, apply_interval: Duration) -> StreamControlsWorker {
    let pending_controls = Arc::new(Mutex::new(BTreeMap::<u32, Vec<PendingControlUpdate>>::new()));
    let (responses_tx, responses_rx) = mpsc::unbounded_channel::<StreamControlOutcome>();
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    let worker_state = state.clone();
    let worker_pending_controls = Arc::clone(&pending_controls);
    tokio::spawn(async move {
        let mut apply_tick = tokio::time::interval(apply_interval);
        apply_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = apply_tick.tick() => {
                    let latest = {
                        let mut guard = worker_pending_controls.lock().await;
                        if guard.is_empty() {
                            continue;
                        }
                        std::mem::take(&mut *guard)
                    };

                    for (apply_id, apply_value) in latest {
                        let Some(latest_update) = apply_value.last().cloned() else {
                            continue;
                        };
                        let result = control_apply_result(
                            crate::http::streams::controls::set_control(
                                worker_state.clone(),
                                stream_id,
                                apply_id,
                                latest_update.value,
                            )
                            .await,
                        )
                        .await;
                        if result.is_ok() {
                            worker_state.publish_realtime_update(
                                crate::ipc::RealtimeUpdateOrigin::Ws,
                                crate::ipc::RealtimeUpdateKind::StreamsControls,
                                format!("/v1/ws/streams/{stream_id}/controls"),
                                Some("set_control".to_string()),
                                latest_update.request_id.clone(),
                            );
                        }
                        for pending in apply_value {
                            if responses_tx
                                .send(StreamControlOutcome {
                                    request_id: pending.request_id.clone(),
                                    result: result.clone(),
                                })
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                }
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                }
            }
        }
    });

    StreamControlsWorker { pending_controls, responses_rx, shutdown_tx }
}

impl StreamControlsWorker {
    pub(crate) async fn enqueue(&self, control_id: u32, request_id: Option<String>, value: CaptureControlValue) {
        let mut guard = self.pending_controls.lock().await;
        guard.entry(control_id).or_default().push(PendingControlUpdate { request_id, value });
    }

    pub(crate) async fn next_result(&mut self) -> Option<StreamControlOutcome> {
        self.responses_rx.recv().await
    }

    pub(crate) fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

async fn control_apply_result(response: axum::response::Response) -> Result<(), String> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = to_bytes(response.into_body(), 64 * 1024).await.ok();
    let detail = body
        .as_deref()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(bytes).ok())
        .and_then(|json| json.get("error").and_then(|value| value.as_str()).map(str::to_string))
        .unwrap_or_else(|| format!("control update failed ({status})"));
    Err(detail)
}
