//! `GET /v1/streams/ws`: the node's live streams to a client over a WebSocket (`live_streams`).
//!
//! Text frames are JSON (`catalog`, `status`, `lagged`); binary frames are samples (see
//! `live_streams::codec` for the layout). Nothing is sent back, and a client that stops reading
//! falls behind and is told so, rather than holding the others up.

use axum::{
    extract::{State, WebSocketUpgrade, ws::{Message, WebSocket}},
    response::Response,
};
use futures_util::StreamExt;
use tokio::sync::broadcast::error::RecvError;

use crate::{
    SharedState,
    live_streams::{LiveMessage, Subscription, lagged_text},
};

pub async fn ws(State(state): State<SharedState>, upgrade: WebSocketUpgrade) -> Response {
    // Subscribed before the upgrade completes, so nothing sent from here on is missed.
    let subscription = state.live_streams.subscribe();
    upgrade.on_upgrade(move |socket| serve(socket, subscription))
}

/// What the loop has to do next: a message for the client, or the client's own frame.
enum Next {
    Message(Result<LiveMessage, RecvError>),
    Incoming(Option<Result<Message, axum::Error>>),
}

async fn serve(mut socket: WebSocket, mut subscription: Subscription) {
    for message in std::mem::take(&mut subscription.initial) {
        if !send(&mut socket, message).await {
            return;
        }
    }
    loop {
        let next = tokio::select! {
            received = subscription.messages.recv() => Next::Message(received),
            incoming = socket.next() => Next::Incoming(incoming),
        };
        match next {
            Next::Message(Ok(message)) => {
                if !send(&mut socket, message).await {
                    return;
                }
            }
            Next::Message(Err(RecvError::Lagged(missed))) => {
                // The frames it dropped are gone; the catalog says what the series are now.
                if !send(&mut socket, LiveMessage::Text(lagged_text(missed).into())).await {
                    return;
                }
                for message in subscription.resync() {
                    if !send(&mut socket, message).await {
                        return;
                    }
                }
            }
            Next::Message(Err(RecvError::Closed)) => return,
            // The client closing, or a broken connection, ends it; anything else it sends is ignored.
            Next::Incoming(None | Some(Err(_)) | Some(Ok(Message::Close(_)))) => return,
            Next::Incoming(Some(Ok(_))) => {}
        }
    }
}

async fn send(socket: &mut WebSocket, message: LiveMessage) -> bool {
    let frame = match message {
        LiveMessage::Text(text) => Message::Text(text.as_ref().into()),
        LiveMessage::Binary(bytes) => Message::Binary(bytes.to_vec().into()),
    };
    socket.send(frame).await.is_ok()
}
