use crate::http::AppState;
use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use helios_updater::ipc::{UpdateStage, UpdateState, UpdaterEvent};
use lib_asyncapi::registry::SchemaRegistry;
use lib_asyncapi::{SchemaProvider, Server, Tag, TypeSchema, WsDoc};
use schemars::{JsonSchema, schema_for};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct UpdaterArtifact {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct UpdaterSnapshot {
    pub update_id: String,
    pub stage: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<UpdaterArtifact>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UpdaterServerEvent {
    Snapshot {
        state: Option<UpdaterSnapshot>,
        cache_usage_bytes: u64,
    },
    StageProgress {
        update_id: String,
        percent: u8,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    StageComplete {
        update_id: String,
    },
    ApplyScheduled {
        update_id: String,
        eta: String,
    },
    ApplyComplete {
        update_id: String,
        reboot_required: bool,
    },
    RollbackTriggered {
        update_id: String,
        reason: String,
    },
    Error {
        message: String,
    },
}

pub async fn ota_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ota_loop(socket, state))
}

async fn ota_loop(socket: WebSocket, state: AppState) {
    let (mut tx, mut rx) = socket.split();
    let conn = match state.services.updater.ensure_updater(&state).await {
        Ok(conn) => conn,
        Err(err) => {
            let _ = send_event(&mut tx, UpdaterServerEvent::Error { message: err }).await;
            return;
        }
    };

    let mut session = match conn.checkout_session().await {
        Ok(session) => session,
        Err(err) => {
            state.services.updater.invalidate_updater(&state, &conn).await;
            let _ = send_event(&mut tx, UpdaterServerEvent::Error { message: err.to_string() }).await;
            return;
        }
    };

    if let Err(err) = state.services.updater.send_query_state(&conn, &mut session, "ota_ws_state").await {
        state.services.updater.invalidate_updater(&state, &conn).await;
        let _ = send_event(&mut tx, UpdaterServerEvent::Error { message: err }).await;
        return;
    }

    loop {
        tokio::select! {
            event = session.next_event() => {
                match event {
                    Ok(Some(event)) => {
                        if let Some(mapped) = map_event(event)
                            && send_event(&mut tx, mapped).await.is_err() {
                                break;
                            }
                    }
                    Ok(None) => break,
                    Err(err) => {
                        let _ = send_event(&mut tx, UpdaterServerEvent::Error { message: err.to_string() }).await;
                        state.services.updater.invalidate_updater(&state, &conn).await;
                        break;
                    }
                }
            }
            msg = rx.next() => {
                if matches!(msg, None | Some(Err(_)) | Some(Ok(Message::Close(_)))) {
                    break;
                }
            }
        }
    }

    conn.recycle_session(session).await;
}

async fn send_event(tx: &mut futures::stream::SplitSink<WebSocket, Message>, event: UpdaterServerEvent) -> Result<(), ()> {
    let body = serde_json::to_string(&event).map_err(|_| ())?;
    tx.send(Message::Text(body.into())).await.map_err(|_| ())
}

fn map_event(event: UpdaterEvent) -> Option<UpdaterServerEvent> {
    match event {
        UpdaterEvent::StateSnapshot { active_update, cache_usage_bytes } => Some(UpdaterServerEvent::Snapshot { state: active_update.map(map_snapshot), cache_usage_bytes }),
        UpdaterEvent::StageProgress { update_id, percent, detail } => Some(UpdaterServerEvent::StageProgress { update_id: update_id.to_string(), percent, detail }),
        UpdaterEvent::StageComplete { update_id } => Some(UpdaterServerEvent::StageComplete { update_id: update_id.to_string() }),
        UpdaterEvent::ApplyScheduled { update_id, eta } => Some(UpdaterServerEvent::ApplyScheduled { update_id: update_id.to_string(), eta: eta.to_rfc3339() }),
        UpdaterEvent::ApplyComplete { update_id, reboot_required } => Some(UpdaterServerEvent::ApplyComplete { update_id: update_id.to_string(), reboot_required }),
        UpdaterEvent::RollbackTriggered { update_id, reason } => Some(UpdaterServerEvent::RollbackTriggered { update_id: update_id.to_string(), reason }),
        _ => None,
    }
}

fn map_snapshot(state: UpdateState) -> UpdaterSnapshot {
    UpdaterSnapshot {
        update_id: state.update_id.to_string(),
        stage: map_stage(&state.stage),
        progress_percent: state.progress_percent,
        started_at: state.started_at.map(|value| value.to_rfc3339()),
        finished_at: state.finished_at.map(|value| value.to_rfc3339()),
        last_error: state.last_error,
        artifacts: state.artifacts.into_iter().map(|artifact| UpdaterArtifact { url: artifact.url.to_string(), checksum: artifact.checksum, size_bytes: artifact.size_bytes }).collect(),
    }
}

fn map_stage(stage: &UpdateStage) -> String {
    serde_json::to_value(stage).ok().and_then(|value| value.as_str().map(|value| value.to_string())).unwrap_or_else(|| format!("{stage:?}").to_ascii_lowercase())
}

fn schema<T: JsonSchema>() -> serde_json::Value {
    serde_json::to_value(schema_for!(T)).expect("schema")
}

impl SchemaProvider for UpdaterServerEvent {
    const NAME: &'static str = "UpdaterServerEvent";
    fn schema() -> serde_json::Value {
        schema::<UpdaterServerEvent>()
    }
    fn register_schemas(map: &mut BTreeMap<String, serde_json::Value>) {
        map.entry(Self::NAME.to_string()).or_insert_with(Self::schema);
    }
}

pub fn register_docs(host: Option<String>, registry: &mut SchemaRegistry, servers: &mut BTreeMap<String, Server>, tags: &mut Vec<Tag>, docs: &mut Vec<WsDoc>) {
    registry.track::<UpdaterServerEvent, _>(UpdaterServerEvent::register_schemas);
    let doc = WsDoc {
        path: "ota.stream",
        summary: "OTA update status stream",
        description: "Streams OTA updater state snapshots and progress events.",
        tags: vec!["ota".into()],
        payload: None,
        responses: vec![TypeSchema { name: UpdaterServerEvent::NAME, schema: UpdaterServerEvent::schema() }],
        params: vec![],
    };
    let server_host = host.unwrap_or_else(|| "localhost:5800/v1/ws".to_string());
    servers.entry("primary".into()).or_insert(Server { host: server_host, protocol: "ws".into(), protocol_version: None, description: Some("Primary WebSocket entrypoint".into()) });
    docs.push(doc);
    tags.push(Tag { name: "ota".into(), description: Some("OTA updater state".to_string()), external_docs: None });
}
