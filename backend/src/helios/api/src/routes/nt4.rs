//! NetworkTables (`GET`/`PUT /v1/device/nt4`): where helios-engine publishes pipeline outputs
//! for robot code. The settings are what the user set on this device (`nt4.json`, which
//! helios-engine reads when it changes). The response also says what they resolve to, the team
//! and the server robot code is at, each with where it came from: the user, the robot controller
//! HeliOS found, or nothing. The connection state is on helios-engine's `engine.runtime` resource.

use axum::{Json, extract::State};
use helios_robot::{ControllerFacts, Effective, Nt4Settings, resolve};
use serde::Serialize;

use crate::{
    SharedState,
    error::{ApiError, ApiResult},
    orion::label_values,
};

fn validate(settings: &Nt4Settings) -> ApiResult<()> {
    if settings.team.is_some_and(|team| !helios_robot::TEAM_RANGE.contains(&team)) {
        return Err(ApiError::unprocessable("team is an FRC team number, 1 to 25599"));
    }
    if settings.server.as_deref().is_some_and(|server| server.len() > 253 || server.chars().any(|c| c.is_whitespace() || c == '/')) {
        return Err(ApiError::unprocessable("server is a host name or address"));
    }
    if settings.port == 0 {
        return Err(ApiError::unprocessable("port is 1 to 65535"));
    }
    let prefix = settings.prefix.trim();
    if prefix.len() > 120 || !prefix.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.')) {
        return Err(ApiError::unprocessable("prefix is a topic path such as /helios (letters, digits, / _ - .)"));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Nt4State {
    /// What the user set on this device.
    pub settings: Nt4Settings,
    /// The team and the server robot code is at, and where each came from.
    pub effective: Effective,
    /// The robot controller HeliOS found, when it found one.
    pub controller: Option<ControllerFacts>,
    /// The server publishing goes to (`null`: off, or none known); `effective.nt4_server.value`.
    pub server: Option<String>,
    /// helios-engine is connected to it.
    pub connected: bool,
    /// helios-engine is running and reports its connection.
    pub engine_reporting: bool,
}

async fn state_of(state: &SharedState, settings: Nt4Settings) -> ApiResult<Nt4State> {
    let controller = state.store.controller_facts().await?;
    let effective = resolve(&settings, controller.as_ref());
    let runtime = match state.orion.view().await {
        Ok(view) => view.resources.get(&format!("engine.runtime.{}", state.config.node_id)).cloned(),
        Err(_) => None,
    };
    let connected = runtime.as_ref().is_some_and(|runtime| label_values(&runtime.labels, "helios.nt4.connected").first() == Some(&"true"));
    let engine_reporting = runtime.as_ref().is_some_and(|runtime| !label_values(&runtime.labels, "helios.nt4.connected").is_empty());
    Ok(Nt4State { server: effective.nt4_server.value.clone(), effective, controller, settings, connected, engine_reporting })
}

pub async fn get_nt4(State(state): State<SharedState>) -> ApiResult<Json<Nt4State>> {
    let settings = state.store.nt4_settings().await?;
    Ok(Json(state_of(&state, settings).await?))
}

pub async fn put_nt4(State(state): State<SharedState>, Json(settings): Json<Nt4Settings>) -> ApiResult<Json<Nt4State>> {
    validate(&settings)?;
    state.store.put_nt4_settings(&settings).await?;
    state.events.publish("nt4", serde_json::to_value(&settings).unwrap_or_default());
    Ok(Json(state_of(&state, settings).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_validated() {
        assert!(validate(&Nt4Settings { team: Some(6390), ..Nt4Settings::default() }).is_ok());
        assert!(validate(&Nt4Settings { team: Some(0), ..Nt4Settings::default() }).is_err());
        assert!(validate(&Nt4Settings { server: Some("bad host".into()), ..Nt4Settings::default() }).is_err());
        assert!(validate(&Nt4Settings { prefix: "/helios front".into(), ..Nt4Settings::default() }).is_err());
        assert!(validate(&Nt4Settings { port: 0, ..Nt4Settings::default() }).is_err());
    }
}
