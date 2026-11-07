//! Resources as Orion reports them (cameras, lemnosd's board devices, engine and updater runtimes),
//! the fan override (`fan_override`) and raw GPIO/PWM/I2C/SPI access (`lemnos_raw`).

use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use orion::{
    control_plane::{ActionRequest as OrionActionRequest, ActionResult as OrionActionResult, ActionState, ActionTarget, ResourceRecord, TypedConfigValue},
    core::ResourceId,
};
use serde::{Deserialize, Serialize};

use crate::{
    SharedState,
    error::{ApiError, ApiResult},
    host::now_ms,
    orion::{StateView, config_value_json, enum_name, label_map},
};

use super::check_id;

/// The providers whose resources are peripherals: the engine's cameras (`provider.cameras.*`)
/// and, until it is gone from every node, helios-peripherals (`provider.peripherals.*`).
const PERIPHERAL_PROVIDER_PREFIXES: [&str; 2] = ["provider.cameras.", "provider.peripherals."];
/// The action's own budget, counted from when the node accepts it (Orion's `deadline_ms`).
const ACTION_DEADLINE_MS: u64 = 10_000;
static ACTION_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A resource's latest action result, as the API reports it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ActionResult {
    pub action_kind: String,
    pub status: String,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
    pub observed_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ResourceDto {
    pub id: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    pub name: String,
    pub kind: Option<String>,
    pub provider: String,
    pub health: String,
    pub availability: String,
    pub ownership: String,
    pub lease_state: String,
    /// Workloads holding a lease on it.
    pub leased_by: Vec<String>,
    /// For derived resources: the workload it was realized for.
    pub workload: Option<String>,
    pub capabilities: Vec<String>,
    pub labels: BTreeMap<String, String>,
    pub endpoints: Vec<String>,
    pub action_result: Option<ActionResult>,
    pub state: Option<BTreeMap<String, serde_json::Value>>,
}

pub fn resource_dto(record: &ResourceRecord, view: &StateView) -> ResourceDto {
    let labels = label_map(&record.labels);
    let id = record.resource_id.to_string();
    let leased_by = view.leases.get(&id).and_then(|lease| lease.holder_workload_id.as_ref()).map(|holder| vec![holder.to_string()]).unwrap_or_default();
    let state = record.state.as_ref();
    ResourceDto {
        name: labels.get("helios.display_name").cloned().unwrap_or_else(|| id.clone()),
        kind: labels.get("helios.kind").cloned(),
        resource_type: record.resource_type.to_string(),
        provider: record.provider_id.to_string(),
        health: enum_name(record.health),
        availability: enum_name(record.availability),
        ownership: enum_name(&record.ownership_mode),
        lease_state: enum_name(record.lease_state),
        leased_by,
        workload: record.realized_for_workload_id.as_ref().map(ToString::to_string),
        capabilities: record.capabilities.iter().map(|c| c.capability_id.to_string()).collect(),
        endpoints: record.endpoints.clone(),
        action_result: state.and_then(|s| s.action_result.as_ref()).map(|result| ActionResult {
            action_kind: result.action_kind.clone(),
            status: enum_name(&result.status),
            data: result.data.as_ref().map(config_value_json),
            error: result.error.clone(),
            observed_at_ms: state.map(|s| s.observed_at_ms).unwrap_or_default(),
        }),
        state: state.and_then(|s| s.config.as_ref()).map(|config| config.payload.iter().map(|(k, v)| (k.clone(), config_value_json(v))).collect()),
        // HeliOS's own labels without their prefix, and a lemnos-orion device's `lemnos.*` labels
        // as they are (its class, model, board, units and control ranges).
        labels: labels.into_iter().filter_map(|(k, v)| match k.strip_prefix("helios.label.") { Some(k) => Some((k.to_string(), v)), None => k.starts_with("lemnos.").then_some((k, v)) }).collect(),
        id,
    }
}

/// Health per resource, for change events.
pub fn digest(view: &StateView) -> BTreeMap<String, serde_json::Value> {
    view.resources
        .values()
        .filter(|record| !record.resource_type.as_str().starts_with("execution.") && !record.resource_type.as_str().starts_with("system.update"))
        .map(|record| {
            let dto = resource_dto(record, view);
            (dto.id.clone(), serde_json::json!({ "type": dto.resource_type, "health": dto.health, "availability": dto.availability, "lease_state": dto.lease_state, "leased_by": dto.leased_by }))
        })
        .collect()
}

