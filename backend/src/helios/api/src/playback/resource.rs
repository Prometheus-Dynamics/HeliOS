//! The Orion resource a player is published as: a `camera.device` of the API's own provider
//! (`provider.playback.<node>`), labelled as playback, with the camera service's frames endpoint.
//! Every publish replaces the provider's whole set, so the set is always the open players.

use orion::{
    ResourceId,
    client::{ClientError, LocalNodeRuntime, LocalRuntimePublisher, ProviderResource},
    control_plane::{AvailabilityState, HealthState, LeaseState, ProviderRecord, ResourceOwnershipMode, ResourceRecord},
    core::{NodeId, ProviderId},
};

use super::{
    CAPTURE_ROLE,
    request::{PlaybackSource, PlayerView, SourceKind},
};
use crate::error::{ApiError, ApiResult};

const CAMERA_TYPE: &str = "camera.device";
const DISPLAY_NAME_LABEL: &str = "helios.display_name";
const KIND_LABEL: &str = "helios.kind";
const LABEL_PREFIX: &str = "helios.label.";
const PLAYBACK_SOURCE_LABEL: &str = "helios.playback.source";
const PLAYBACK_KIND_LABEL: &str = "helios.playback.kind";

/// The Orion resource of `view`: a shared-read camera with the frames endpoint of its service.
pub fn resource_record(view: &PlayerView, provider: &str) -> ResourceRecord {
    let source = match &view.source {
        PlaybackSource::Recording { recording } => recording.clone(),
        PlaybackSource::Path { path } => path.clone(),
    };
    let (kind, backend) = match view.kind {
        SourceKind::Recording => ("recording", "replay"),
        SourceKind::Images => ("images", "replay"),
    };
    ProviderResource::new(ResourceId::new(view.resource_id.clone()), CAMERA_TYPE, ProviderId::new(provider.to_string()))
        .ownership_mode(ResourceOwnershipMode::SharedRead)
        .health(HealthState::Healthy)
        .availability(AvailabilityState::Available)
        .lease_state(LeaseState::Unleased)
        .label(format!("{DISPLAY_NAME_LABEL}={}", view.name))
        .label(format!("{KIND_LABEL}=capture_device"))
        .label(format!("{LABEL_PREFIX}capture_role={CAPTURE_ROLE}"))
        .label(format!("{LABEL_PREFIX}styx.backend={backend}"))
        .label(format!("{PLAYBACK_KIND_LABEL}={kind}"))
        .label(format!("{PLAYBACK_SOURCE_LABEL}={source}"))
        .endpoint(view.frames_endpoint.clone())
        .build()
}

/// The provider id of the API's playback resources on `node_id`.
pub fn provider_id(node_id: &str) -> String {
    format!("provider.playback.{node_id}")
}

/// Publishes the player set as the API's provider's resources (registering the provider first,
/// once).
pub struct PlaybackPublisher {
    sink: Sink,
    provider: String,
}

/// Where the sets go: Orion, or (in the tests) a list of what was published.
enum Sink {
    Orion {
        publisher: Box<LocalRuntimePublisher>,
        registered: tokio::sync::Mutex<bool>,
    },
    #[cfg(test)]
    Recorded(std::sync::Mutex<Vec<Vec<ResourceRecord>>>),
}

impl PlaybackPublisher {
    pub fn new(runtime: LocalNodeRuntime, node_id: &str) -> Self {
        let provider = provider_id(node_id);
        let record = ProviderRecord::builder(ProviderId::new(provider.clone()), NodeId::new(node_id.to_string())).resource_type(CAMERA_TYPE).build();
        let publisher = LocalRuntimePublisher::builder(runtime, "helios-api-playback").provider(record).build();
        Self { sink: Sink::Orion { publisher: Box::new(publisher), registered: tokio::sync::Mutex::new(false) }, provider }
    }

    /// A publisher that records its sets instead of publishing them (tests).
    #[cfg(test)]
    pub fn recorded(node_id: &str) -> Self {
        Self { sink: Sink::Recorded(std::sync::Mutex::new(Vec::new())), provider: provider_id(node_id) }
    }

    /// Every set published so far, oldest first (tests).
    #[cfg(test)]
    pub fn published(&self) -> Vec<Vec<ResourceRecord>> {
        match &self.sink {
            Sink::Recorded(sets) => sets.lock().expect("recorded sets").clone(),
            Sink::Orion { .. } => Vec::new(),
        }
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// Replaces the provider's resources with `records`.
    pub async fn publish(&self, records: Vec<ResourceRecord>) -> ApiResult<()> {
        match &self.sink {
            Sink::Orion { publisher, registered } => {
                let mut registered = registered.lock().await;
                if !*registered {
                    publisher.register_all().await.map_err(orion_error)?;
                    *registered = true;
                }
                publisher.publish_provider_resources(records).await.map_err(orion_error)
            }
            #[cfg(test)]
            Sink::Recorded(sets) => {
                sets.lock().expect("recorded sets").push(records);
                Ok(())
            }
        }
    }
}

fn orion_error(error: ClientError) -> ApiError {
    ApiError::backend(format!("Orion did not take the playback resources: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::request::{PlaybackSource, PlayerView, SourceKind};

    fn view(kind: SourceKind, source: PlaybackSource) -> PlayerView {
        PlayerView {
            id: "playback-1a2b3c4d".into(),
            resource_id: "camera.playback.1a2b3c4d".into(),
            name: "Playback: rec-1".into(),
            source,
            kind,
            raw: false,
            note: None,
            frames_endpoint: "styx-frames+unix:///run/helios/streams/playback-1a2b3c4d.styx.sock".into(),
            speed: 1.0,
            looping: false,
            fps: None,
            created_at_ms: 1,
            status: None,
            status_error: None,
        }
    }

    #[test]
    fn a_recording_player_is_a_playback_camera_with_its_frames_endpoint() {
        let record = resource_record(&view(SourceKind::Recording, PlaybackSource::Recording { recording: "rec-1".into() }), "provider.playback.node-local");
        assert_eq!(record.resource_type.as_str(), CAMERA_TYPE);
        assert_eq!(record.provider_id.as_str(), "provider.playback.node-local");
        assert!(record.labels.iter().any(|label| label == "helios.label.capture_role=playback"));
        assert!(record.labels.iter().any(|label| label == "helios.playback.source=rec-1"));
        assert!(record.labels.iter().any(|label| label == "helios.playback.kind=recording"));
        assert!(record.endpoints.iter().any(|endpoint| endpoint.starts_with(crate::playback::FRAMES_SCHEME)));
    }

    #[test]
    fn image_players_name_the_folder_they_play() {
        let record = resource_record(&view(SourceKind::Images, PlaybackSource::Path { path: "/data/frames".into() }), "provider.playback.node-local");
        assert!(record.labels.iter().any(|label| label == "helios.playback.kind=images"));
        assert!(record.labels.iter().any(|label| label == "helios.label.styx.backend=replay"));
        assert!(record.labels.iter().any(|label| label == "helios.playback.source=/data/frames"));
    }

    #[test]
    fn the_provider_is_named_by_node() {
        assert_eq!(provider_id("node-local"), "provider.playback.node-local");
    }
}
