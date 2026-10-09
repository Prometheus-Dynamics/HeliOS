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

/// The resources helios-peripherals publishes (lemnosd's board devices, cameras).
pub async fn peripherals(State(state): State<SharedState>) -> ApiResult<Json<Vec<ResourceDto>>> {
    let view = state.orion.view().await?;
    Ok(Json(view.resources.values().filter(|r| r.provider_id.as_str().starts_with(PERIPHERALS_PROVIDER_PREFIX)).map(|r| resource_dto(r, &view)).collect()))
}

// --- actions -------------------------------------------------------------------

/// A peripheral action: `{"kind": "fan.override", "arg": {"duty": 0.8, "duration_ms": 60000}}`.
/// Kinds and arguments are helios-peripherals' resource actions on lemnosd devices:
/// `fan.override` (`pwm` 0-255 or `duty` 0-1, `duration_ms` 1000-600000; the fan goes back to the
/// kernel governor when it ends), `fan.release`, and `control.set` (`control`, `value`) for other
/// devices' controls.
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
    pub pwm: Option<u64>,
    pub duty: Option<f64>,
    pub duration_ms: Option<u64>,
    pub control: Option<String>,
    pub value: Option<f64>,
}

pub const ACTION_KINDS: &[&str] = &["fan.override", "fan.release", "control.set"];

pub fn action_config(request: &ActionRequest) -> ApiResult<WorkloadConfig> {
    if !ACTION_KINDS.contains(&request.kind.as_str()) {
        return Err(ApiError::bad_request(format!("unknown action kind {:?}; one of {}", request.kind, ACTION_KINDS.join(", "))));
    }
    let mut config = WorkloadConfig::new("helios.peripheral.resource_action.config.v1").field("action.kind", TypedConfigValue::String(request.kind.clone()));
    let arg = &request.arg;
    for (name, value) in [("pwm", arg.pwm), ("duration_ms", arg.duration_ms)] {
        if let Some(value) = value {
            config = config.field(format!("arg.{name}"), TypedConfigValue::UInt(value));
        }
    }
    for (name, value) in [("duty", arg.duty), ("value", arg.value)] {
        if let Some(value) = value {
            if !value.is_finite() {
                return Err(ApiError::bad_request(format!("arg.{name} must be a finite number")));
            }
            config = config.field(format!("arg.{name}"), TypedConfigValue::F64(value));
        }
    }
    if let Some(control) = &arg.control {
        config = config.field("arg.control", TypedConfigValue::String(control.clone()));
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
        "helios-peripherals publishing the hwmon fan (Lemnos fan.read, and the timed fan.override / fan.release that always returns to the kernel thermal governor) as an Orion resource with fan actions",
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
        let config = action_config(&ActionRequest { kind: "fan.override".into(), arg: ActionArgs { duty: Some(0.8), duration_ms: Some(60_000), ..ActionArgs::default() } }).expect("config");
        assert_eq!(config.payload.get("action.kind"), Some(&TypedConfigValue::String("fan.override".into())));
        assert_eq!(config.payload.get("arg.duty"), Some(&TypedConfigValue::F64(0.8)));
        assert_eq!(config.payload.get("arg.duration_ms"), Some(&TypedConfigValue::UInt(60_000)));
        let config = action_config(&ActionRequest { kind: "control.set".into(), arg: ActionArgs { control: Some("level".into()), value: Some(1.0), ..ActionArgs::default() } }).expect("config");
        assert_eq!(config.payload.get("arg.control"), Some(&TypedConfigValue::String("level".into())));
        assert!(action_config(&ActionRequest { kind: "gpio.write".into(), arg: ActionArgs::default() }).is_err(), "raw GPIO is lemnosd's");
        assert!(action_config(&ActionRequest { kind: "control.set".into(), arg: ActionArgs { value: Some(f64::NAN), ..ActionArgs::default() } }).is_err());
    }
}
