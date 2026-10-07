//! helios-api: the HeliOS application API (`/v1`), for the HeliOS UI and Atlas.
//!
//! The API owns little state of its own. It reads and writes Orion (pipelines, resources), drives
//! the device package's A/B updater for OS updates, serves the UI's static build, asks the Styx camera services for camera facts and controls, and reads systemd, the journal and
//! the kernel for device facts. Features without a backend answer 501 with
//! `{"error": {"code": "not_available", "needs": "..."}}`. See `docs/docs/api/http.md`.

pub mod auth;
pub mod auth_state;
pub mod camera_controls;
pub mod config;
pub mod error;
pub mod events;
pub mod host;
pub mod orion;
pub mod pd_update;
pub mod routes;
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
    /// Device security: open (default) or secured.
    pub auth: auth::Auth,
    /// Held while helios-api has the device package's writer stage an image.
    pub update_lock: Arc<Mutex<()>>,
    /// The last stage/apply helios-api started, for `/v1/update/status`.
    pub update_task: std::sync::Mutex<Option<routes::update::UpdateTask>>,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn new(config: ApiConfig) -> SharedState {
        let store = Arc::new(store::Store::new(config.state_dir.clone()));
        let events = Arc::new(events::EventHub::new(256));
        Arc::new(Self {
            orion: orion::Orion::new(config.orion_socket.clone(), config.orion_stream_socket.clone()),
            cameras: camera_controls::CameraControls::new(store.clone(), events.clone()),
            store,
            events,
            auth: auth::Auth::new(config.auth_file.clone()),
            update_lock: routes::update::new_update_lock(),
            update_task: std::sync::Mutex::new(None),
            config,
        })
    }
}

pub use routes::router;
