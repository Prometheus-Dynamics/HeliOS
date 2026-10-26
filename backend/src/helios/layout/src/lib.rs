//! The install layout: where HeliOS's files live on the device it runs on.
//!
//! HeliOS runs in two profiles. The **image** profile is the Raze owner image: HeliOS is the OS,
//! its files are the image's (`/usr`, `/etc/helios`, `/var/lib/helios`, ...), and the board
//! contract (identity, update writer, camera description) is present. The **guest** profile is
//! HeliOS installed as a package on another OS (SystemCore): every install path sits under one
//! prefix (`/usr/local` by default), there is no board contract, and host facts are read from the
//! host's own files.
//!
//! Every path HeliOS uses comes from a [`Layout`], never from a literal in the code. Loading
//! ([`Layout::load`], [`Layout::from_vars`]):
//!
//! 1. `HELIOS_LAYOUT` names an env-style file (`KEY=VALUE` per line, `#` comments). Without it,
//!    `/etc/helios/layout.env` is read when it exists.
//! 2. The process environment overrides the file, key by key.
//! 3. `HELIOS_LAYOUT_PROFILE=image|guest` picks the preset. Without it, `HELIOS_PREFIX` selects the
//!    guest and no prefix selects the image.
//! 4. `HELIOS_*_DIR` keys override single roots.
//!
//! Services keep their own, narrower overrides (`HELIOS_API_STATE_DIR`, `ENGINE_SOCKET`, ...).
//! Those are read after the layout and win over it.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

/// A file of `KEY=VALUE` lines naming the layout; read instead of [`DEFAULT_LAYOUT_FILE`].
pub const LAYOUT_FILE_ENV: &str = "HELIOS_LAYOUT";
/// `image` or `guest`.
pub const PROFILE_ENV: &str = "HELIOS_LAYOUT_PROFILE";
/// The guest's prefix (`/usr/local` when unset). Given without a profile, it selects the guest.
pub const PREFIX_ENV: &str = "HELIOS_PREFIX";
/// Read when `HELIOS_LAYOUT` is unset and the file exists.
pub const DEFAULT_LAYOUT_FILE: &str = "/etc/helios/layout.env";
/// The guest prefix when `HELIOS_PREFIX` does not say.
pub const GUEST_PREFIX: &str = "/usr/local";

/// The image's data partition (kept across updates).
const IMAGE_DATA_DIR: &str = "/data";
/// Runtime directories: tmpfs, owned by HeliOS's units on both profiles.
const RUN_DIR: &str = "/run/helios";
const ORION_RUN_DIR: &str = "/run/orion";
const LEMNOS_RUN_DIR: &str = "/run/lemnos";
/// The image's optional helper scripts (none on a guest).
const IMAGE_OPT_DIR: &str = "/opt";
/// Host files and directories HeliOS reads for host facts. They belong to the host OS, so they are
/// the same on every profile.
const OS_RELEASE_FILE: &str = "/etc/os-release";
const MACHINE_ID_FILE: &str = "/etc/machine-id";
const JOURNAL_DIR: &str = "/var/log/journal";
const REMOVABLE_MEDIA_ROOTS: &[&str] = &["/media", "/run/media"];

/// Which preset a [`Layout`] was built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// The Raze owner image: HeliOS is the OS.
    Image,
    /// A package on another OS, under one prefix.
    Guest,
}

impl Profile {
    pub fn name(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Guest => "guest",
        }
    }
}

/// The board's own files, present on the image only. Each is a field so a service can override
/// one (its env var) after the layout is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardContract {
    /// Written by the device package's identity tool.
    pub identity_file: PathBuf,
    /// The device package's update CLI.
    pub update_tool: PathBuf,
    /// The update CLI's state file.
    pub update_status: PathBuf,
    /// The update CLI's copy progress while an image is staged (0..1000).
    pub update_progress: PathBuf,
    /// The health check that keeps a trial boot.
    pub update_health: PathBuf,
    /// The hook run before a reboot.
    pub update_pre_reboot: PathBuf,
    /// The device package's description (`BOARD_PACKAGE_VERSION`, ...).
    pub device_package_env: PathBuf,
    /// The camera module's description (its lens and prior).
    pub camera_file: PathBuf,
    /// The camera stream list the board reads.
    pub camera_streams_file: PathBuf,
    /// The board's configuration directory (for snapshot globs).
    pub etc_dir: PathBuf,
}

