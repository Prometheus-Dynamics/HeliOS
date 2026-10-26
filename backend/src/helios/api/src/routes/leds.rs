//! The LED director (`GET`/`PUT /v1/device/leds`, `POST /v1/device/leds/locate` and
//! `POST /v1/device/leds/preview`): the settings, the live state, a locate and a state preview.
//! See `leds/mod.rs`; `docs/docs/leds.md` describes the states.

use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};

use crate::{
    SharedState,
    capabilities::{self, feature},
    error::{ApiError, ApiResult},
    leds::{
        settings::{LedSettings, RING_LEDS},
        states::{State as LedState, state_list},
    },
};

const LOCATE_SECONDS: u64 = 10;
const LOCATE_MAX_SECONDS: u64 = 60;
const PREVIEW_SECONDS: u64 = 4;
const PREVIEW_MAX_SECONDS: u64 = 30;

#[derive(Debug, Serialize)]
pub struct LedsView {
    pub settings: LedSettings,
    pub live: LiveView,
    pub device: DeviceView,
    pub preview: Option<PreviewView>,
    pub locate_until_ms: Option<u64>,
    pub states: Vec<crate::leds::states::StateInfo>,
}

#[derive(Debug, Serialize)]
pub struct LiveView {
    pub state: &'static str,
    pub look: Option<String>,
    pub brightness: f64,
    pub tags: usize,
    pub locked: bool,
    pub recording: bool,
    pub ambient_lux: Option<f64>,
    pub ring_leds: usize,
}

#[derive(Debug, Serialize)]
pub struct DeviceView {
    pub reachable: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PreviewView {
    pub state: &'static str,
    pub until_ms: u64,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LocateRequest {
    pub seconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewRequest {
    pub state: Option<String>,
    pub seconds: Option<u64>,
}

fn view(state: &SharedState) -> LedsView {
    let live = state.leds.live();
    let (preview, locate_until) = state.leds.overrides();
    LedsView {
        settings: state.leds.settings(),
        live: LiveView {
            state: live.state.name(),
            look: live.look,
            brightness: live.brightness,
            tags: live.tags,
            locked: live.locked,
            recording: live.recording,
            ambient_lux: live.lux,
            ring_leds: RING_LEDS,
        },
        device: DeviceView { reachable: live.reachable, error: live.error },
        preview: preview.map(|(state, until_ms)| PreviewView { state: state.name(), until_ms }),
        locate_until_ms: locate_until,
        states: state_list(),
    }
}

pub async fn get_leds(State(state): State<SharedState>) -> ApiResult<Json<LedsView>> {
    capabilities::require(&state, feature::LEDS).await?;
    Ok(Json(view(&state)))
}

pub async fn put_leds(State(state): State<SharedState>, Json(settings): Json<LedSettings>) -> ApiResult<Json<LedsView>> {
    capabilities::require(&state, feature::LEDS).await?;
    settings.validate()?;
    state.store.put_led_settings(&settings).await?;
    state.leds.set_settings(settings);
    Ok(Json(view(&state)))
}

pub async fn locate(State(state): State<SharedState>, request: Option<Json<LocateRequest>>) -> ApiResult<Json<LedsView>> {
    capabilities::require(&state, feature::LEDS).await?;
    let seconds = request.and_then(|Json(request)| request.seconds).unwrap_or(LOCATE_SECONDS);
    if !(1..=LOCATE_MAX_SECONDS).contains(&seconds) {
        return Err(ApiError::unprocessable("seconds is 1 to 60"));
    }
    state.leds.locate(seconds);
    Ok(Json(view(&state)))
}

pub async fn preview(State(state): State<SharedState>, Json(request): Json<PreviewRequest>) -> ApiResult<Json<LedsView>> {
    capabilities::require(&state, feature::LEDS).await?;
    let seconds = request.seconds.unwrap_or(PREVIEW_SECONDS);
    if !(1..=PREVIEW_MAX_SECONDS).contains(&seconds) {
        return Err(ApiError::unprocessable("seconds is 1 to 30"));
    }
    let preview = match request.state.as_deref() {
        None => None,
        Some(name) => Some(LedState::parse(name).ok_or_else(|| ApiError::unprocessable(format!("state is one of: {}", state_names())))?),
    };
    state.leds.preview(preview, seconds);
    Ok(Json(view(&state)))
}

fn state_names() -> String {
    LedState::ALL.iter().map(|state| state.name()).collect::<Vec<_>>().join(", ")
}
