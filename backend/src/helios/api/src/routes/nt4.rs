//! NetworkTables (`GET`/`PUT /v1/device/nt4`): where helios-engine publishes pipeline outputs
//! for robot code. The settings are a file in the API's state directory (`nt4.json`) that
//! helios-engine reads when it changes; the connection state is on its `engine.runtime`
//! resource.

use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};

use crate::{
    SharedState,
    error::{ApiError, ApiResult},
    orion::label_values,
};

pub const DEFAULT_PREFIX: &str = "/helios";
const DEFAULT_PORT: u16 = 5810;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Nt4Settings {
    pub enabled: bool,
    /// FRC team number: the server is that team's roboRIO (`10.TE.AM.2`).
    pub team: Option<u32>,
    /// A server host instead of the team's roboRIO.
    pub server: Option<String>,
    pub port: u16,
    /// Topics are `<prefix>/<pipeline>/<output>`.
    pub prefix: String,
}

impl Default for Nt4Settings {
    fn default() -> Self {
        Self { enabled: true, team: None, server: None, port: DEFAULT_PORT, prefix: DEFAULT_PREFIX.into() }
    }
}

impl Nt4Settings {
    fn validate(&self) -> ApiResult<()> {
        if self.team.is_some_and(|team| !(1..=25_599).contains(&team)) {
            return Err(ApiError::unprocessable("team is an FRC team number, 1 to 25599"));
        }
        if self.server.as_deref().is_some_and(|server| server.len() > 253 || server.chars().any(|c| c.is_whitespace() || c == '/')) {
            return Err(ApiError::unprocessable("server is a host name or address"));
        }
        if self.port == 0 {
            return Err(ApiError::unprocessable("port is 1 to 65535"));
        }
        let prefix = self.prefix.trim();
        if prefix.len() > 120 || !prefix.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.')) {
            return Err(ApiError::unprocessable("prefix is a topic path such as /helios (letters, digits, / _ - .)"));
        }
        Ok(())
    }

    /// The server helios-engine connects to, when publishing is on and there is one.
    pub fn host(&self) -> Option<String> {
        if !self.enabled {
            return None;
        }
        if let Some(server) = self.server.as_deref().map(str::trim).filter(|server| !server.is_empty()) {
            return Some(server.to_string());
        }
        self.team.map(|team| format!("10.{}.{}.2", team / 100, team % 100))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Nt4State {
    pub settings: Nt4Settings,
    /// The server publishing goes to (`null`: off, or no team number or server set).
    pub server: Option<String>,
    /// helios-engine is connected to it.
    pub connected: bool,
    /// helios-engine is running and reports its connection.
    pub engine_reporting: bool,
}

async fn state_of(state: &SharedState, settings: Nt4Settings) -> Nt4State {
    let runtime = match state.orion.view().await {
        Ok(view) => view.resources.get(&format!("engine.runtime.{}", state.config.node_id)).cloned(),
        Err(_) => None,
    };
    let connected = runtime.as_ref().is_some_and(|runtime| label_values(&runtime.labels, "helios.nt4.connected").first() == Some(&"true"));
    let engine_reporting = runtime.as_ref().is_some_and(|runtime| !label_values(&runtime.labels, "helios.nt4.connected").is_empty());
    Nt4State { server: settings.host(), settings, connected, engine_reporting }
}

pub async fn get_nt4(State(state): State<SharedState>) -> ApiResult<Json<Nt4State>> {
    let settings = state.store.nt4_settings().await?;
    Ok(Json(state_of(&state, settings).await))
}

pub async fn put_nt4(State(state): State<SharedState>, Json(settings): Json<Nt4Settings>) -> ApiResult<Json<Nt4State>> {
    settings.validate()?;
    state.store.put_nt4_settings(&settings).await?;
    state.events.publish("nt4", serde_json::to_value(&settings).unwrap_or_default());
    Ok(Json(state_of(&state, settings).await))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_validate_and_resolve() {
        let team = Nt4Settings { team: Some(6390), ..Nt4Settings::default() };
        assert!(team.validate().is_ok());
        assert_eq!(team.host().as_deref(), Some("10.63.90.2"));
        assert!(Nt4Settings { team: Some(0), ..Nt4Settings::default() }.validate().is_err());
        assert!(Nt4Settings { server: Some("bad host".into()), ..Nt4Settings::default() }.validate().is_err());
        assert!(Nt4Settings { prefix: "/helios front".into(), ..Nt4Settings::default() }.validate().is_err());
        assert_eq!(Nt4Settings { enabled: false, ..team }.host(), None);
        assert_eq!(Nt4Settings::default().host(), None);
    }
}
