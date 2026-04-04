use std::net::SocketAddr;

use lib_runtime_policy::HELIOS_API_SERVER_POLICY;

#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub bind_addr: SocketAddr,
}

impl ApiConfig {
    pub fn from_env() -> Self {
        let bind_addr = HELIOS_API_SERVER_POLICY.resolve().bind_addr;
        Self { bind_addr }
    }
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use super::ApiConfig;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().expect("env lock poisoned")
    }

    #[test]
    fn api_config_prefers_canonical_bind_env() {
        let _lock = env_lock();
        unsafe {
            std::env::set_var("HELIOS_API__SERVER__BIND", "127.0.0.1:5999");
        }
        let config = ApiConfig::from_env();
        assert_eq!(config.bind_addr.to_string(), "127.0.0.1:5999");
        unsafe {
            std::env::remove_var("HELIOS_API__SERVER__BIND");
        }
    }

    #[test]
    fn api_config_ignores_removed_bind_alias_envs() {
        let _lock = env_lock();
        unsafe {
            std::env::remove_var("HELIOS_API__SERVER__BIND");
            std::env::set_var("HELIOS_API_BIND", "127.0.0.1:6001");
            std::env::set_var("PORT", "6002");
        }
        let config = ApiConfig::from_env();
        assert_eq!(config.bind_addr.to_string(), "0.0.0.0:5800");
        unsafe {
            std::env::remove_var("HELIOS_API_BIND");
            std::env::remove_var("PORT");
        }
    }
}