impl BoardContract {
    /// The Raze image's board contract.
    pub fn image() -> Self {
        Self {
            identity_file: "/run/board/identity.json".into(),
            update_tool: "/usr/lib/board/update".into(),
            update_status: "/run/board/update.json".into(),
            update_progress: "/run/board/update/progress".into(),
            update_health: "/etc/board/update-health".into(),
            update_pre_reboot: "/etc/board/update.d/pre-reboot".into(),
            device_package_env: "/usr/lib/board/board-package.env".into(),
            camera_file: "/usr/share/board/raze/camera.toml".into(),
            camera_streams_file: "/run/board/camera-streams.json".into(),
            etc_dir: "/etc/board".into(),
        }
    }
}

/// Every install, state, runtime and board path HeliOS uses, by role.
///
/// Roots are fields so that one can be overridden; the concrete paths are methods, so callers
/// never join strings themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    profile: Profile,
    bin_dir: PathBuf,
    lib_dir: PathBuf,
    share_dir: PathBuf,
    etc_dir: PathBuf,
    config_dir: PathBuf,
    state_dir: PathBuf,
    log_dir: PathBuf,
    run_dir: PathBuf,
    data_dir: PathBuf,
    orion_run_dir: PathBuf,
    lemnos_run_dir: PathBuf,
    board: Option<BoardContract>,
}

impl Layout {
    /// The owner image's layout: today's paths, byte for byte.
    pub fn image() -> Self {
        Self {
            profile: Profile::Image,
            bin_dir: "/usr/bin".into(),
            lib_dir: "/usr/lib/helios".into(),
            share_dir: "/usr/share/helios".into(),
            etc_dir: "/etc".into(),
            config_dir: "/etc/helios".into(),
            state_dir: "/var/lib/helios".into(),
            log_dir: "/var/log/helios".into(),
            run_dir: RUN_DIR.into(),
            data_dir: IMAGE_DATA_DIR.into(),
            orion_run_dir: ORION_RUN_DIR.into(),
            lemnos_run_dir: LEMNOS_RUN_DIR.into(),
            board: Some(BoardContract::image()),
        }
    }

    /// The guest layout under `prefix` (`/usr/local` on SystemCore). No board contract.
    pub fn guest(prefix: impl AsRef<Path>) -> Self {
        let prefix = prefix.as_ref();
        let state_dir = prefix.join("var/lib/helios");
        Self {
            profile: Profile::Guest,
            bin_dir: prefix.join("bin"),
            lib_dir: prefix.join("lib/helios"),
            share_dir: prefix.join("share/helios"),
            etc_dir: prefix.join("etc"),
            config_dir: prefix.join("etc/helios"),
            log_dir: prefix.join("var/log/helios"),
            // Guest data is kept with the state.
            data_dir: state_dir.clone(),
            state_dir,
            run_dir: RUN_DIR.into(),
            orion_run_dir: ORION_RUN_DIR.into(),
            lemnos_run_dir: LEMNOS_RUN_DIR.into(),
            board: None,
        }
    }

    /// The layout of this process: `HELIOS_LAYOUT`, the environment, and the image default.
    pub fn load() -> Result<Self> {
        Self::from_vars(std::env::vars())
    }

