use helios_engine::capture::CaptureDescriptor;
use helios_engine::ipc::EngineEvent;
use lib_ipc::client::ClientTransportError;
use tokio::time::{Duration, Instant};
use uuid::Uuid;

use super::util::list_streams_timeout;
use crate::http::AppState;

pub(crate) async fn wait_for_stream_started(state: &AppState, stream_id: Uuid, timeout: Duration) -> Result<Option<CaptureDescriptor>, ClientTransportError> {
    let deadline = Instant::now() + timeout;
    let mut events = state.engine.subscribe_events();
    let mut ticker = tokio::time::interval(Duration::from_millis(750));

    loop {
        if Instant::now() >= deadline {
            return Ok(None);
        }

        tokio::select! {
            _ = ticker.tick() => {
                let streams = state.engine.list_streams_with_timeout(list_streams_timeout()).await?;
                if let Some(found) = streams.into_iter().find(|s| s.stream_id == stream_id) {
                    return Ok(Some(found.descriptor));
                }
            }
            recv = events.recv() => {
                match recv {
                    Ok(EngineEvent::Started { stream_id: sid, descriptor, .. }) if sid == stream_id => return Ok(Some(descriptor)),
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {}
                }
            }
        }
    }
}

pub(crate) async fn wait_for_stream_gone(state: &AppState, stream_id: Uuid, timeout: Duration) -> Result<bool, ClientTransportError> {
    let deadline = Instant::now() + timeout;
    let mut events = state.engine.subscribe_events();
    let mut ticker = tokio::time::interval(Duration::from_millis(750));

    loop {
        if Instant::now() >= deadline {
            return Ok(false);
        }

        tokio::select! {
            _ = ticker.tick() => {
                let streams = state.engine.list_streams_with_timeout(list_streams_timeout()).await?;
                if !streams.iter().any(|s| s.stream_id == stream_id) {
                    return Ok(true);
                }
            }
            recv = events.recv() => {
                match recv {
                    Ok(EngineEvent::Stopped { stream_id: sid, .. }) if sid == stream_id => return Ok(true),
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {}
                }
            }
        }
    }
}
