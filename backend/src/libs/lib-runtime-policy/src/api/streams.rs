use crate::{BoundedU64Policy, OptionalBoundedF64Policy, OptionalBoundedU64Policy};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ApiStreamsPolicy {
    pub cache_ms: BoundedU64Policy,
    pub mjpeg_poll_ms: OptionalBoundedU64Policy,
    pub preview_poll_ms: OptionalBoundedU64Policy,
    pub mjpeg_interval_ms: OptionalBoundedU64Policy,
    pub snapshot_interval_ms: BoundedU64Policy,
    pub preview_max_fps: OptionalBoundedF64Policy,
    pub preview_outage_ms: BoundedU64Policy,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedApiStreamsPolicy {
    pub cache_ms: u64,
    pub mjpeg_poll_ms: Option<u64>,
    pub preview_poll_ms: Option<u64>,
    pub mjpeg_interval_ms: Option<u64>,
    pub snapshot_interval_ms: u64,
    pub preview_max_fps: Option<f64>,
    pub preview_outage_ms: u64,
}

impl ApiStreamsPolicy {
    pub fn resolve(self) -> ResolvedApiStreamsPolicy {
        ResolvedApiStreamsPolicy {
            cache_ms: self.cache_ms.resolve(),
            mjpeg_poll_ms: self.mjpeg_poll_ms.resolve(),
            preview_poll_ms: self.preview_poll_ms.resolve(),
            mjpeg_interval_ms: self.mjpeg_interval_ms.resolve(),
            snapshot_interval_ms: self.snapshot_interval_ms.resolve(),
            preview_max_fps: self.preview_max_fps.resolve(),
            preview_outage_ms: self.preview_outage_ms.resolve(),
        }
    }
}

pub const HELIOS_API_STREAMS_POLICY: ApiStreamsPolicy = ApiStreamsPolicy {
    cache_ms: BoundedU64Policy { env_var: "HELIOS_API_STREAMS_CACHE_MS", default: 750, min: 0, max: 5_000 },
    mjpeg_poll_ms: OptionalBoundedU64Policy { env_var: "HELIOS_MJPEG_POLL_MS", min: 1, max: 100 },
    preview_poll_ms: OptionalBoundedU64Policy { env_var: "HELIOS_PREVIEW_POLL_MS", min: 1, max: 100 },
    mjpeg_interval_ms: OptionalBoundedU64Policy { env_var: "HELIOS_MJPEG_INTERVAL_MS", min: 1, max: 100 },
    snapshot_interval_ms: BoundedU64Policy { env_var: "HELIOS_MJPEG_INTERVAL_MS", default: 33, min: 20, max: 500 },
    preview_max_fps: OptionalBoundedF64Policy { env_var: "HELIOS_PREVIEW_MAX_FPS", min: 1.0, max: 120.0 },
    preview_outage_ms: BoundedU64Policy { env_var: "HELIOS_PREVIEW_OUTAGE_MS", default: 15_000, min: 1_000, max: 15_000 },
};
