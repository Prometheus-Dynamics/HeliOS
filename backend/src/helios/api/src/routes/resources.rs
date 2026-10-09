//! Resources as Orion reports them (cameras, lemnosd's board devices, raw GPIO/PWM/I2C/SPI
//! access, engine and updater runtimes) and peripheral actions through helios-peripherals.

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
/// How often a running action looks for its result.
const ACTION_POLL: Duration = Duration::from_millis(50);

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
/// Kinds and arguments are helios-peripherals' resource actions through lemnosd:
///
/// - on lemnosd devices: `fan.override` (`pwm` 0-255 or `duty` 0-1, `duration_ms`
///   1000-600000; the fan goes back to the kernel governor when it ends), `fan.release`, and
///   `control.set` (`control`, `value`) for other devices' controls;
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

pub const DEVICE_ACTION_KINDS: &[&str] = &["fan.override", "fan.release", "control.set"];
pub const RAW_ACTION_KINDS: &[&str] =
    &["gpio.claim", "gpio.configure", "gpio.get", "gpio.set", "gpio.release", "pwm.claim", "pwm.configure", "pwm.release", "i2c.transfer", "spi.transfer", "raw.renew"];
/// The raw-access resource's type (one per node, from helios-peripherals).
pub const RAW_RESOURCE_TYPE: &str = "lemnos.raw";
/// The most `arg` fields an action may expand to (an SPI transfer of 64 KiB is one field).
const MAX_ARG_FIELDS: usize = 512;

/// The workload config of an action: `action.kind` and `arg.*`, with nested objects and arrays
/// as dotted paths (Orion's config decoding puts them back together) and arrays of byte values
/// (0-255) as one `Bytes` value.
pub fn action_config(request: &ActionRequest) -> ApiResult<WorkloadConfig> {
    if !DEVICE_ACTION_KINDS.contains(&request.kind.as_str()) && !RAW_ACTION_KINDS.contains(&request.kind.as_str()) {
        return Err(ApiError::bad_request(format!("unknown action kind {:?}; one of {}, {}", request.kind, DEVICE_ACTION_KINDS.join(", "), RAW_ACTION_KINDS.join(", "))));
    }
    let mut fields = Vec::new();
    for (key, value) in &request.arg {
        flatten(&format!("arg.{key}"), value, &mut fields)?;
    }
    if fields.len() > MAX_ARG_FIELDS {
        return Err(ApiError::bad_request(format!("arg has more than {MAX_ARG_FIELDS} values")));
    }
    let config = WorkloadConfig::new("helios.peripheral.resource_action.config.v1").field("action.kind", TypedConfigValue::String(request.kind.clone()));
    Ok(fields.into_iter().fold(config, |config, (key, value)| config.field(key, value)))
}

