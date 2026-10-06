//! helios-api: the HeliOS application API (`/v1`), for the HeliOS UI and Atlas.
//!
//! The API owns little state of its own. It reads and writes Orion (pipelines, resources, the
//! updater), asks the Styx camera services for camera facts and controls, and reads systemd, the journal and
//! the kernel for device facts. Features without a backend answer 501 with
//! `{"error": {"code": "not_available", "needs": "..."}}`. See `docs/docs/api/http.md`.

pub mod auth;
pub mod camera_controls;
pub mod config;
pub mod error;
pub mod events;
pub mod host;
pub mod orion;
pub mod routes;
pub mod store;

use std::sync::Arc;

use tokio::sync::Mutex;

pub use config::ApiConfig;

pub const API_VERSION: &str = "v1";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct AppState {
    pub config: ApiConfig,
    pub orion: orion::Orion,
    pub store: store::Store,
    pub cpu: host::CpuSampler,
    pub events: Arc<events::EventHub>,
    /// One Styx client per camera service, for camera controls (opened on first use).
    pub cameras: camera_controls::CameraControls,
    /// Device security: open (default) or secured.
    pub auth: auth::Auth,
    /// Held while an OTA image is being prepared and submitted.
    pub update_lock: Mutex<()>,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn new(config: ApiConfig) -> SharedState {
        Arc::new(Self {
            orion: orion::Orion::new(config.orion_socket.clone(), config.orion_stream_socket.clone()),
            store: store::Store::new(config.state_dir.clone()),
            cpu: host::CpuSampler::default(),
            events: Arc::new(events::EventHub::new(256)),
            cameras: camera_controls::CameraControls::default(),
            auth: auth::Auth::new(config.auth_file.clone()),
            update_lock: Mutex::new(()),
            config,
        })
    }
}

pub use routes::router;
