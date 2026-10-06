//! Resources as Orion reports them (cameras, GPIO, PWM, buses, engine and updater runtimes) and
//! peripheral actions through helios-peripherals.

use std::{collections::BTreeMap, time::Duration};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use orion::{
    control_plane::{DesiredState, DesiredStateMutation, LeaseRecord, LeaseState, ResourceRecord, TypedConfigValue, WorkloadConfig, WorkloadRecord},
    core::{ArtifactId, NodeId, ResourceId, WorkloadId},
};
use serde::{Deserialize, Serialize};

use crate::{
    SharedState,
    error::{ApiError, ApiResult},
    host::now_ms,
    orion::{StateView, config_value_json, enum_name, label_map},
};

use super::check_id;

const PERIPHERALS_PROVIDER_PREFIX: &str = "provider.peripherals.";
pub const RESOURCE_ACTION_RUNTIME: &str = "helios.peripheral.resource_action.v1";
const RESOURCE_ACTION_ARTIFACT: &str = "artifact.helios.peripheral-action";
const ACTION_WAIT: Duration = Duration::from_secs(5);

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
        labels: labels.into_iter().filter_map(|(k, v)| k.strip_prefix("helios.label.").map(|k| (k.to_string(), v))).collect(),
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

/// The resources helios-peripherals publishes (GPIO, PWM, I2C, SPI, USB, cameras).
pub async fn peripherals(State(state): State<SharedState>) -> ApiResult<Json<Vec<ResourceDto>>> {
    let view = state.orion.view().await?;
    Ok(Json(view.resources.values().filter(|r| r.provider_id.as_str().starts_with(PERIPHERALS_PROVIDER_PREFIX)).map(|r| resource_dto(r, &view)).collect()))
}

// --- actions -------------------------------------------------------------------

/// A peripheral action: `{"kind": "gpio.write", "arg": {"high": true}}`. Kinds and arguments are
/// helios-peripherals' resource actions.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ActionRequest {
    pub kind: String,
    #[serde(default)]
    pub arg: ActionArgs,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ActionArgs {
    pub high: Option<bool>,
    pub direction: Option<String>,
    pub initial_high: Option<bool>,
    pub enabled: Option<bool>,
    pub period_ns: Option<u64>,
    pub duty_cycle_ns: Option<u64>,
    pub len: Option<u64>,
    pub bytes: Option<Vec<u8>>,
    pub write: Option<Vec<u8>>,
    pub read_len: Option<u64>,
}

pub const ACTION_KINDS: &[&str] = &[
    "gpio.read",
    "gpio.write",
    "gpio.configure_direction",
    "pwm.enable",
    "pwm.set_period_ns",
    "pwm.set_duty_cycle_ns",
    "pwm.configure",
    "i2c.read",
    "i2c.write",
    "i2c.write_read",
    "spi.transfer",
    "spi.write",
];

pub fn action_config(request: &ActionRequest) -> ApiResult<WorkloadConfig> {
    if !ACTION_KINDS.contains(&request.kind.as_str()) {
        return Err(ApiError::bad_request(format!("unknown action kind {:?}; one of {}", request.kind, ACTION_KINDS.join(", "))));
    }
    let mut config = WorkloadConfig::new("helios.peripheral.resource_action.config.v1").field("action.kind", TypedConfigValue::String(request.kind.clone()));
    let arg = &request.arg;
    let bools = [("high", arg.high), ("initial_high", arg.initial_high), ("enabled", arg.enabled)];
    for (name, value) in bools {
        if let Some(value) = value {
            config = config.field(format!("arg.{name}"), TypedConfigValue::Bool(value));
        }
    }
    let uints = [("period_ns", arg.period_ns), ("duty_cycle_ns", arg.duty_cycle_ns), ("len", arg.len), ("read_len", arg.read_len)];
    for (name, value) in uints {
        if let Some(value) = value {
            config = config.field(format!("arg.{name}"), TypedConfigValue::UInt(value));
        }
    }
    for (name, value) in [("bytes", &arg.bytes), ("write", &arg.write)] {
        if let Some(value) = value {
            config = config.field(format!("arg.{name}"), TypedConfigValue::Bytes(value.clone()));
        }
    }
    if let Some(direction) = &arg.direction {
        if direction != "input" && direction != "output" {
            return Err(ApiError::bad_request("arg.direction is input or output"));
        }
        config = config.field("arg.direction", TypedConfigValue::String(direction.clone()));
    }
    Ok(config)
}

#[derive(Debug, Serialize)]
pub struct ActionResponse {
    pub workload_id: String,
    pub resource: String,
    pub done: bool,
    pub result: Option<ActionResult>,
}

