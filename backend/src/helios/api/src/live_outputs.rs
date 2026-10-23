//! Pipeline outputs as they are produced: helios-engine's live socket (`helios_engine::live`,
//! `HELIOS_ENGINE_LIVE_SOCKET`) read while anyone follows the event stream, each line published at
//! once as an `output` event (the same shape the Orion watcher sends, `GET /v1/outputs`'s
//! entries). The engine coalesces to about the display rate, so a 120 fps pipeline's overlay
//! follows its frames instead of Orion's publish interval (250 ms).

use std::{path::PathBuf, sync::atomic::Ordering, time::Duration};

use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::UnixStream,
};

use crate::SharedState;

/// The engine's socket when `HELIOS_ENGINE_LIVE_SOCKET` does not say.
pub const DEFAULT_SOCKET: &str = "/run/helios/engine-live.sock";
/// Wait between connection attempts while the engine is not there.
const RETRY: Duration = Duration::from_secs(1);

pub fn socket_path() -> PathBuf {
    std::env::var_os("HELIOS_ENGINE_LIVE_SOCKET").map_or_else(|| PathBuf::from(DEFAULT_SOCKET), PathBuf::from)
}

/// Follow the live outputs while the event stream has subscribers.
pub fn spawn(state: SharedState, socket: PathBuf) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            state.events.wait_for_subscriber().await;
            match UnixStream::connect(&socket).await {
                Ok(stream) => {
                    state.events.live_outputs.store(true, Ordering::Release);
                    follow(&state, stream).await;
                    state.events.live_outputs.store(false, Ordering::Release);
                }
                Err(_) => tokio::time::sleep(RETRY).await,
            }
        }
    })
}

/// Publish each line until the engine closes the socket or nobody is subscribed any more.
async fn follow(state: &SharedState, stream: UnixStream) {
    let mut lines = BufReader::with_capacity(64 * 1024, stream).lines();
    let mut idle = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(line)) => {
                    if let Some((id, value)) = output_event(&line) {
                        state.events.publish("output", serde_json::json!({ "id": id, "change": "updated", "value": value }));
                    }
                }
                Ok(None) | Err(_) => return,
            },
            // Disconnecting with nobody watching stops the engine serializing for no one.
            _ = idle.tick() => if state.events.subscribers() == 0 { return },
        }
    }
}

/// A live line as the `output` event's id (`<pipeline>/<port>`) and value.
fn output_event(line: &str) -> Option<(String, serde_json::Value)> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let id = format!("{}/{}", value.get("pipeline")?.as_str()?, value.get("port")?.as_str()?);
    Some((id, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_becomes_an_output_event_with_the_outputs_shape() {
        let (id, value) = output_event(r#"{"pipeline":"front","port":"detections","observed_at_ms":5,"value":{"detections":[]}}"#).expect("event");
        assert_eq!(id, "front/detections");
        assert_eq!(value["value"]["detections"], serde_json::json!([]));
        assert_eq!(value["observed_at_ms"], 5);
        assert!(output_event("not json").is_none());
    }
}
