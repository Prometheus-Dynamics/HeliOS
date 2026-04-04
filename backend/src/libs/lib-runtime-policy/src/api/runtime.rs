use crate::{BoundedU64Policy, PathPolicy, StringPolicy};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiServerPolicy {
    pub bind_addr: StringPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedApiServerPolicy {
    pub bind_addr: SocketAddr,
}

impl ApiServerPolicy {
    pub fn resolve(self) -> ResolvedApiServerPolicy {
        let bind_addr = self.bind_addr.resolve().parse::<SocketAddr>().unwrap_or_else(|_| "0.0.0.0:5800".parse().expect("default api bind addr"));
        ResolvedApiServerPolicy { bind_addr }
    }
}

pub const HELIOS_API_SERVER_POLICY: ApiServerPolicy = ApiServerPolicy { bind_addr: StringPolicy { env_var: "HELIOS_API__SERVER__BIND", default: "0.0.0.0:5800" } };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiPeripheralsClientPolicy {
    pub session_idle_ms: BoundedU64Policy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedApiPeripheralsClientPolicy {
    pub session_idle_ms: u64,
}

impl ApiPeripheralsClientPolicy {
    pub fn resolve(self) -> ResolvedApiPeripheralsClientPolicy {
        ResolvedApiPeripheralsClientPolicy { session_idle_ms: self.session_idle_ms.resolve() }
    }
}

pub const HELIOS_API_PERIPHERALS_CLIENT_POLICY: ApiPeripheralsClientPolicy =
    ApiPeripheralsClientPolicy { session_idle_ms: BoundedU64Policy { env_var: "HELIOS_PERIPHERALS_SESSION_IDLE_MS", default: 0, min: 0, max: 2_000 } };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiLightingTemplatesPolicy {
    pub template_dir: PathPolicy,
    pub dev_relative_dir: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedApiLightingTemplatesPolicy {
    pub template_dir: PathBuf,
}

impl ApiLightingTemplatesPolicy {
    pub fn resolve(self, cwd: Option<&Path>) -> ResolvedApiLightingTemplatesPolicy {
        if let Ok(raw) = std::env::var(self.template_dir.env_var) {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                return ResolvedApiLightingTemplatesPolicy { template_dir: PathBuf::from(trimmed) };
            }
        }

        if let Some(cwd) = cwd {
            let dev_dir = cwd.join(self.dev_relative_dir);
            if dev_dir.is_dir() {
                return ResolvedApiLightingTemplatesPolicy { template_dir: dev_dir };
            }
        }

        ResolvedApiLightingTemplatesPolicy { template_dir: PathBuf::from(self.template_dir.default) }
    }
}

pub const HELIOS_API_LIGHTING_TEMPLATES_POLICY: ApiLightingTemplatesPolicy = ApiLightingTemplatesPolicy {
    template_dir: PathPolicy { env_var: "HELIOS_API_LIGHTING_TEMPLATE_DIR", default: "/usr/share/helios/lighting-templates" },
    dev_relative_dir: "configs/lighting/templates",
};