fn flatten(key: &str, value: &serde_json::Value, out: &mut Vec<(String, TypedConfigValue)>) -> ApiResult<()> {
    if key.split('.').any(|part| part.is_empty()) || key.len() > 256 {
        return Err(ApiError::bad_request(format!("invalid argument name {key:?}")));
    }
    match value {
        serde_json::Value::Null => {}
        serde_json::Value::Bool(value) => out.push((key.to_string(), TypedConfigValue::Bool(*value))),
        serde_json::Value::String(value) => out.push((key.to_string(), TypedConfigValue::String(value.clone()))),
        serde_json::Value::Number(number) => {
            let value = if let Some(value) = number.as_u64() {
                TypedConfigValue::UInt(value)
            } else if let Some(value) = number.as_i64() {
                TypedConfigValue::Int(value)
            } else {
                // JSON numbers are finite.
                TypedConfigValue::F64(number.as_f64().unwrap_or_default())
            };
            out.push((key.to_string(), value));
        }
        serde_json::Value::Array(items) => {
            let bytes: Option<Vec<u8>> = items.iter().map(|item| item.as_u64().and_then(|byte| u8::try_from(byte).ok())).collect();
            match bytes {
                Some(bytes) => out.push((key.to_string(), TypedConfigValue::Bytes(bytes))),
                None => {
                    for (index, item) in items.iter().enumerate() {
                        flatten(&format!("{key}.{index}"), item, out)?;
                    }
                }
            }
        }
        serde_json::Value::Object(entries) => {
            for (name, item) in entries {
                if name.parse::<usize>().is_ok() {
                    return Err(ApiError::bad_request(format!("invalid argument name {name:?} in {key}")));
                }
                flatten(&format!("{key}.{name}"), item, out)?;
            }
        }
    }
    Ok(())
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
/// its lease. helios-api runs one action at a time, so its own callers queue instead of
/// meeting each other's leases.
pub async fn action(State(state): State<SharedState>, Path(id): Path<String>, Json(request): Json<ActionRequest>) -> ApiResult<(StatusCode, Json<ActionResponse>)> {
    check_id(&id)?;
    run_action(&state, id, request).await
}

async fn run_action(state: &SharedState, id: String, request: ActionRequest) -> ApiResult<(StatusCode, Json<ActionResponse>)> {
    let config = action_config(&request)?;
    let _turn = state.action_lock.lock().await;
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
        tokio::time::sleep(ACTION_POLL).await;
        let Ok(view) = state.orion.view().await else { continue };
        if let Some(found) = view.resources.get(&id).map(|r| resource_dto(r, &view)).and_then(|dto| dto.action_result).filter(|r| r.observed_at_ms >= started_at && r.action_kind == request.kind) {
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

// --- raw GPIO, PWM, I2C and SPI ------------------------------------------------

/// One of helios-peripherals' raw claims (a lease on a GPIO line or PWM channel).
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
    /// The raw-access resource (for `/v1/peripherals/{id}/actions`).
    pub resource: String,
    pub available: bool,
    pub claims: Vec<ClaimDto>,
}

/// The claims in a raw-access resource's state (`claim.<id>.<field>`).
pub fn claims_of(record: &ResourceRecord) -> Vec<ClaimDto> {
    let Some(payload) = record.state.as_ref().and_then(|state| state.config.as_ref()).map(|config| &config.payload) else { return Vec::new() };
    let mut claims: BTreeMap<&str, ClaimDto> = BTreeMap::new();
    for (key, value) in payload {
        let Some((id, field)) = key.strip_prefix("claim.").and_then(|rest| rest.split_once('.')) else { continue };
        let claim = claims.entry(id).or_insert_with(|| ClaimDto { id: id.to_string(), ..ClaimDto::default() });
        let text = || if let TypedConfigValue::String(text) = value { Some(text.clone()) } else { None };
        let number = || if let TypedConfigValue::UInt(number) = value { Some(*number) } else { None };
        let flag = || if let TypedConfigValue::Bool(flag) = value { Some(*flag) } else { None };
        match field {
            "kind" => claim.kind = text().unwrap_or_default(),
            "target" => claim.target = text().unwrap_or_default(),
            "expires_at_ms" => claim.expires_at_ms = number().unwrap_or_default(),
            "held" => claim.held = flag().unwrap_or_default(),
            "direction" => claim.direction = text(),
            "value" => claim.value = flag(),
            "edges" => claim.edges = number(),
            "edge.rising" => claim.last_edge.get_or_insert(EdgeDto { rising: false, timestamp_ns: 0, seq: 0 }).rising = flag().unwrap_or_default(),
            "edge.timestamp_ns" => claim.last_edge.get_or_insert(EdgeDto { rising: false, timestamp_ns: 0, seq: 0 }).timestamp_ns = number().unwrap_or_default(),
            "edge.seq" => claim.last_edge.get_or_insert(EdgeDto { rising: false, timestamp_ns: 0, seq: 0 }).seq = number().unwrap_or_default(),
            "period_ns" => claim.period_ns = number(),
            "duty_ns" => claim.duty_ns = number(),
            "enabled" => claim.enabled = flag(),
            _ => {}
        }
    }
    claims.into_values().collect()
}

fn raw_resource(view: &StateView) -> Option<&ResourceRecord> {
    view.resources.values().find(|record| record.resource_type.as_str() == RAW_RESOURCE_TYPE && record.provider_id.as_str().starts_with(PERIPHERALS_PROVIDER_PREFIX))
}

/// `GET /v1/peripherals/io`: raw access through lemnosd and HeliOS's live claims.
pub async fn raw_io(State(state): State<SharedState>) -> ApiResult<Json<RawIoDto>> {
    let view = state.orion.view().await?;
    let record = raw_resource(&view).ok_or_else(|| ApiError::not_found("no raw-access resource: helios-peripherals has not reached lemnosd"))?;
    let dto = resource_dto(record, &view);
    Ok(Json(RawIoDto { available: dto.availability == "available", claims: claims_of(record), resource: dto.id }))
}

/// `POST /v1/peripherals/io/actions`: a raw action on this node's raw-access resource.
pub async fn raw_io_action(State(state): State<SharedState>, Json(request): Json<ActionRequest>) -> ApiResult<(StatusCode, Json<ActionResponse>)> {
    if !RAW_ACTION_KINDS.contains(&request.kind.as_str()) {
        return Err(ApiError::bad_request(format!("{:?} is not a raw action; one of {}", request.kind, RAW_ACTION_KINDS.join(", "))));
    }
    let view = state.orion.view().await?;
    let id = raw_resource(&view).map(|record| record.resource_id.to_string()).ok_or_else(|| ApiError::not_found("no raw-access resource: helios-peripherals has not reached lemnosd"))?;
    run_action(&state, id, request).await
}

/// The raw claims, for `gpio` change events (a claim made, renewed, set, an edge, released).
pub fn claims_digest(view: &StateView) -> BTreeMap<String, serde_json::Value> {
    raw_resource(view).map(|record| claims_of(record).into_iter().filter_map(|claim| Some((claim.id.clone(), serde_json::to_value(claim).ok()?))).collect()).unwrap_or_default()
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
        let request = |kind: &str, arg: serde_json::Value| ActionRequest { kind: kind.into(), arg: arg.as_object().cloned().unwrap_or_default() };
        let config = action_config(&request("fan.override", serde_json::json!({ "duty": 0.8, "duration_ms": 60_000 }))).expect("config");
        assert_eq!(config.payload.get("action.kind"), Some(&TypedConfigValue::String("fan.override".into())));
        assert_eq!(config.payload.get("arg.duty"), Some(&TypedConfigValue::F64(0.8)));
        assert_eq!(config.payload.get("arg.duration_ms"), Some(&TypedConfigValue::UInt(60_000)));
        let config = action_config(&request("control.set", serde_json::json!({ "control": "level", "value": 1.0 }))).expect("config");
        assert_eq!(config.payload.get("arg.control"), Some(&TypedConfigValue::String("level".into())));
        assert_eq!(config.payload.get("arg.value"), Some(&TypedConfigValue::F64(1.0)));
        assert!(action_config(&request("gpio.write", serde_json::json!({}))).is_err(), "unknown kind");
    }

    #[test]
    fn raw_action_args_expand_to_dotted_fields_with_byte_arrays_as_bytes() {
        let request = ActionRequest {
            kind: "i2c.transfer".into(),
            arg: serde_json::json!({ "bus": "i2c-1", "address": 80, "ops": [{ "write": [16] }, { "read": 2 }], "skip": null, "offset": -1 }).as_object().cloned().expect("object"),
        };
        let config = action_config(&request).expect("config");
        assert_eq!(config.payload.get("arg.bus"), Some(&TypedConfigValue::String("i2c-1".into())));
        assert_eq!(config.payload.get("arg.address"), Some(&TypedConfigValue::UInt(80)));
        assert_eq!(config.payload.get("arg.ops.0.write"), Some(&TypedConfigValue::Bytes(vec![16])));
        assert_eq!(config.payload.get("arg.ops.1.read"), Some(&TypedConfigValue::UInt(2)));
        assert_eq!(config.payload.get("arg.offset"), Some(&TypedConfigValue::Int(-1)));
        assert!(!config.payload.contains_key("arg.skip"));
        // Orion's config decoding gives the same JSON back.
        let decoded = orion::control_plane::config_json_value(&config.payload).expect("decode");
        assert_eq!(decoded["arg"]["ops"], serde_json::json!([{ "write": [16] }, { "read": 2 }]));

        let too_big =
            ActionRequest { kind: "spi.transfer".into(), arg: serde_json::json!({ "transfers": vec![serde_json::json!({ "rx_len": 1 }); MAX_ARG_FIELDS + 1] }).as_object().cloned().expect("object") };
        assert!(action_config(&too_big).is_err());
        let numbered = ActionRequest { kind: "gpio.claim".into(), arg: serde_json::json!({ "x": { "0": 1 } }).as_object().cloned().expect("object") };
        assert!(action_config(&numbered).is_err(), "numeric keys would read as array indices");
    }

    #[test]
    fn claims_come_from_the_raw_resource_state() {
        use orion::control_plane::{ResourceConfigState, ResourceState};
        let payload = BTreeMap::from([
            ("claims".to_string(), TypedConfigValue::UInt(2)),
            ("claim.gpio-1.kind".to_string(), TypedConfigValue::String("gpio".into())),
            ("claim.gpio-1.target".to_string(), TypedConfigValue::String("aux".into())),
            ("claim.gpio-1.held".to_string(), TypedConfigValue::Bool(true)),
            ("claim.gpio-1.direction".to_string(), TypedConfigValue::String("input".into())),
            ("claim.gpio-1.edges".to_string(), TypedConfigValue::UInt(3)),
            ("claim.gpio-1.edge.rising".to_string(), TypedConfigValue::Bool(true)),
            ("claim.gpio-1.edge.timestamp_ns".to_string(), TypedConfigValue::UInt(1234)),
            ("claim.gpio-1.edge.seq".to_string(), TypedConfigValue::UInt(3)),
            ("claim.pwm-2.kind".to_string(), TypedConfigValue::String("pwm".into())),
            ("claim.pwm-2.duty_ns".to_string(), TypedConfigValue::UInt(500)),
        ]);
        let record = ResourceRecord::builder(ResourceId::new("lemnos_raw_node_io"), RAW_RESOURCE_TYPE, orion::core::ProviderId::new("provider.peripherals.node"))
            .state(ResourceState::new(1).with_config(ResourceConfigState { payload }))
            .build();
        let claims = claims_of(&record);
        assert_eq!(claims.len(), 2);
        assert_eq!(claims[0].id, "gpio-1");
        assert_eq!((claims[0].kind.as_str(), claims[0].target.as_str(), claims[0].held, claims[0].edges), ("gpio", "aux", true, Some(3)));
        assert_eq!(claims[0].last_edge, Some(EdgeDto { rising: true, timestamp_ns: 1234, seq: 3 }));
        assert_eq!((claims[1].kind.as_str(), claims[1].duty_ns), ("pwm", Some(500)));
    }
}
