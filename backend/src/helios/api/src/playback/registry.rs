//! The open players: at most the configured number, each with its service and its Orion resource.
//! Every change publishes the whole set, so Orion's view is the open players, nothing else.

use std::path::{Path, PathBuf};

use orion::client::LocalNodeRuntime;

use super::{
    FRAMES_SCHEME, PlaybackSettings,
    facts::RecordingFacts,
    new_token,
    player::{Player, StartOptions, device_for, serve, socket_path},
    request::{PlayerView, SourceKind},
    resource::{PlaybackPublisher, resource_record},
    source::Resolved,
};
use crate::error::{ApiError, ApiResult};

pub struct Players {
    open: tokio::sync::Mutex<Vec<Player>>,
    settings: PlaybackSettings,
    publisher: PlaybackPublisher,
}

impl Players {
    pub fn new(runtime: LocalNodeRuntime, node_id: &str, settings: PlaybackSettings) -> Self {
        Self { open: tokio::sync::Mutex::new(Vec::new()), publisher: PlaybackPublisher::new(runtime, node_id), settings }
    }

    pub fn settings(&self) -> &PlaybackSettings {
        &self.settings
    }

    /// Opens a player for `resolved`, published as a camera. Refused at the limit.
    pub async fn open(&self, resolved: Resolved, options: StartOptions, created_at_ms: u64) -> ApiResult<PlayerView> {
        let mut open = self.open.lock().await;
        if open.len() >= self.settings.max_players {
            return Err(ApiError::conflict(format!("{} players are open, the limit; close one first", open.len())));
        }
        let token = fresh_token(&open);
        let device = device_for(&resolved, options)?;
        let socket = socket_path(&self.settings.stream_dir, &token);
        let serve_socket = socket.clone();
        let service = tokio::task::spawn_blocking(move || serve(&serve_socket, device)).await.map_err(|error| ApiError::internal(error.to_string()))??;
        let view = view_of(&resolved, &token, options, created_at_ms, self.settings.stream_dir.as_path());
        let mut records: Vec<_> = open.iter().map(|player| resource_record(&player.view, self.publisher.provider())).collect();
        records.push(resource_record(&view, self.publisher.provider()));
        if let Err(error) = self.publisher.publish(records).await {
            Player { view: view.clone(), token, socket, facts: resolved.facts.clone(), service: Some(service) }.stop().await;
            return Err(error);
        }
        open.push(Player { view: view.clone(), token, socket, facts: resolved.facts.clone(), service: Some(service) });
        Ok(view)
    }

    /// Closes the player `id`: its service stops and its resource leaves Orion. `None` when there
    /// is no such player.
    pub async fn close(&self, id: &str) -> ApiResult<Option<PlayerView>> {
        let mut open = self.open.lock().await;
        let Some(index) = open.iter().position(|player| player.view.id == id) else {
            return Ok(None);
        };
        let player = open.remove(index);
        let records: Vec<_> = open.iter().map(|other| resource_record(&other.view, self.publisher.provider())).collect();
        if let Err(error) = self.publisher.publish(records).await {
            open.insert(index, player);
            return Err(error);
        }
        let view = player.view.clone();
        player.stop().await;
        Ok(Some(view))
    }

    /// Publishes the open players as they are. Run once at startup, it clears the resources an
    /// earlier helios-api published (its players are gone with it).
    pub async fn sync(&self) -> ApiResult<()> {
        let open = self.open.lock().await;
        let records = open.iter().map(|player| resource_record(&player.view, self.publisher.provider())).collect();
        self.publisher.publish(records).await
    }

    pub async fn views(&self) -> Vec<PlayerView> {
        self.open.lock().await.iter().map(|player| player.view.clone()).collect()
    }

    pub async fn get(&self, id: &str) -> Option<PlayerView> {
        self.open.lock().await.iter().find(|player| player.view.id == id).map(|player| player.view.clone())
    }

    /// The player `id`'s camera service socket.
    pub async fn socket(&self, id: &str) -> Option<PathBuf> {
        self.open.lock().await.iter().find(|player| player.view.id == id).map(|player| player.socket.clone())
    }

    /// The recording facts of the player `id`.
    pub async fn facts(&self, id: &str) -> Option<RecordingFacts> {
        self.open.lock().await.iter().find(|player| player.view.id == id).map(|player| player.facts.clone())
    }

    /// The ids of the open players' resources (`camera.playback.<token>`).
    pub async fn resource_ids(&self) -> Vec<String> {
        self.open.lock().await.iter().map(|player| player.view.resource_id.clone()).collect()
    }
}

#[cfg(test)]
impl Players {
    /// Players that record what they publish instead of talking to Orion (tests).
    pub fn recorded(settings: PlaybackSettings) -> Self {
        Self { open: tokio::sync::Mutex::new(Vec::new()), publisher: PlaybackPublisher::recorded("node-test"), settings }
    }

    /// Every set published so far (tests).
    pub fn published(&self) -> Vec<Vec<orion::control_plane::ResourceRecord>> {
        self.publisher.published()
    }
}

fn fresh_token(open: &[Player]) -> String {
    loop {
        let token = new_token();
        if !open.iter().any(|player| player.token == token) {
            return token;
        }
    }
}

fn view_of(resolved: &Resolved, token: &str, options: StartOptions, created_at_ms: u64, stream_dir: &Path) -> PlayerView {
    let frames_endpoint = format!("{FRAMES_SCHEME}{}", socket_path(stream_dir, token).display());
    PlayerView {
        id: format!("playback-{token}"),
        resource_id: format!("{}{token}", super::RESOURCE_PREFIX),
        name: format!("Playback: {}", resolved.label),
        source: resolved.source.clone(),
        kind: resolved.kind,
        raw_only: resolved.raw_only,
        note: resolved.note.clone(),
        frames_endpoint,
        speed: options.speed,
        looping: options.looping,
        fps: (resolved.kind == SourceKind::Images).then_some(options.fps),
        created_at_ms,
        status: None,
        status_error: None,
    }
}
