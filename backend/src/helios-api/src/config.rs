use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub bind_addr: SocketAddr,
}

impl ApiConfig {
    pub fn from_env() -> Self {
        let bind_addr = resolve_bind_addr().unwrap_or_else(|| "0.0.0.0:5800".parse().expect("default bind addr"));
        Self { bind_addr }
    }
}

fn resolve_bind_addr() -> Option<SocketAddr> {
    if let Ok(value) = std::env::var("HELIOS_API__SERVER__BIND") {
        return parse_bind_value(&value);
    }
    if let Ok(value) = std::env::var("HELIOS_API_BIND") {
        return parse_bind_value(&value);
    }
    if let Ok(value) = std::env::var("PORT") {
        if let Ok(port) = value.trim().parse::<u16>() {
            return Some(SocketAddr::from(([0, 0, 0, 0], port)));
        }
        return parse_bind_value(&value);
    }
    None
}

fn parse_bind_value(value: &str) -> Option<SocketAddr> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<SocketAddr>().ok()
}
