//! helios-api: the HeliOS application API (`/v1`), for the HeliOS UI and Atlas.
//!
//! The API owns little state of its own. It reads and writes Orion (pipelines, resources), drives
//! the device package's A/B updater for OS updates, serves the UI's static build, asks the Styx camera services for camera facts and controls, and reads systemd, the journal and
//! the kernel for device facts. Features without a backend answer 501 with
//! `{"error": {"code": "not_available", "needs": "..."}}`. See `docs/docs/api/http.md`.

pub mod auth;
pub mod auth_state;
pub mod board_update;
pub mod calibration_capture;
pub mod camera_context;
pub mod camera_uncertainty;
pub mod camera_controls;
pub mod camera_preview;
pub mod config;
pub mod error;
pub mod events;
pub mod field_layouts;
pub mod host;
pub mod leds;
pub mod live_outputs;
pub mod live_streams;
pub mod orion;
pub mod playback;
pub mod recording_nt;
pub mod recording_rings;
pub mod recordings;
pub mod routes;
pub mod self_calibration;
pub mod startup;
pub mod store;
pub mod ui;

use std::sync::Arc;

use tokio::sync::Mutex;

pub use config::ApiConfig;

pub const API_VERSION: &str = "v1";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct AppState {
    pub config: ApiConfig,
    pub orion: orion::Orion,
    pub store: Arc<store::Store>,
    pub events: Arc<events::EventHub>,
    /// One Styx control client per camera, for camera controls and their persisted values.
    pub cameras: camera_controls::CameraControls,
    /// One Styx preview per camera, made on its first viewer.
    pub previews: camera_preview::CameraPreviews,
    /// Calibration views captured per camera, until solved.
    pub calibrations: calibration_capture::CalibrationSessions,
    /// The session recorder: one recording at a time, from a camera's frames.
    pub recorder: helios_recorder::Recorder,
    /// Live journal tails, one `journalctl -f` per filter shared by its viewers.
    pub log_followers: routes::logs::LogFollowers,
    /// Device security: open (default) or secured.
    pub auth: auth::Auth,
    /// Held while a peripheral action runs: helios-api runs one at a time, so its callers queue
    /// instead of meeting each other's resource leases.
    /// Held while helios-api has the device package's writer stage an image.
    pub update_lock: Arc<Mutex<()>>,
    /// The last stage/apply helios-api started, for `/v1/update/status`.
    pub update_task: std::sync::Mutex<Option<routes::update::UpdateTask>>,
    /// The LED director: HeliOS's state on the status ring (`leds`, started by `leds::start`).
    pub leds: leds::LedHandle,
    /// Every pipeline output of every tick, relayed from helios-engine (`GET /v1/live/ws`).
    pub live: Arc<live_outputs::LiveRelay>,
    /// The open playback players (recordings and image folders served as cameras).
    pub playback: playback::Players,
    /// The node's live streams for the UI (`/v1/streams/ws`): one subscription, shared.
    pub live_streams: Arc<live_streams::LiveStreams>,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn new(config: ApiConfig) -> SharedState {
        let store = Arc::new(store::Store::new(config.state_dir.clone()));
        let events = Arc::new(events::EventHub::new(256));
        let orion_link = orion::Orion::new(config.orion_socket.clone(), config.orion_stream_socket.clone());
        Arc::new(Self {
            live_streams: live_streams::LiveStreams::new(config.live_streams.clone(), orion_link.clone()),
            orion: orion_link,
            cameras: camera_controls::CameraControls::new(store.clone(), events.clone()),
            previews: camera_preview::CameraPreviews::new(config.preview),
            calibrations: calibration_capture::CalibrationSessions::default(),
            recorder: helios_recorder::Recorder::default(),
            log_followers: routes::logs::LogFollowers::default(),
            store,
            events,
            auth: auth::Auth::new(config.auth_file.clone()),
            update_lock: routes::update::new_update_lock(),
            update_task: std::sync::Mutex::new(None),
            leds: leds::LedHandle::new(),
            live: live_outputs::LiveRelay::new(),
            playback: playback::Players::new(::orion::client::LocalNodeRuntime::new(config.orion_socket.clone(), config.orion_stream_socket.clone()), &config.node_id, config.playback.clone()),
            config,
        })
    }
}

pub use routes::router;
