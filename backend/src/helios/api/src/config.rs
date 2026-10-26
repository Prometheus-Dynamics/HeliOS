//! Runtime configuration from the environment (`/etc/default/helios-api.env` on the device).
//!
//! Paths come from the install layout (`helios-layout`): the image's by default, a guest's when
//! the layout says so. The variables here override single paths after the layout is read.

use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf};

use helios_layout::{BoardContract, Layout};

pub const DEFAULT_BIND: &str = "127.0.0.1:5800";
pub const DEFAULT_NODE_ID: &str = "node-local";

/// systemd units the API reports and may restart, on the owner image.
pub const MANAGED_UNITS: &[&str] = &["orion-node.service", "helios-engine.service", "helios-peripherals.service", "helios-api.service"];
/// The same units on a guest package: its Orion is `helios-orion.service` (the SystemCore build).
pub const GUEST_MANAGED_UNITS: &[&str] = &["helios-orion.service", "helios-engine.service", "helios-peripherals.service", "helios-api.service"];

#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub bind: SocketAddr,
    pub node_id: String,
    /// The install layout every path here came from.
    pub layout: Layout,
    pub orion_socket: PathBuf,
    pub orion_stream_socket: PathBuf,
    /// Pipeline revision history and camera mounts.
    pub state_dir: PathBuf,
    /// Where OTA uploads are kept until they are staged. On the data partition (the layout's
    /// state is bind-mounted from it), since `/run` is RAM and the device package's writer reads
    /// the image from a file.
    pub upload_dir: PathBuf,
    /// Run the update CLI through `systemd-run`, outside helios-api's cgroup, so that stopping
    /// helios-api (the pre-reboot hook does) cannot kill it. Off runs it directly (tests).
    pub board_update_systemd_run: bool,
    /// The UI's static files; `None` (or a missing directory) serves the API only.
    pub ui_dir: Option<PathBuf>,
    /// The device package's files (identity, update writer, camera description). `None` on a
    /// guest, which has none: the features that need them report that they are not available.
    pub board: Option<BoardContract>,
    /// The systemd units the services list reports and may restart (`MANAGED_UNITS` on the owner,
    /// `GUEST_MANAGED_UNITS` on a guest).
    pub managed_units: &'static [&'static str],
    /// A guest's security (sign-in and tokens) is off unless this is set: the host owns access
    /// there. `HELIOS_API_GUEST_SECURITY=on` turns it on.
    pub guest_security: bool,
    /// Unit tests have no Orion: these stand in for the lemnosd device classes Orion reports
    /// (`None` reads Orion).
    #[cfg(test)]
    pub test_lemnos_classes: Option<std::collections::BTreeSet<String>>,
    /// `Access-Control-Allow-Origin` value; `None` sends no CORS headers.
    pub cors_origin: Option<String>,
    /// Largest accepted OTA upload.
    pub max_upload_bytes: u64,
    /// Device security state (password hash, API token hashes). Absent means open.
    pub auth_file: PathBuf,
    /// Camera previews: size, frame rate and JPEG quality.
    pub preview: crate::camera_preview::PreviewSettings,
    /// The first-boot startup file (`[default_pipeline]`, see `startup.rs`).
    pub startup_file: PathBuf,
    /// lemnosd's socket: the IMU, magnetometer and power samples a recording takes.
    pub lemnos_socket: PathBuf,
    /// Playback players (`playback/`): their concurrent limit, sockets and allowed folders.
    pub playback: crate::playback::PlaybackSettings,
    /// The node's live stream the UI shows, and the history a new client starts with.
    pub live_streams: crate::live_streams::LiveStreamSettings,
    /// The networks the robot's links are on. The gateway reaches a peer only over one of these
    /// (`peers.rs`); none means no peer is reached.
    pub peer_nets: Vec<crate::peers::Ipv4Net>,
    /// The HeliOS API ports a discovered peer is probed on while security is open (`peers.rs`).
    pub peer_ports: Vec<u16>,
}

impl Default for ApiConfig {
    /// The image's configuration.
    fn default() -> Self {
        Self::from_layout(&Layout::image())
    }
}