/// Run one action: a short-lived resource-action workload holding the resource's lease. Waits up
/// to five seconds for helios-peripherals to report the result, then removes the workload and
/// its lease.
pub async fn action(State(state): State<SharedState>, Path(id): Path<String>, Json(request): Json<ActionRequest>) -> ApiResult<(StatusCode, Json<ActionResponse>)> {
    check_id(&id)?;
    let config = action_config(&request)?;
    let view = state.orion.view().await?;
    let resource = view.resources.get(&id).ok_or_else(|| ApiError::not_found(format!("no resource {id}")))?;
    if !resource.provider_id.as_str().starts_with(PERIPHERALS_PROVIDER_PREFIX) {
        return Err(ApiError::bad_request(format!("{id} is not a helios-peripherals resource")));
    }
    if let Some(holder) = view.leases.get(&id).and_then(|lease| lease.holder_workload_id.as_ref()) {
        return Err(ApiError::conflict(format!("{id} is leased by {holder}")));
    }
    let node = state.config.node_id.clone();
    let started_at = now_ms();
    let workload_id = format!("action.{}.{started_at}", id.replace(':', "_"));
    let workload = WorkloadRecord::builder(WorkloadId::new(workload_id.clone()), RESOURCE_ACTION_RUNTIME, ArtifactId::new(RESOURCE_ACTION_ARTIFACT))
        .desired_state(DesiredState::Running)
        .assigned_to(NodeId::new(node.clone()))
        .bind_resource(ResourceId::new(id.clone()), NodeId::new(node.clone()))
        .config(config)
        .build();
    let lease = LeaseRecord::builder(ResourceId::new(id.clone())).lease_state(LeaseState::Leased).holder_node(NodeId::new(node)).holder_workload(WorkloadId::new(workload_id.clone())).build();
    state.orion.apply(vec![DesiredStateMutation::PutWorkload(workload), DesiredStateMutation::PutLease(lease)]).await?;

    let deadline = tokio::time::Instant::now() + ACTION_WAIT;
    let mut result = None;
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let Ok(view) = state.orion.view().await else { continue };
        if let Some(found) = view.resources.get(&id).map(|r| resource_dto(r, &view)).and_then(|dto| dto.action_result).filter(|r| r.observed_at_ms >= started_at) {
            result = Some(found);
            break;
        }
    }
    let cleanup = vec![DesiredStateMutation::RemoveWorkload(WorkloadId::new(workload_id.clone())), DesiredStateMutation::RemoveLease(ResourceId::new(id.clone()))];
    if let Err(error) = state.orion.apply(cleanup).await {
        tracing::warn!(workload = %workload_id, error = %error.message, "failed to remove finished peripheral action");
    }
    let done = result.is_some();
    let status = if done { StatusCode::OK } else { StatusCode::ACCEPTED };
    Ok((status, Json(ActionResponse { workload_id, resource: id, done, result })))
}

// --- not yet backed ------------------------------------------------------------

pub async fn fan() -> ApiError {
    ApiError::not_available(
        "fan control is not available",
        "helios-peripherals publishing the hwmon fan (Lemnos fan.read / fan.set_pwm / fan.set_mode) as an Orion resource with fan actions, and a decision on how it shares control with the kernel thermal governor",
    )
}

pub async fn leds() -> ApiError {
    ApiError::not_available("LED control is not available", "LED resources (the Raze status LEDs) published by helios-peripherals with colour/pattern actions")
}

pub async fn imu() -> ApiError {
    ApiError::not_available("IMU readings are not available", "an IMU resource in helios-peripherals (Lemnos IIO driver) publishing orientation and accepting rate/filter configuration")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_config_encodes_typed_args() {
        let config =
            action_config(&ActionRequest { kind: "pwm.configure".into(), arg: ActionArgs { period_ns: Some(20_000), duty_cycle_ns: Some(5_000), enabled: Some(true), ..ActionArgs::default() } })
                .expect("config");
        assert_eq!(config.payload.get("action.kind"), Some(&TypedConfigValue::String("pwm.configure".into())));
        assert_eq!(config.payload.get("arg.period_ns"), Some(&TypedConfigValue::UInt(20_000)));
        assert_eq!(config.payload.get("arg.enabled"), Some(&TypedConfigValue::Bool(true)));
        assert!(action_config(&ActionRequest { kind: "fan.set".into(), arg: ActionArgs::default() }).is_err());
        assert!(action_config(&ActionRequest { kind: "gpio.configure_direction".into(), arg: ActionArgs { direction: Some("sideways".into()), ..ActionArgs::default() } }).is_err());
    }
}