#[derive(Debug, Deserialize, Default)]
pub struct ResourceFilter {
    #[serde(rename = "type")]
    pub resource_type: Option<String>,
}

pub async fn list(State(state): State<SharedState>, Query(filter): Query<ResourceFilter>) -> ApiResult<Json<Vec<ResourceDto>>> {
    let view = state.orion.view().await?;
    Ok(Json(view.resources.values().filter(|r| filter.resource_type.as_deref().is_none_or(|t| r.resource_type.as_str() == t)).map(|r| resource_dto(r, &view)).collect()))
}

pub async fn get_one(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<Json<ResourceDto>> {
    check_id(&id)?;
    let view = state.orion.view().await?;
    view.resources.get(&id).map(|r| Json(resource_dto(r, &view))).ok_or_else(|| ApiError::not_found(format!("no resource {id}")))
}

/// The peripheral resources: the cameras the engine serves and
/// lemnosd's devices from the Lemnos bridge (`lemnos.device`), whose readings and statuses are
/// their status entries (`state`).
pub async fn peripherals(State(state): State<SharedState>) -> ApiResult<Json<Vec<ResourceDto>>> {
    let view = state.orion.view().await?;
    let mut readings = lemnos_readings(&state).await?;
    let resources = view
        .resources
        .values()
        .filter(|r| PERIPHERAL_PROVIDER_PREFIXES.iter().any(|prefix| r.provider_id.as_str().starts_with(prefix)) || r.resource_type.as_str() == LEMNOS_DEVICE_TYPE)
        .map(|r| {
            let mut dto = resource_dto(r, &view);
            if let Some(entries) = readings.remove(&dto.id) {
                dto.state.get_or_insert_with(BTreeMap::new).extend(entries);
            }
            if r.resource_type.as_str() == LEMNOS_DEVICE_TYPE && is_lemnos_fan(r) {
                let active = state.fan_overrides.active(&dto.id);
                let entries = dto.state.get_or_insert_with(BTreeMap::new);
                entries.insert("fan.override.active".into(), serde_json::Value::Bool(active.is_some()));
                if let Some(active) = active {
                    entries.insert("fan.override.until_ms".into(), active.until_ms.into());
                    entries.insert("fan.override.duty".into(), active.duty.into());
                }
            }
            dto
        })
        .collect();
    Ok(Json(resources))
}

/// The latest status of each Lemnos bridge resource, by resource id: its `status`, `reason`,
/// `read_us`, channel readings and `control.<name>` values.
async fn lemnos_readings(state: &SharedState) -> ApiResult<BTreeMap<String, BTreeMap<String, serde_json::Value>>> {
    let mut by_resource: BTreeMap<String, BTreeMap<String, serde_json::Value>> = BTreeMap::new();
    for entry in state.orion.status_entries().await? {
        if let orion::control_plane::StatusSubject::Resource(id) = &entry.subject
            && id.as_str().starts_with("lemnos.")
        {
            by_resource.entry(id.to_string()).or_default().insert(entry.key, config_value_json(&entry.value));
        }
    }
    Ok(by_resource)
}

// --- actions -------------------------------------------------------------------

/// A peripheral action: `{"kind": "fan.override", "arg": {"duty": 0.8, "duration_ms": 60000}}`.
/// Kinds and arguments are helios-peripherals' resource actions through lemnosd:
///
/// - on a fan of Lemnos's Orion bridge (`lemnos.<board>.<device>`, `lemnos.class=fan`):
///   `fan.override` (`pwm` 0-255 or `duty` 0-1, `duration_ms` 1000-600000; the fan goes back to
///   the kernel governor when it ends) and `fan.release`, timed here (`fan_override`) over the
///   bridge's `set` and `release`;
/// - on the raw-access resource (`lemnos.raw`, also `/v1/peripherals/io`): `gpio.claim`,
///   `gpio.configure`, `gpio.get`, `gpio.set`, `gpio.release`, `pwm.claim`, `pwm.configure`,
///   `pwm.release`, `i2c.transfer`, `spi.transfer` and `raw.renew` (docs/docs/api/http.md, "Raw
///   GPIO, PWM, I2C and SPI").
///
/// `arg` is any JSON object; helios-peripherals checks it per kind.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ActionRequest {
    pub kind: String,
    #[serde(default)]
    pub arg: serde_json::Map<String, serde_json::Value>,
}