    /// The layout for an environment, reading the layout file it names (or the default one).
    pub fn from_vars<I, K, V>(vars: I) -> Result<Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self::resolve(vars, Path::new(DEFAULT_LAYOUT_FILE))
    }

    fn resolve<I, K, V>(vars: I, default_file: &Path) -> Result<Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let process: BTreeMap<String, String> = vars.into_iter().map(|(key, value)| (key.into(), value.into())).filter(|(_, value): &(String, String)| !value.trim().is_empty()).collect();
        // The file first, then the environment over it.
        let mut merged = match process.get(LAYOUT_FILE_ENV) {
            Some(path) => parse_env_file(Path::new(path))?,
            None if default_file.is_file() => parse_env_file(default_file)?,
            None => BTreeMap::new(),
        };
        merged.extend(process);

        let profile = match merged.get(PROFILE_ENV).map(|value| value.trim()) {
            Some("image") => Profile::Image,
            Some("guest") => Profile::Guest,
            Some(other) => bail!("{PROFILE_ENV} must be image or guest, got {other:?}"),
            None if merged.contains_key(PREFIX_ENV) => Profile::Guest,
            None => Profile::Image,
        };
        let mut layout = match profile {
            Profile::Image => Self::image(),
            Profile::Guest => Self::guest(merged.get(PREFIX_ENV).map_or(GUEST_PREFIX, String::as_str)),
        };
        layout.apply_overrides(&merged);
        Ok(layout)
    }

    fn apply_overrides(&mut self, vars: &BTreeMap<String, String>) {
        override_dir(vars, "HELIOS_BIN_DIR", &mut self.bin_dir);
        override_dir(vars, "HELIOS_LIB_DIR", &mut self.lib_dir);
        override_dir(vars, "HELIOS_SHARE_DIR", &mut self.share_dir);
        override_dir(vars, "HELIOS_ETC_DIR", &mut self.etc_dir);
        override_dir(vars, "HELIOS_CONFIG_DIR", &mut self.config_dir);
        override_dir(vars, "HELIOS_STATE_DIR", &mut self.state_dir);
        // A guest keeps its data with its state, unless the data dir is named.
        if self.profile == Profile::Guest {
            self.data_dir = self.state_dir.clone();
        }
        override_dir(vars, "HELIOS_LOG_DIR", &mut self.log_dir);
        override_dir(vars, "HELIOS_RUN_DIR", &mut self.run_dir);
        override_dir(vars, "HELIOS_DATA_DIR", &mut self.data_dir);
        override_dir(vars, "HELIOS_ORION_RUN_DIR", &mut self.orion_run_dir);
        override_dir(vars, "HELIOS_LEMNOS_RUN_DIR", &mut self.lemnos_run_dir);
    }

    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// The board's files; `None` on a guest, which has no board contract of its own.
    pub fn board(&self) -> Option<&BoardContract> {
        self.board.as_ref()
    }

    // --- Install roots ---------------------------------------------------------------------

    pub fn bin_dir(&self) -> &Path {
        &self.bin_dir
    }

    /// One of HeliOS's binaries (`helios-api`, `orionctl`, ...).
    pub fn binary(&self, name: &str) -> PathBuf {
        self.bin_dir.join(name)
    }

    pub fn lib_dir(&self) -> &Path {
        &self.lib_dir
    }

    pub fn share_dir(&self) -> &Path {
        &self.share_dir
    }

    pub fn etc_dir(&self) -> &Path {
        &self.etc_dir
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// Pipeline history, auth, uploads and the other state HeliOS keeps across restarts.
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    pub fn log_dir(&self) -> &Path {
        &self.log_dir
    }

    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }

    /// Kept across updates: the image's data partition, or the state directory on a guest.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn orion_run_dir(&self) -> &Path {
        &self.orion_run_dir
    }

    pub fn lemnos_run_dir(&self) -> &Path {
        &self.lemnos_run_dir
    }

    // --- Sockets and runtime directories ---------------------------------------------------

    /// helios-engine's control socket.
    pub fn engine_socket(&self) -> PathBuf {
        self.run_dir.join("engine.sock")
    }

    /// helios-engine's live outputs (every tick, newline-delimited JSON).
    pub fn engine_live_socket(&self) -> PathBuf {
        self.run_dir.join("engine-live.sock")
    }

    /// The camera and playback frame sockets (Styx).
    pub fn streams_dir(&self) -> PathBuf {
        self.run_dir.join("streams")
    }

    pub fn orion_socket(&self) -> PathBuf {
        self.orion_run_dir.join("control.sock")
    }

    pub fn orion_stream_socket(&self) -> PathBuf {
        self.orion_run_dir.join("control-stream.sock")
    }

    pub fn lemnos_socket(&self) -> PathBuf {
        self.lemnos_run_dir.join("lemnosd.sock")
    }

    /// The peripherals' IPC directory when `HELIOS_IPC_DIR` does not say (the temp dir, on both
    /// profiles).
    pub fn ipc_dir(&self) -> PathBuf {
        std::env::temp_dir().join("helios-ipc")
    }

    /// The peripherals' stream directory in a scratch run (no `HELIOS_IPC_DIR`).
    pub fn scratch_peripheral_streams_dir(&self) -> PathBuf {
        std::env::temp_dir().join("helios-peripherals").join("streams")
    }

    // --- Shipped software ------------------------------------------------------------------

    /// The Daedalus plugins HeliOS ships.
    pub fn shipped_plugin_dir(&self) -> PathBuf {
        self.lib_dir.join("plugins/daedalus")
    }

    /// Every plugin directory the engine loads, shipped first.
    pub fn plugin_dirs(&self) -> Vec<PathBuf> {
        vec![self.shipped_plugin_dir(), self.state_dir.join("plugins/daedalus")]
    }

    /// The image's own helpers: the first-boot data setup and the scripts the image installs
    /// (none on a guest, which has no image of its own).
    pub fn image_helpers(&self) -> Vec<PathBuf> {
        match self.profile {
            Profile::Image => vec![self.lib_dir.join("data-setup"), Path::new(IMAGE_OPT_DIR).join("set-governor.sh"), Path::new(IMAGE_OPT_DIR).join("helios-update-issue.sh")],
            Profile::Guest => Vec::new(),
        }
    }

    /// The UI's static build, served by helios-api.
    pub fn ui_dir(&self) -> PathBuf {
        self.share_dir.join("ui")
    }

    // --- Configuration ---------------------------------------------------------------------

    /// The first-boot startup file (the default pipeline).
    pub fn startup_file(&self) -> PathBuf {
        self.config_dir.join("startup.toml")
    }

    /// The release version and build id, written at build time.
    pub fn version_file(&self) -> PathBuf {
        self.config_dir.join("version")
    }

    pub fn build_id_file(&self) -> PathBuf {
        self.config_dir.join("build-id")
    }

    /// The image's description (`HELIOS_COMMIT`, `IMAGE_VERSION`, ...).
    pub fn image_env_file(&self) -> PathBuf {
        self.etc_dir.join("default/helios-image.env")
    }

    /// Lemnos's configuration (its board definition).
    pub fn lemnos_config_dir(&self) -> PathBuf {
        self.etc_dir.join("lemnos")
    }

    pub fn lemnos_board_file(&self) -> PathBuf {
        self.lemnos_config_dir().join("board.toml")
    }

    /// The host's release (read on every profile).
    pub fn os_release_file(&self) -> PathBuf {
        PathBuf::from(OS_RELEASE_FILE)
    }

    /// The host's machine id (read on every profile).
    pub fn machine_id_file(&self) -> PathBuf {
        PathBuf::from(MACHINE_ID_FILE)
    }

    /// The host's journal (read on every profile).
    pub fn journal_dir(&self) -> PathBuf {
        PathBuf::from(JOURNAL_DIR)
    }

    /// Where removable media mount (the recorder looks for a USB stick here).
    pub fn removable_media_roots(&self) -> Vec<PathBuf> {
        REMOVABLE_MEDIA_ROOTS.iter().map(PathBuf::from).collect()
    }

    // --- State -----------------------------------------------------------------------------

    /// helios-api's own state (NetworkTables settings, ...).
    pub fn api_state_dir(&self) -> PathBuf {
        self.state_dir.join("api")
    }

    /// The device security state (password hash, token hashes).
    pub fn auth_file(&self) -> PathBuf {
        self.state_dir.join("auth/auth.json")
    }

    /// Where OTA uploads are kept until they are staged.
    pub fn upload_dir(&self) -> PathBuf {
        self.state_dir.join("updates")
    }

    /// The node catalog helios-engine writes at startup.
    pub fn catalog_file(&self) -> PathBuf {
        self.state_dir.join("state/catalog.json")
    }

    /// Each pipeline's full Daedalus plan.
    pub fn plans_dir(&self) -> PathBuf {
        self.state_dir.join("state/plans")
    }

    pub fn nt4_settings_file(&self) -> PathBuf {
        self.api_state_dir().join("nt4.json")
    }

    pub fn diagnostics_dir(&self) -> PathBuf {
        self.state_dir.join("diagnostics")
    }

    /// Host facts the snapshot bundle keeps (the board's name, team and peers).
    pub fn hostname_file(&self) -> PathBuf {
        self.state_dir.join("hostname")
    }

    pub fn team_file(&self) -> PathBuf {
        self.state_dir.join("team")
    }

    pub fn peers_file(&self) -> PathBuf {
        self.state_dir.join("peers.json")
    }

    /// Network configuration HeliOS writes for systemd-networkd.
    pub fn networkd_dir(&self) -> PathBuf {
        self.state_dir.join("networkd")
    }

    /// Recordings kept when no removable media is mounted.
    pub fn fallback_recordings_dir(&self) -> PathBuf {
        match self.profile {
            Profile::Image => self.data_dir.join("helios/recordings"),
            Profile::Guest => self.state_dir.join("recordings"),
        }
    }

    // --- Kept across updates ---------------------------------------------------------------

    /// The machine id persisted on the data partition.
    pub fn persistent_machine_id_file(&self) -> PathBuf {
        self.data_dir.join("identity/machine-id")
    }

    /// sshd's host keys (`HostKey` in sshd_config).
    pub fn ssh_host_key_dir(&self) -> PathBuf {
        self.data_dir.join("ssh")
    }
}

