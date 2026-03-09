use crate::{
    console_protocol::{ConsoleClientEvent, ConsoleServerEvent},
    console_sessions::CONSOLE_SESSIONS,
    http::AppState,
};
use axum::{
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use lib_asyncapi::registry::SchemaRegistry;
use lib_asyncapi::{SchemaProvider, Server, Tag, TypeSchema, WsDoc};
use serde::Deserialize;
use tracing::{debug, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct ConsoleParams {
    pub cols: Option<u16>,
    pub rows: Option<u16>,
    #[serde(rename = "sessionId")]
    pub session_id: Option<Uuid>,
}

pub async fn console_upgrade(ws: WebSocketUpgrade, State(_state): State<AppState>, Query(params): Query<ConsoleParams>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        if let Err(err) = handle_console_socket(socket, params).await {
            warn!(error = %err, "console session terminated early");
        }
    })
}

#[derive(Debug, thiserror::Error)]
enum ConsoleSocketError {
    #[error("failed to serialize event: {0}")]
    Serialize(#[from] serde_json::Error),
}

async fn handle_console_socket(mut stream: WebSocket, params: ConsoleParams) -> Result<(), ConsoleSocketError> {
    let session = match params.session_id {
        Some(session_id) => match CONSOLE_SESSIONS.get(session_id).await {
            Some(session) => session,
            None => {
                let payload = serde_json::to_string(&ConsoleServerEvent::Error { message: "console session not found".into() })?;
                let _ = stream.send(Message::Text(payload.into())).await;
                let _ = stream.close().await;
                return Ok(());
            }
        },
        None => match CONSOLE_SESSIONS.create(params.cols, params.rows).await {
            Ok(session) => session,
            Err(err) => {
                let payload = serde_json::to_string(&ConsoleServerEvent::Error { message: err.to_string() })?;
                let _ = stream.send(Message::Text(payload.into())).await;
                let _ = stream.close().await;
                return Ok(());
            }
        },
    };

    let _client_guard = session.connect_client();
    let (mut ws_tx, mut ws_rx) = stream.split();
    let mut events_rx = session.subscribe();
    let input_tx = session.input_sender();

    ws_tx.send(Message::Text(serde_json::to_string(&session.ready_event())?.into())).await.ok();
    for chunk in session.output_snapshot().await {
        let payload = serde_json::to_string(&ConsoleServerEvent::Output { data: chunk })?;
        if ws_tx.send(Message::Text(payload.into())).await.is_err() {
            return Ok(());
        }
    }

    loop {
        tokio::select! {
            event = events_rx.recv() => {
                let Ok(event) = event else { break; };
                let payload = serde_json::to_string(&event)?;
                if ws_tx.send(Message::Text(payload.into())).await.is_err() {
                    break;
                }
                if matches!(event, ConsoleServerEvent::Exit { .. }) {
                    break;
                }
            }
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ConsoleClientEvent>(&text) {
                            Ok(ConsoleClientEvent::Input { data }) => {
                                session.touch();
                                let _ = input_tx.send(data.into_bytes());
                            }
                            Ok(ConsoleClientEvent::Resize { cols, rows }) => {
                                session.touch();
                                if let Err(err) = session.resize(cols, rows) {
                                    debug!(error = %err, "failed to resize console pty");
                                }
                            }
                            Err(err) => {
                                debug!(error = %err, "invalid console message");
                                let payload = serde_json::to_string(&ConsoleServerEvent::Error { message: format!("invalid input: {err}") })?;
                                let _ = ws_tx.send(Message::Text(payload.into())).await;
                            }
                        }
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        session.touch();
                        let _ = input_tx.send(bytes.to_vec());
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        let _ = ws_tx.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let _ = ws_tx.send(Message::Close(frame)).await;
                        break;
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Err(err)) => {
                        debug!(error = %err, "console websocket closed with error");
                        break;
                    }
                    None => break,
                }
            }
        }
    }

    Ok(())
}

pub fn register_docs(host: Option<String>, registry: &mut SchemaRegistry, servers: &mut std::collections::BTreeMap<String, Server>, tags: &mut Vec<Tag>, docs: &mut Vec<WsDoc>) {
    registry.track::<ConsoleClientEvent, _>(ConsoleClientEvent::register_schemas);
    registry.track::<ConsoleServerEvent, _>(ConsoleServerEvent::register_schemas);

    let doc = WsDoc {
        path: "console.session",
        summary: "Interactive console",
        description: "PTY-backed interactive shell session (supports persistent sessions via sessionId).",
        tags: vec!["console".into()],
        payload: Some(TypeSchema { name: ConsoleClientEvent::NAME, schema: ConsoleClientEvent::schema() }),
        responses: vec![TypeSchema { name: ConsoleServerEvent::NAME, schema: ConsoleServerEvent::schema() }],
        params: vec![],
    };

    let server_host = host.unwrap_or_else(|| "localhost:5800/v1/ws".to_string());
    servers.entry("primary".into()).or_insert(Server { host: server_host, protocol: "ws".into(), protocol_version: None, description: Some("Primary WebSocket entrypoint".into()) });
    docs.push(doc);
    tags.push(Tag { name: "console".into(), description: Some("Console session".to_string()), external_docs: None });
}