/// The Lemnos bridge's resource type (`lemnosd`'s board devices, one `lemnos.<board>.<device>` each;
/// Lemnos `docs/orion.md`). Its readings are status entries, merged into the resource's `state`.
pub const LEMNOS_DEVICE_TYPE: &str = "lemnos.device";
/// What a raw action's answer names as its resource (raw access is helios-api's own, not Orion's).
pub const RAW_RESOURCE: &str = "lemnos.raw";
#[derive(Debug, Serialize)]
pub struct ActionResponse {
    /// The id Orion ran the action under.
    pub action_id: String,
    pub resource: String,
    pub done: bool,
    pub result: Option<ActionResult>,
}

/// `POST /v1/peripherals/{id}/actions`: `fan.override` or `fan.release` on a Lemnos fan
/// (`fan_override`). The answer is 200 with the final result, or 202 with `done: false` and the
/// latest result when the wait runs out.
pub async fn action(State(state): State<SharedState>, Path(id): Path<String>, Json(request): Json<ActionRequest>) -> ApiResult<(StatusCode, Json<ActionResponse>)> {
    check_id(&id)?;
    if !crate::fan_override::is_fan_action(&request.kind) {
        return Err(ApiError::bad_request(format!(
            "unknown action kind {:?}; one of {}, {} (raw access is POST /v1/peripherals/io/actions)",
            request.kind,
            crate::fan_override::OVERRIDE,
            crate::fan_override::RELEASE
        )));
    }
    let view = state.orion.view().await?;
    let resource = view.resources.get(&id).ok_or_else(|| ApiError::not_found(format!("no resource {id}")))?;
    if resource.resource_type.as_str() != LEMNOS_DEVICE_TYPE || !is_lemnos_fan(resource) {
        return Err(ApiError::bad_request(format!("{id} is not a fan")));
    }
    let result = crate::fan_override::run(&state.fan_overrides, state.orion.clone(), &id, &request.kind, &request.arg).await?;
    Ok(respond(id, &result))
}

/// The Orion request for an action on resource `id`: a fresh id, the deadline, and the arguments.
pub fn action_request(id: &str, kind: &str, args: BTreeMap<String, TypedConfigValue>) -> OrionActionRequest {
    let seq = ACTION_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let action_id = format!("helios-api.{}.{seq}", now_ms());
    let mut request = OrionActionRequest::new(action_id, ActionTarget::Resource(ResourceId::new(id)), kind).with_deadline_ms(ACTION_DEADLINE_MS);
    request.args = args;
    request
}

/// The HTTP answer for an action `result` on `resource`.
fn respond(resource: String, result: &OrionActionResult) -> (StatusCode, Json<ActionResponse>) {
    let done = result.state.is_terminal();
    let status = if done { StatusCode::OK } else { StatusCode::ACCEPTED };
    (status, Json(ActionResponse { action_id: result.action_id.clone(), resource, done, result: Some(action_result_dto(result)) }))
}

/// A `lemnos.device` of class `fan`.
fn is_lemnos_fan(resource: &ResourceRecord) -> bool {
    label_map(&resource.labels).get("lemnos.class").map(String::as_str) == Some(crate::fan_override::FAN_CLASS)
}

