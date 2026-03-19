use std::path::PathBuf;

use clap::{ArgAction, Parser, ValueEnum};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SessionBackend {
    #[value(name = "mock")]
    Mock,
    #[value(name = "libcamera")]
    Libcamera,
}

impl SessionBackend {
    #[must_use]
    pub fn capture_backend(self) -> &'static str {
        match self {
            Self::Mock => "VirtualCaptureBackend",
            Self::Libcamera => "LibcameraCaptureBackend",
        }
    }

    #[must_use]
    pub fn matches_descriptor(self, backend: &str) -> bool {
        let backend = backend.trim();
        match self {
            Self::Mock => backend.eq_ignore_ascii_case("mock-backend") || backend.eq_ignore_ascii_case("VirtualCaptureBackend"),
            Self::Libcamera => backend.eq_ignore_ascii_case("LibcameraCaptureBackend"),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::Libcamera => "libcamera",
        }
    }
}

#[derive(Debug, Parser, Clone)]
#[command(name = "dev-sim", about = "Orchestrate the Helios API, engine, and updater locally")]
pub struct Args {
    /// Directory (relative to the workspace) where runtime state is stored.
    #[arg(long, default_value = "target/dev")]
    pub state_dir: PathBuf,

    /// Bind address used for all HTTP/WebSocket listeners.
    #[arg(long, default_value = "127.0.0.1")]
    pub bind_address: String,

    /// API HTTP port.
    #[arg(long, default_value_t = 5800)]
    pub http_port: u16,

    /// API WebSocket port.
    #[arg(long, default_value_t = 5802)]
    pub ws_port: u16,

    /// API metrics exporter port.
    #[arg(long, default_value_t = 19100)]
    pub api_metrics_port: u16,

    /// Capture backend to prefer when the API seeds default pipelines.
    #[arg(long = "session-backend", alias = "preview-backend", value_enum, default_value_t = SessionBackend::Mock)]
    pub session_backend: SessionBackend,

    /// Skip building binaries before launching them.
    #[arg(long)]
    pub skip_build: bool,

    /// Build binaries in release mode before launching them.
    #[arg(long)]
    pub release: bool,

    /// Exit the simulator if preview seeding fails.
    #[arg(long = "session-failure-fatal", alias = "preview-failure-fatal", default_value_t = true, action = ArgAction::Set)]
    pub session_failure_fatal: bool,

    /// Comma separated list of engine stream logging categories to enable (lifecycle, preset, manifest, supervisor, all).
    #[arg(long = "engine-stream-log", value_delimiter = ',')]
    pub engine_stream_log: Vec<String>,

    /// Enable debug logging for all simulator-managed services.
    #[arg(long)]
    pub debug_logs: bool,

    /// Run a one-shot scenario and shut services down when it completes.
    #[arg(long, value_enum)]
    pub scenario: Option<TestScenario>,

    /// Remove existing runtime state before launching services.
    #[arg(long)]
    pub fresh: bool,

    #[command(flatten)]
    pub scenario_options: ScenarioOptions,
}

#[derive(Debug, Clone, clap::Args)]
pub struct ScenarioOptions {
    /// Template identifier used when one-shot scenarios create pipelines.
    #[arg(long = "scenario-template", default_value = "crosshair_overlay")]
    pub scenario_template_id: String,

    /// Path to the image used by the simulated capture backend.
    #[arg(long = "scenario-image", default_value = "backend/images/test.jpg")]
    pub scenario_image_path: PathBuf,

    /// Human friendly name for the seeded media asset.
    #[arg(long = "scenario-media-name", default_value = "DevSim Image Source")]
    pub scenario_media_name: String,

    /// Number of frames to fetch from the capture session when running one-shot scenarios.
    #[arg(long = "scenario-frame-count", default_value_t = 100)]
    pub scenario_frame_count: u32,

    /// Expected frame rate (FPS hint) for the seeded media asset.
    #[arg(long = "scenario-frame-rate", default_value_t = 15.0)]
    pub scenario_frame_rate: f32,

    /// Whether the seeded media backend loops through its frame list.
    #[arg(long = "scenario-loop-mode", default_value_t = true)]
    pub scenario_loop_mode: bool,

    /// Rotation applied by the media backend (degrees, multiples of 90).
    #[arg(long = "scenario-rotation", default_value_t = 0)]
    pub scenario_rotation_degrees: i32,

    /// Directory where one-shot scenario outputs (frames, metrics) are stored. Defaults to <state_dir>/output when unset.
    #[arg(long = "scenario-output-dir")]
    pub scenario_output_dir: Option<PathBuf>,

    /// Save every captured frame instead of only the final one.
    #[arg(long = "scenario-save-all-frames", action = ArgAction::SetTrue)]
    pub scenario_save_all_frames: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TestScenario {
    #[value(name = "pipeline-metrics-roundtrip")]
    PipelineMetricsRoundtrip,
    #[value(name = "pipeline-image-one-shot")]
    PipelineImageOneShot,
}

impl TestScenario {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PipelineMetricsRoundtrip => "pipeline-metrics-roundtrip",
            Self::PipelineImageOneShot => "pipeline-image-one-shot",
        }
    }
}
