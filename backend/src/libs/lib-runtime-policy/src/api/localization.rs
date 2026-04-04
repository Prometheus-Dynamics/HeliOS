use crate::filesystem::PathPolicy;
use crate::{BoundedU64Policy, BoundedUsizePolicy};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiLocalizationPolicy {
    pub media_seed_dir: PathPolicy,
    pub max_map_upload_mb: BoundedU64Policy,
    pub solve_cache_entries: BoundedUsizePolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedApiLocalizationPolicy {
    pub media_seed_dir: PathBuf,
    pub max_map_upload_bytes: u64,
    pub solve_cache_entries: usize,
}

impl ApiLocalizationPolicy {
    pub fn resolve(self) -> ResolvedApiLocalizationPolicy {
        let max_map_upload_mb = self.max_map_upload_mb.resolve();
        ResolvedApiLocalizationPolicy {
            media_seed_dir: self.media_seed_dir.resolve(),
            max_map_upload_bytes: max_map_upload_mb.saturating_mul(1024 * 1024),
            solve_cache_entries: self.solve_cache_entries.resolve(),
        }
    }
}

pub const HELIOS_API_LOCALIZATION_POLICY: ApiLocalizationPolicy = ApiLocalizationPolicy {
    media_seed_dir: PathPolicy { env_var: "HELIOS_API_MEDIA_SEED_DIR", default: "/usr/share/helios/media" },
    max_map_upload_mb: BoundedU64Policy { env_var: "HELIOS_API_MAX_MAP_UPLOAD_MB", default: 5, min: 1, max: 1024 },
    solve_cache_entries: BoundedUsizePolicy { env_var: "HELIOS_API_LOCALIZATION_SOLVE_CACHE_ENTRIES", default: 64, min: 1, max: 4096 },
};