/// An Orion result as the API reports an action: `applied` (succeeded), `failed` (failed,
/// rejected, timed out or cancelled, with the reason), or `running`. `data` is the `value` output, or the
/// whole output when there are other entries.
fn action_result_dto(result: &OrionActionResult) -> ActionResult {
    let (status, error) = match &result.state {
        ActionState::Succeeded => ("applied", None),
        ActionState::Failed { reason } | ActionState::Rejected { reason } => ("failed", Some(reason.clone())),
        ActionState::TimedOut => ("failed", Some("timed out".to_string())),
        ActionState::Cancelled => ("failed", Some("cancelled".to_string())),
        ActionState::Accepted | ActionState::Running { .. } => ("running", None),
    };
    let data = match result.output.get("value") {
        Some(value) => Some(config_value_json(value)),
        None if result.output.is_empty() => None,
        None => Some(serde_json::Value::Object(result.output.iter().map(|(key, value)| (key.clone(), config_value_json(value))).collect())),
    };
    ActionResult { action_kind: result.name.clone(), status: status.to_string(), data, error, observed_at_ms: result.updated_at_ms }
}

// --- raw GPIO, PWM, I2C and SPI ------------------------------------------------

/// One of helios-api's raw claims (a lease on a GPIO line or PWM channel, `lemnos_raw`).
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct ClaimDto {
    pub id: String,
    /// `gpio` or `pwm`.
    pub kind: String,
    /// The line or channel as claimed: a board or kernel name, or `chip:offset` / `chip:channel`.
    pub target: String,
    /// When the lease runs out unless an action on it (or `raw.renew`) renews it.
    pub expires_at_ms: u64,
    /// `false` while lemnosd reconnects (the claim is taken again when it is back).
    pub held: bool,
    pub direction: Option<String>,
    /// An output's level.
    pub value: Option<bool>,
    /// Edges seen since the claim was made, and the last one.
    pub edges: Option<u64>,
    pub last_edge: Option<EdgeDto>,
    pub period_ns: Option<u64>,
    pub duty_ns: Option<u64>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct EdgeDto {
    pub rising: bool,
    pub timestamp_ns: u64,
    pub seq: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RawIoDto {
    /// The board definition admits HeliOS (lemnosd's `raw_clients`).
    pub available: bool,
    /// helios-api is connected to lemnosd (it connects on the first raw action).
    pub connected: bool,
    pub claims: Vec<ClaimDto>,
}

/// `GET /v1/peripherals/io`: raw access now (helios-api's own connection to lemnosd, `lemnos_raw`).
pub async fn raw_io(State(state): State<SharedState>) -> Json<RawIoDto> {
    let status = state.raw.status();
    Json(RawIoDto { available: status.allowed, connected: status.connected, claims: status.claims })
}

/// `POST /v1/peripherals/io/actions`: a raw action, straight to lemnosd. A refused or failed action
/// is 200 with `status: "failed"` and lemnosd's reason, like any other action.
pub async fn raw_io_action(State(state): State<SharedState>, Json(request): Json<ActionRequest>) -> ApiResult<Json<ActionResponse>> {
    let action = match crate::lemnos_raw::RawAction::from_kind(&request.kind, serde_json::Value::Object(request.arg)) {
        None => return Err(ApiError::bad_request(format!("{:?} is not a raw action; one of {}", request.kind, crate::lemnos_raw::RAW_ACTIONS.join(", ")))),
        Some(Err(error)) => return Err(ApiError::bad_request(error)),
        Some(Ok(action)) => action,
    };
    let seq = ACTION_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let action_id = format!("helios-api.{}.{seq}", now_ms());
    let (status, data, error) = match state.raw.run(action).await {
        Ok(data) => ("applied", data, None),
        Err(error) => ("failed", None, Some(error)),
    };
    let result = ActionResult { action_kind: request.kind, status: status.into(), data, error, observed_at_ms: now_ms() };
    Ok(Json(ActionResponse { action_id, resource: RAW_RESOURCE.into(), done: true, result: Some(result) }))
}

// --- not yet backed ------------------------------------------------------------

pub async fn fan() -> ApiError {
    ApiError::not_available("fan control is not available here", "the fan's Lemnos resource: POST /v1/peripherals/lemnos.<board>.fan/actions with fan.override or fan.release")
}

pub async fn leds() -> ApiError {
    ApiError::not_available("LED control is not available", "LED resources (the Raze status LEDs) published by helios-peripherals with colour/pattern actions")
}

pub async fn imu() -> ApiError {
    ApiError::not_available("IMU readings are not available", "an IMU resource in helios-peripherals (Lemnos IIO driver) publishing orientation and accepting rate/filter configuration")
}
