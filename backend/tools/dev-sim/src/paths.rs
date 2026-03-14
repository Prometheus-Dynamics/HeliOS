use std::path::{Path, PathBuf};

use crate::AnyResult;
use crate::args::Args;

pub struct PathConfig {
    pub workspace: PathBuf,
    pub state_root: PathBuf,
    pub engine: EnginePaths,
    pub updater: UpdaterPaths,
    pub peripherals: PeripheralsPaths,
    pub api: ApiPaths,
    pub media: MediaPaths,
}

impl PathConfig {
    pub fn resolve(args: &Args) -> AnyResult<Self> {
        let workspace = workspace_root()?;
        let state_root = if args.state_dir.is_absolute() { args.state_dir.clone() } else { workspace.join(&args.state_dir) };

        let engine = EnginePaths::new(&state_root, args);
        let updater = UpdaterPaths::new(&state_root, args);
        let peripherals = PeripheralsPaths::new(&state_root);
        let api = ApiPaths::new(&state_root);
        let media = MediaPaths::new(&state_root);

        Ok(Self { workspace, state_root, engine, updater, peripherals, api, media })
    }

    pub fn runtime_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        dirs.extend(self.api.runtime_dirs());
        dirs.extend(self.engine.runtime_dirs());
        dirs.extend(self.updater.runtime_dirs());
        dirs.extend(self.peripherals.runtime_dirs());
        dirs.extend(self.media.runtime_dirs());
        dirs.push(self.state_root.join("run"));
        dirs
    }
}

pub struct EnginePaths {
    pub socket: PathBuf,
    pub journal: PathBuf,
    pub data_dir: PathBuf,
}

impl EnginePaths {
    fn new(state_root: &Path, args: &Args) -> Self {
        let socket = state_root.join("run/engine.sock");
        let journal = state_root.join("engine/journal/engine.log");
        let data_dir = state_root.join("engine");
        let _ = args;
        Self { socket, journal, data_dir }
    }

    pub fn runtime_dirs(&self) -> Vec<PathBuf> {
        vec![self.data_dir.clone(), self.data_dir.join("pipelines"), self.data_dir.join("processing_pipelines"), self.data_dir.join("snapshots"), self.data_dir.join("journal")]
    }

    pub fn socket_str(&self) -> String {
        self.socket.to_string_lossy().into_owned()
    }

    pub fn journal_str(&self) -> String {
        self.journal.to_string_lossy().into_owned()
    }

    pub fn data_dir_str(&self) -> String {
        self.data_dir.to_string_lossy().into_owned()
    }
}

pub struct UpdaterPaths {
    pub socket: PathBuf,
    pub journal: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub work_dir: PathBuf,
}

impl UpdaterPaths {
    fn new(state_root: &Path, args: &Args) -> Self {
        let socket = state_root.join("run/updater.sock");
        let journal = state_root.join("updater/journal/updater.log");
        let data_dir = state_root.join("updater");
        let cache_dir = data_dir.join("ota/cache");
        let work_dir = data_dir.join("ota/work");
        let _ = args;
        Self { socket, journal, data_dir, cache_dir, work_dir }
    }

    pub fn runtime_dirs(&self) -> Vec<PathBuf> {
        vec![self.data_dir.clone(), self.data_dir.join("journal"), self.data_dir.join("ota"), self.cache_dir.clone(), self.work_dir.clone()]
    }

    pub fn socket_str(&self) -> String {
        self.socket.to_string_lossy().into_owned()
    }

    pub fn journal_str(&self) -> String {
        self.journal.to_string_lossy().into_owned()
    }

    pub fn data_dir_str(&self) -> String {
        self.data_dir.to_string_lossy().into_owned()
    }

    pub fn cache_dir_str(&self) -> String {
        self.cache_dir.to_string_lossy().into_owned()
    }

    pub fn work_dir_str(&self) -> String {
        self.work_dir.to_string_lossy().into_owned()
    }
}

pub struct ApiPaths {
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    pub config_path: PathBuf,
    pub state_snapshot: PathBuf,
}

pub struct PeripheralsPaths {
    pub socket: PathBuf,
    pub state_dir: PathBuf,
    pub journal: PathBuf,
    pub log_path: PathBuf,
}

pub struct MediaPaths {
    pub root: PathBuf,
}

impl PeripheralsPaths {
    fn new(state_root: &Path) -> Self {
        let state_dir = state_root.join("peripherals");
        let socket = state_root.join("run/peripherals.sock");
        let journal = state_dir.join("journal/peripherals.log");
        let log_path = state_root.join("log/peripherals.log");
        Self { socket, state_dir, journal, log_path }
    }

    pub fn runtime_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.state_dir.clone()];
        if let Some(parent) = self.journal.parent() {
            dirs.push(parent.to_path_buf());
        }
        if let Some(parent) = self.log_path.parent() {
            dirs.push(parent.to_path_buf());
        }
        dirs
    }

    pub fn socket_str(&self) -> String {
        self.socket.to_string_lossy().into_owned()
    }

    pub fn state_dir_str(&self) -> String {
        self.state_dir.to_string_lossy().into_owned()
    }
}

impl MediaPaths {
    fn new(state_root: &Path) -> Self {
        let root = state_root.join("media");
        Self { root }
    }

    pub fn runtime_dirs(&self) -> Vec<PathBuf> {
        vec![self.root.clone(), self.root.join("images"), self.root.join("videos")]
    }

    pub fn root_str(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }
}

impl ApiPaths {
    fn new(state_root: &Path) -> Self {
        let data_dir = state_root.join("api");
        let log_dir = state_root.join("log");
        let config_path = state_root.join("config/dev-sim/helios-api.toml");
        let state_snapshot = data_dir.join("state.json");
        Self { data_dir, log_dir, config_path, state_snapshot }
    }

    pub fn runtime_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![
            self.data_dir.clone(),
            self.data_dir.join("pipelines"),
            self.data_dir.join("processing_pipelines"),
            self.data_dir.join("snapshots"),
            self.log_dir.clone(),
            self.log_dir.join("panics"),
            self.log_dir.join("api"),
            self.log_dir.join("engine"),
            self.log_dir.join("updater"),
        ];
        if let Some(parent) = self.config_path.parent() {
            dirs.push(parent.to_path_buf());
        }
        dirs
    }

    pub fn data_dir_str(&self) -> String {
        self.data_dir.to_string_lossy().into_owned()
    }

    pub fn log_dir_str(&self) -> String {
        self.log_dir.to_string_lossy().into_owned()
    }

    pub fn scoped_log_dir(&self, scope: &str) -> PathBuf {
        self.log_dir.join(scope)
    }

    pub fn config_path_str(&self) -> String {
        self.config_path.to_string_lossy().into_owned()
    }

    pub fn state_snapshot_str(&self) -> String {
        self.state_snapshot.to_string_lossy().into_owned()
    }
}

fn workspace_root() -> AnyResult<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().and_then(Path::parent).map(Path::to_path_buf).ok_or_else(|| "failed to determine workspace root".into())
}