impl ApiConfig {
    /// The configuration of the device whose layout this is, before the environment overrides it.
    pub fn from_layout(layout: &Layout) -> Self {
        Self {
            bind: DEFAULT_BIND.parse().expect("default bind address"),
            node_id: DEFAULT_NODE_ID.into(),
            layout: layout.clone(),
            orion_socket: layout.orion_socket(),
            orion_stream_socket: layout.orion_stream_socket(),
            state_dir: layout.api_state_dir(),
            upload_dir: layout.upload_dir(),
            board_update_systemd_run: true,
            ui_dir: Some(layout.ui_dir()),
            board: layout.board().cloned(),
            managed_units: if layout.board().is_some() { MANAGED_UNITS } else { GUEST_MANAGED_UNITS },
            guest_security: false,
            #[cfg(test)]
            test_lemnos_classes: None,
            cors_origin: None,
            max_upload_bytes: 8 << 30,
            auth_file: layout.auth_file(),
            preview: crate::camera_preview::PreviewSettings::default(),
            startup_file: layout.startup_file(),
            lemnos_socket: layout.lemnos_socket(),
            playback: crate::playback::PlaybackSettings { stream_dir: layout.streams_dir(), ..crate::playback::PlaybackSettings::default() },
            live_streams: crate::live_streams::LiveStreamSettings::default(),
            peer_nets: Vec::new(),
            peer_ports: crate::peers::DEFAULT_PEER_PORTS.to_vec(),
        }
    }

    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_vars(std::env::vars())
    }

    pub fn from_vars<I, K, V>(vars: I) -> anyhow::Result<Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let env: BTreeMap<String, String> = vars.into_iter().map(|(k, v)| (k.into(), v.into())).filter(|(_, v)| !v.trim().is_empty()).collect();
        let layout = Layout::from_vars(env.iter())?;
        let mut config = Self::from_layout(&layout);
        if let Some(bind) = env.get("HELIOS_API_BIND_ADDR").or_else(|| env.get("HELIOS_API_BIND")) {
            config.bind = bind.parse()?;
        }
        if let Some(value) = env.get("HELIOS_NODE_ID") {
            config.node_id = value.clone();
        }
        if let Some(value) = env.get("HELIOS_ORION_IPC_SOCKET") {
            config.orion_socket = value.into();
        }
        if let Some(value) = env.get("HELIOS_ORION_IPC_STREAM_SOCKET") {
            config.orion_stream_socket = value.into();
        }
        if let Some(value) = env.get("HELIOS_API_STATE_DIR") {
            config.state_dir = value.into();
        }
        if let Some(value) = env.get("HELIOS_API_UPLOAD_DIR") {
            config.upload_dir = value.into();
        }
        // The board's own files exist on the image only; on a guest these have nothing to override.
        if let Some(board) = config.board.as_mut() {
            if let Some(value) = env.get("HELIOS_BOARD_UPDATE_TOOL") {
                board.update_tool = value.into();
            }
            if let Some(value) = env.get("HELIOS_BOARD_IDENTITY_PATH") {
                board.identity_file = value.into();
            }
        }
        if let Some(value) = env.get("HELIOS_API_UI_DIR") {
            config.ui_dir = if value == "off" { None } else { Some(value.into()) };
        }
        if let Some(value) = env.get("HELIOS_API_GUEST_SECURITY") {
            config.guest_security = match value.as_str() {
                "on" | "1" | "true" => true,
                "off" | "0" | "false" => false,
                other => anyhow::bail!("HELIOS_API_GUEST_SECURITY must be on or off, got {other}"),
            };
        }
        if let Some(value) = env.get("HELIOS_API_CORS_ORIGIN") {
            config.cors_origin = Some(value.clone());
        }
        if let Some(value) = env.get("HELIOS_API_MAX_UPLOAD_BYTES") {
            config.max_upload_bytes = value.parse()?;
        }
        if let Some(value) = env.get(crate::auth_state::AUTH_FILE_ENV) {
            config.auth_file = value.into();
        }
        if let Some(value) = env.get("HELIOS_LEMNOSD_SOCKET") {
            config.lemnos_socket = value.into();
        }
        if let Some(value) = env.get("HELIOS_LIVE_STREAM_ID") {
            config.live_streams.stream_id = value.clone();
        }
        if let Some(value) = env.get("HELIOS_LIVE_STREAM_BACKFILL_MS") {
            config.live_streams.backfill_ms = value.parse()?;
        }
        if let Some(value) = env.get("HELIOS_API_STARTUP_FILE") {
            config.startup_file = value.into();
        }
        if let Some(value) = env.get("HELIOS_API_PREVIEW_SIZE") {
            let (width, height) =
                crate::camera_preview::PreviewSettings::parse_size(value).ok_or_else(|| anyhow::anyhow!("HELIOS_API_PREVIEW_SIZE must be <width>x<height> (16 to 4096 each), got {value:?}"))?;
            config.preview.width = width;
            config.preview.height = height;
        }
        if let Some(value) = env.get("HELIOS_API_PREVIEW_FPS") {
            let fps: f32 = value.parse()?;
            anyhow::ensure!(fps.is_finite() && (0.5..=60.0).contains(&fps), "HELIOS_API_PREVIEW_FPS must be 0.5 to 60, got {value}");
            config.preview.max_fps = fps;
        }
        if let Some(value) = env.get("HELIOS_API_PREVIEW_QUALITY") {
            let quality: u8 = value.parse()?;
            anyhow::ensure!((1..=100).contains(&quality), "HELIOS_API_PREVIEW_QUALITY must be 1 to 100, got {value}");
            config.preview.quality = quality;
        }
        if let Some(value) = env.get("HELIOS_API_PLAYBACK_MAX") {
            let max: usize = value.parse()?;
            anyhow::ensure!((1..=8).contains(&max), "HELIOS_API_PLAYBACK_MAX must be 1 to 8, got {value}");
            config.playback.max_players = max;
        }
        if let Some(value) = env.get("HELIOS_API_PLAYBACK_STREAM_DIR") {
            config.playback.stream_dir = value.into();
        }
        if let Some(value) = env.get("HELIOS_API_PEER_PORTS") {
            config.peer_ports = value
                .split(',')
                .map(|port| port.trim().parse::<u16>().map_err(|_| anyhow::anyhow!("HELIOS_API_PEER_PORTS takes port numbers, got {port:?}")))
                .collect::<anyhow::Result<_>>()?;
        }
        if let Some(value) = env.get("HELIOS_API_PEER_NETS") {
            config.peer_nets = value
                .split(',')
                .filter(|net| !net.trim().is_empty())
                .map(|net| crate::peers::Ipv4Net::parse(net).ok_or_else(|| anyhow::anyhow!("HELIOS_API_PEER_NETS takes a.b.c.d/len entries, got {net:?}")))
                .collect::<anyhow::Result<_>>()?;
        }
        if let Some(value) = env.get("HELIOS_API_PLAYBACK_ROOTS") {
            config.playback.roots = value.split(':').filter(|root| !root.is_empty()).map(PathBuf::from).collect();
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_overrides_defaults() {
        let config = ApiConfig::from_vars([
            ("HELIOS_API_BIND", "0.0.0.0:5800"),
            ("HELIOS_NODE_ID", "raze-1"),
            ("HELIOS_API_UPLOAD_DIR", "/data/updates"),
            ("HELIOS_API_CORS_ORIGIN", ""),
            ("HELIOS_API_UI_DIR", "off"),
            ("HELIOS_API_STARTUP_FILE", "/data/startup.toml"),
        ])
        .expect("config");
        assert_eq!(config.bind.port(), 5800);
        assert_eq!(config.node_id, "raze-1");
        assert_eq!(config.upload_dir, PathBuf::from("/data/updates"));
        assert_eq!(config.cors_origin, None);
        assert_eq!(config.ui_dir, None);
        assert_eq!(config.startup_file, PathBuf::from("/data/startup.toml"));
        assert_eq!(config.preview, crate::camera_preview::PreviewSettings::default());
    }

    #[test]
    fn peer_networks_are_read_from_the_environment() {
        let config = ApiConfig::from_vars([("HELIOS_API_PEER_NETS", "172.31.12.80/29, 10.63.90.0/24")]).expect("config");
        assert_eq!(config.peer_nets.len(), 2);
        assert!(ApiConfig::from_vars([("HELIOS_API_PEER_NETS", "172.31.12.80")]).is_err(), "a network needs its prefix");
        assert!(ApiConfig::from_vars([("HELIOS_API_PEER_NETS", "")]).expect("unset").peer_nets.is_empty());
    }

    #[test]
    fn the_defaults_are_the_image_paths() {
        let config = ApiConfig::from_vars(std::iter::empty::<(String, String)>()).expect("config");
        assert_eq!(config.state_dir, PathBuf::from("/var/lib/helios/api"));
        assert_eq!(config.upload_dir, PathBuf::from("/var/lib/helios/updates"));
        assert_eq!(config.ui_dir, Some(PathBuf::from("/usr/share/helios/ui")));
        assert_eq!(config.startup_file, PathBuf::from("/etc/helios/startup.toml"));
        assert_eq!(config.auth_file, PathBuf::from("/var/lib/helios/auth/auth.json"));
        assert_eq!(config.orion_socket, PathBuf::from("/run/orion/control.sock"));
        assert_eq!(config.lemnos_socket, PathBuf::from("/run/lemnos/lemnosd.sock"));
        assert_eq!(config.playback.stream_dir, PathBuf::from("/run/helios/streams"));
        let board = config.board.as_ref().expect("the image has a board contract");
        assert_eq!(board.update_tool, PathBuf::from("/usr/lib/board/update"));
        assert_eq!(board.identity_file, PathBuf::from("/run/board/identity.json"));
    }

    #[test]
    fn a_guest_has_no_board_and_its_paths_under_the_prefix() {
        let config = ApiConfig::from_vars([("HELIOS_LAYOUT_PROFILE", "guest")]).expect("config");
        assert!(config.board.is_none());
        assert_eq!(config.state_dir, PathBuf::from("/usr/local/var/lib/helios/api"));
        assert_eq!(config.upload_dir, PathBuf::from("/usr/local/var/lib/helios/updates"));
        assert_eq!(config.ui_dir, Some(PathBuf::from("/usr/local/share/helios/ui")));
        assert_eq!(config.startup_file, PathBuf::from("/usr/local/etc/helios/startup.toml"));
        assert_eq!(config.layout.plans_dir(), PathBuf::from("/usr/local/var/lib/helios/state/plans"));
    }

    #[test]
    fn a_guest_manages_its_own_units_and_keeps_security_off() {
        let image = ApiConfig::from_vars(std::iter::empty::<(String, String)>()).expect("config");
        assert_eq!(image.managed_units, MANAGED_UNITS);
        assert!(!image.guest_security);
        let guest = ApiConfig::from_vars([("HELIOS_LAYOUT_PROFILE", "guest"), ("HELIOS_API_GUEST_SECURITY", "on")]).expect("config");
        assert_eq!(guest.managed_units, GUEST_MANAGED_UNITS);
        assert!(guest.guest_security);
        assert!(ApiConfig::from_vars([("HELIOS_API_GUEST_SECURITY", "maybe")]).is_err());
    }

    #[test]
    fn board_overrides_apply_on_the_image_only() {
        let image = ApiConfig::from_vars([("HELIOS_BOARD_IDENTITY_PATH", "/tmp/identity.json")]).expect("config");
        assert_eq!(image.board.map(|board| board.identity_file), Some(PathBuf::from("/tmp/identity.json")));
        let guest = ApiConfig::from_vars([("HELIOS_LAYOUT_PROFILE", "guest"), ("HELIOS_BOARD_IDENTITY_PATH", "/tmp/identity.json")]).expect("config");
        assert!(guest.board.is_none());
    }

    #[test]
    fn preview_settings_from_env() {
        let config = ApiConfig::from_vars([("HELIOS_API_PREVIEW_SIZE", "320x200"), ("HELIOS_API_PREVIEW_FPS", "10"), ("HELIOS_API_PREVIEW_QUALITY", "60")]).expect("config");
        assert_eq!((config.preview.width, config.preview.height, config.preview.max_fps, config.preview.quality), (320, 200, 10.0, 60));
        assert!(ApiConfig::from_vars([("HELIOS_API_PREVIEW_SIZE", "big")]).is_err());
        assert!(ApiConfig::from_vars([("HELIOS_API_PREVIEW_QUALITY", "0")]).is_err());
    }
}
