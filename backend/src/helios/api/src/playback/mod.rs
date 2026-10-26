//! Playback: a recording (or a folder of images) served as a camera.
//!
//! A **player** is a Styx `CameraService` in helios-api, serving a replayed recording
//! (`ReplaySourceConfig`, with the replay controls) or an image sequence (`FileSourceConfig`) on
//! its own socket. It is published to Orion as a `camera.device` resource labelled
//! `helios.label.capture_role=playback`, so a pipeline binds to it exactly as to a live camera.
//! Players live in this process: closing one (or restarting helios-api) removes its resource, and
//! the API's startup publishes the empty set, which clears any left by an earlier run.
//!
//! - `request.rs`: `POST /v1/playback` bodies and the player's JSON view.
//! - `source.rs`: resolving a recording id, a video or a folder into a replayable source. Replayed
//!   without conversion: MCAP recordings, Motion JPEG streams (`.mjpeg`), AVI (`.avi`) and JPEG
//!   folders (at the player frame rate). H.264 or MP4 files: convert on a PC with
//!   `styx-record convert IN OUT.mcap --gray --lz4` and copy the MCAP to the recordings directory.
//! - `facts.rs`: a recording's `helios.camera` metadata (calibration, mount, mode).
//! - `player.rs`: the device and the camera service of one player.
//! - `registry.rs`: the open players, their limit and their Orion resources.
//! - `resource.rs`: the Orion resource a player is published as.
//! - `transport.rs`: the replay controls' state, and seek requests.
//! - `context.rs`: which calibration and mount a run on a recording uses.
//! - `runs.rs`: "Run on this recording" (a pipeline copy bound to a player).

pub mod context;
pub mod facts;
pub mod player;
pub mod registry;
pub mod request;
pub mod resource;
pub mod runs;
pub mod source;
pub mod transport;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

pub use registry::Players;

/// Where players' sockets go: the peripherals' stream directory, which the engine reaches.
/// How many players may be open at once.
pub const DEFAULT_MAX_PLAYERS: usize = 2;
/// The resource id prefix of a player (`camera.playback.<token>`).
pub const RESOURCE_PREFIX: &str = "camera.playback.";
/// The `capture_role` label that marks a camera resource as playback.
pub const CAPTURE_ROLE: &str = "playback";
/// The endpoint scheme of a camera service's frames (`styx-frames+unix://<socket>`).
pub const FRAMES_SCHEME: &str = "styx-frames+unix://";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackSettings {
    pub max_players: usize,
    pub stream_dir: PathBuf,
    /// Folders (besides the recordings directory) that a `path` source may name.
    pub roots: Vec<PathBuf>,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        // The image's stream directory (`ApiConfig::from_layout` takes the configured layout's): the
        // peripherals' stream directory, which the engine reaches.
        Self { max_players: DEFAULT_MAX_PLAYERS, stream_dir: helios_layout::Layout::image().streams_dir(), roots: Vec::new() }
    }
}

/// Whether a camera resource is a player (not a live camera).
pub fn is_player(resource_id: &str) -> bool {
    resource_id.starts_with(RESOURCE_PREFIX)
}

/// A new player's token: 8 random hex digits (its id and resource id end with it).
pub fn new_token() -> String {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes).expect("OS randomness");
    hex::encode(bytes)
}

/// How often the open players are published again: Orion takes them back if it was restarted.
const PUBLISH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);
/// How soon the set is tried again while Orion does not take it.
const RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// Publishes the open players (empty at boot: the resources an earlier helios-api published go),
/// and forgets the calibrations and mounts stored for players that are gone. Until Orion takes
/// the set it is tried again, logging the first failure only.
pub fn spawn_startup(state: crate::SharedState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut first = true;
        let mut failing = false;
        loop {
            match state.playback.sync().await {
                Ok(()) => {
                    failing = false;
                    if first {
                        first = false;
                        let open = state.playback.resource_ids().await;
                        if let Err(error) = context::forget_stale(&state.store, &open).await {
                            tracing::warn!(error = %error.message, "playback: stored settings of earlier players not cleared");
                        }
                    }
                    tokio::time::sleep(PUBLISH_INTERVAL).await;
                }
                Err(error) => {
                    if !failing {
                        tracing::warn!(error = %error.message, "playback: resources not published yet");
                    }
                    failing = true;
                    tokio::time::sleep(RETRY_INTERVAL).await;
                }
            }
        }
    })
}