fn override_dir(vars: &BTreeMap<String, String>, key: &str, target: &mut PathBuf) {
    if let Some(value) = vars.get(key) {
        *target = PathBuf::from(value);
    }
}

/// `KEY=VALUE` lines of a layout file. Blank lines and `#` comments are skipped; surrounding
/// quotes are removed; empty values are dropped.
pub fn parse_env(text: &str) -> Result<BTreeMap<String, String>> {
    let mut vars = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            bail!("line {}: expected KEY=VALUE", index + 1);
        };
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
        if !value.is_empty() {
            vars.insert(key.trim().to_string(), value.to_string());
        }
    }
    Ok(vars)
}

fn parse_env_file(path: &Path) -> Result<BTreeMap<String, String>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read the layout file {}", path.display()))?;
    parse_env(&text).with_context(|| format!("layout file {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The layout for `vars`, with no layout file on the machine (the default file is absent).
    fn resolve(vars: &[(&str, &str)]) -> Result<Layout> {
        let dir = tempfile::tempdir().expect("tempdir");
        Layout::resolve(vars.iter().copied(), &dir.path().join("layout.env"))
    }

    #[test]
    fn image_defaults_are_the_literal_image_paths() {
        let layout = Layout::image();
        assert_eq!(layout.profile(), Profile::Image);
        assert_eq!(layout.bin_dir(), Path::new("/usr/bin"));
        assert_eq!(layout.binary("helios-api"), PathBuf::from("/usr/bin/helios-api"));
        assert_eq!(layout.shipped_plugin_dir(), PathBuf::from("/usr/lib/helios/plugins/daedalus"));
        assert_eq!(layout.plugin_dirs(), vec![PathBuf::from("/usr/lib/helios/plugins/daedalus"), PathBuf::from("/var/lib/helios/plugins/daedalus")]);
        assert_eq!(layout.image_helpers(), vec![PathBuf::from("/usr/lib/helios/data-setup"), PathBuf::from("/opt/set-governor.sh"), PathBuf::from("/opt/helios-update-issue.sh")]);
        assert_eq!(layout.ui_dir(), PathBuf::from("/usr/share/helios/ui"));
        assert_eq!(layout.config_dir(), Path::new("/etc/helios"));
        assert_eq!(layout.startup_file(), PathBuf::from("/etc/helios/startup.toml"));
        assert_eq!(layout.version_file(), PathBuf::from("/etc/helios/version"));
        assert_eq!(layout.build_id_file(), PathBuf::from("/etc/helios/build-id"));
        assert_eq!(layout.image_env_file(), PathBuf::from("/etc/default/helios-image.env"));
        assert_eq!(layout.lemnos_board_file(), PathBuf::from("/etc/lemnos/board.toml"));
        assert_eq!(layout.os_release_file(), PathBuf::from("/etc/os-release"));
        assert_eq!(layout.machine_id_file(), PathBuf::from("/etc/machine-id"));
        assert_eq!(layout.journal_dir(), PathBuf::from("/var/log/journal"));
        assert_eq!(layout.removable_media_roots(), vec![PathBuf::from("/media"), PathBuf::from("/run/media")]);
        assert_eq!(layout.log_dir(), Path::new("/var/log/helios"));
        assert_eq!(layout.state_dir(), Path::new("/var/lib/helios"));
        assert_eq!(layout.api_state_dir(), PathBuf::from("/var/lib/helios/api"));
        assert_eq!(layout.auth_file(), PathBuf::from("/var/lib/helios/auth/auth.json"));
        assert_eq!(layout.upload_dir(), PathBuf::from("/var/lib/helios/updates"));
        assert_eq!(layout.catalog_file(), PathBuf::from("/var/lib/helios/state/catalog.json"));
        assert_eq!(layout.plans_dir(), PathBuf::from("/var/lib/helios/state/plans"));
        assert_eq!(layout.nt4_settings_file(), PathBuf::from("/var/lib/helios/api/nt4.json"));
        assert_eq!(layout.diagnostics_dir(), PathBuf::from("/var/lib/helios/diagnostics"));
        assert_eq!(layout.hostname_file(), PathBuf::from("/var/lib/helios/hostname"));
        assert_eq!(layout.team_file(), PathBuf::from("/var/lib/helios/team"));
        assert_eq!(layout.peers_file(), PathBuf::from("/var/lib/helios/peers.json"));
        assert_eq!(layout.networkd_dir(), PathBuf::from("/var/lib/helios/networkd"));
        assert_eq!(layout.fallback_recordings_dir(), PathBuf::from("/data/helios/recordings"));
        assert_eq!(layout.data_dir(), Path::new("/data"));
        assert_eq!(layout.persistent_machine_id_file(), PathBuf::from("/data/identity/machine-id"));
        assert_eq!(layout.ssh_host_key_dir(), PathBuf::from("/data/ssh"));
        assert_eq!(layout.run_dir(), Path::new("/run/helios"));
        assert_eq!(layout.engine_socket(), PathBuf::from("/run/helios/engine.sock"));
        assert_eq!(layout.engine_live_socket(), PathBuf::from("/run/helios/engine-live.sock"));
        assert_eq!(layout.streams_dir(), PathBuf::from("/run/helios/streams"));
        assert_eq!(layout.orion_socket(), PathBuf::from("/run/orion/control.sock"));
        assert_eq!(layout.orion_stream_socket(), PathBuf::from("/run/orion/control-stream.sock"));
        assert_eq!(layout.lemnos_socket(), PathBuf::from("/run/lemnos/lemnosd.sock"));
    }

    #[test]
    fn image_board_contract_is_the_raze_paths() {
        let board = Layout::image().board().cloned().expect("the image has a board contract");
        assert_eq!(board, BoardContract::image());
        assert_eq!(board.identity_file, PathBuf::from("/run/board/identity.json"));
        assert_eq!(board.update_tool, PathBuf::from("/usr/lib/board/update"));
        assert_eq!(board.update_status, PathBuf::from("/run/board/update.json"));
        assert_eq!(board.update_progress, PathBuf::from("/run/board/update/progress"));
        assert_eq!(board.update_health, PathBuf::from("/etc/board/update-health"));
        assert_eq!(board.update_pre_reboot, PathBuf::from("/etc/board/update.d/pre-reboot"));
        assert_eq!(board.device_package_env, PathBuf::from("/usr/lib/board/board-package.env"));
        assert_eq!(board.camera_file, PathBuf::from("/usr/share/board/raze/camera.toml"));
        assert_eq!(board.camera_streams_file, PathBuf::from("/run/board/camera-streams.json"));
        assert_eq!(board.etc_dir, PathBuf::from("/etc/board"));
    }

    #[test]
    fn guest_preset_is_under_usr_local() {
        let layout = Layout::guest(GUEST_PREFIX);
        assert_eq!(layout.profile(), Profile::Guest);
        assert_eq!(layout.binary("helios-api"), PathBuf::from("/usr/local/bin/helios-api"));
        assert_eq!(layout.shipped_plugin_dir(), PathBuf::from("/usr/local/lib/helios/plugins/daedalus"));
        assert_eq!(layout.plugin_dirs(), vec![PathBuf::from("/usr/local/lib/helios/plugins/daedalus"), PathBuf::from("/usr/local/var/lib/helios/plugins/daedalus")]);
        assert_eq!(layout.ui_dir(), PathBuf::from("/usr/local/share/helios/ui"));
        assert_eq!(layout.config_dir(), Path::new("/usr/local/etc/helios"));
        assert_eq!(layout.startup_file(), PathBuf::from("/usr/local/etc/helios/startup.toml"));
        assert_eq!(layout.state_dir(), Path::new("/usr/local/var/lib/helios"));
        assert_eq!(layout.log_dir(), Path::new("/usr/local/var/log/helios"));
        assert_eq!(layout.image_env_file(), PathBuf::from("/usr/local/etc/default/helios-image.env"));
        assert_eq!(layout.lemnos_board_file(), PathBuf::from("/usr/local/etc/lemnos/board.toml"));
        assert_eq!(layout.fallback_recordings_dir(), PathBuf::from("/usr/local/var/lib/helios/recordings"));
        assert_eq!(layout.data_dir(), layout.state_dir());
        assert_eq!(layout.image_helpers(), Vec::<PathBuf>::new());
        // Host facts and runtime sockets are the same on every profile.
        assert_eq!(layout.os_release_file(), PathBuf::from("/etc/os-release"));
        assert_eq!(layout.engine_socket(), PathBuf::from("/run/helios/engine.sock"));
        assert_eq!(layout.orion_socket(), PathBuf::from("/run/orion/control.sock"));
        assert_eq!(layout.lemnos_socket(), PathBuf::from("/run/lemnos/lemnosd.sock"));
    }

    #[test]
    fn guest_has_no_board_contract() {
        assert!(Layout::guest(GUEST_PREFIX).board().is_none());
        assert!(resolve(&[(PROFILE_ENV, "guest")]).expect("layout").board().is_none());
        assert!(resolve(&[]).expect("layout").board().is_some());
    }

    #[test]
    fn prefix_alone_selects_the_guest_and_moves_its_roots() {
        let layout = resolve(&[(PREFIX_ENV, "/opt/helios")]).expect("layout");
        assert_eq!(layout.profile(), Profile::Guest);
        assert_eq!(layout.bin_dir(), Path::new("/opt/helios/bin"));
        assert_eq!(layout.state_dir(), Path::new("/opt/helios/var/lib/helios"));
    }

    #[test]
    fn profile_without_prefix_uses_the_usr_local_guest() {
        let layout = resolve(&[(PROFILE_ENV, "guest")]).expect("layout");
        assert_eq!(layout.bin_dir(), Path::new("/usr/local/bin"));
        let image = resolve(&[(PROFILE_ENV, "image"), (PREFIX_ENV, "/opt/helios")]).expect("layout");
        assert_eq!(image.bin_dir(), Path::new("/usr/bin"), "a prefix does not move the image");
    }

    #[test]
    fn unknown_profile_is_an_error() {
        let error = resolve(&[(PROFILE_ENV, "laptop")]).expect_err("profile");
        assert!(error.to_string().contains("image or guest"), "{error}");
    }

    #[test]
    fn env_overrides_one_root_and_keeps_the_rest() {
        let layout = resolve(&[(PROFILE_ENV, "image"), ("HELIOS_STATE_DIR", "/srv/helios/state"), ("HELIOS_RUN_DIR", "")]).expect("layout");
        assert_eq!(layout.state_dir(), Path::new("/srv/helios/state"));
        assert_eq!(layout.auth_file(), PathBuf::from("/srv/helios/state/auth/auth.json"));
        assert_eq!(layout.run_dir(), Path::new("/run/helios"), "empty values are ignored");
    }

    #[test]
    fn layout_file_is_read_and_the_environment_overrides_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("layout.env");
        std::fs::write(&file, "# SystemCore guest\nHELIOS_LAYOUT_PROFILE=guest\nHELIOS_PREFIX=\"/usr/local\"\nHELIOS_STATE_DIR='/data/helios'\n\nHELIOS_DATA_DIR=\n").expect("write");
        let path = file.to_string_lossy().into_owned();

        let from_file = Layout::from_vars([(LAYOUT_FILE_ENV, path.as_str())]).expect("layout");
        assert_eq!(from_file.profile(), Profile::Guest);
        assert_eq!(from_file.bin_dir(), Path::new("/usr/local/bin"));
        assert_eq!(from_file.state_dir(), Path::new("/data/helios"), "the file's value");
        assert_eq!(from_file.data_dir(), Path::new("/data/helios"), "an empty file value is dropped");

        let overridden = Layout::from_vars([(LAYOUT_FILE_ENV, path.as_str()), ("HELIOS_STATE_DIR", "/env/state"), ("HELIOS_PREFIX", "/opt/x")]).expect("layout");
        assert_eq!(overridden.state_dir(), Path::new("/env/state"), "the environment wins over the file");
        assert_eq!(overridden.bin_dir(), Path::new("/opt/x/bin"), "and over the file's prefix");
    }

    #[test]
    fn default_layout_file_applies_when_the_variable_is_unset() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("layout.env");
        std::fs::write(&file, "HELIOS_LAYOUT_PROFILE=guest\n").expect("write");
        let layout = Layout::resolve(std::iter::empty::<(String, String)>(), &file).expect("layout");
        assert_eq!(layout.profile(), Profile::Guest);
    }

    #[test]
    fn a_missing_named_layout_file_is_an_error() {
        let error = resolve(&[(LAYOUT_FILE_ENV, "/nonexistent/helios/layout.env")]).expect_err("missing file");
        assert!(format!("{error:#}").contains("layout.env"), "{error:#}");
    }

    #[test]
    fn a_malformed_layout_line_is_an_error_with_its_line() {
        let error = parse_env("HELIOS_STATE_DIR=/x\nnot a pair\n").expect_err("malformed");
        assert!(error.to_string().contains("line 2"), "{error}");
    }

    #[test]
    fn unknown_keys_in_the_file_are_ignored() {
        let vars = parse_env("FUTURE_KEY=1\nHELIOS_LAYOUT_PROFILE=image\n").expect("parse");
        assert_eq!(vars.get("FUTURE_KEY").map(String::as_str), Some("1"));
        assert_eq!(resolve(&[("FUTURE_KEY", "1")]).expect("layout").profile(), Profile::Image);
    }
}
