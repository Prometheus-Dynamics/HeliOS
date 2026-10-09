//! Resources as Orion reports them (cameras, lemnosd's board devices, raw GPIO/PWM/I2C/SPI
//! access, engine and updater runtimes) and peripheral actions through helios-peripherals.

use std::{collections::BTreeMap, time::Duration};

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
    orion::{ActionCall, StateView, config_value_json, enum_name, label_map},
};

use super::check_id;

const PERIPHERALS_PROVIDER_PREFIX: &str = "provider.peripherals.";
/// How long a request waits for an action's result (Orion's `wait_ms`); a longer action still runs,
/// and the answer is 202 with its latest result.
const ACTION_WAIT: Duration = Duration::from_secs(5);
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

/// The arguments of an action as Orion carries them: typed values under their names, nested
/// objects and arrays as dotted names (`ops.0.write`), arrays of byte values (0-255) as `Bytes`.
/// Orion's `config_json_value` puts them back together on helios-peripherals' side.
pub fn action_args(request: &ActionRequest) -> ApiResult<BTreeMap<String, TypedConfigValue>> {
    if !DEVICE_ACTION_KINDS.contains(&request.kind.as_str()) && !RAW_ACTION_KINDS.contains(&request.kind.as_str()) {
        return Err(ApiError::bad_request(format!("unknown action kind {:?}; one of {}, {}", request.kind, DEVICE_ACTION_KINDS.join(", "), RAW_ACTION_KINDS.join(", "))));
    }
    let mut fields = Vec::new();
    for (key, value) in &request.arg {
        flatten(key, value, &mut fields)?;
    }
    if fields.len() > MAX_ARG_FIELDS {
        return Err(ApiError::bad_request(format!("arg has more than {MAX_ARG_FIELDS} values")));
    }
    Ok(fields.into_iter().collect())
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
    /// The id Orion ran the action under.
    pub action_id: String,
    pub resource: String,
    pub done: bool,
    pub result: Option<ActionResult>,
}

/// Run one action on a resource: an Orion action request, answered by the resource's provider
/// (helios-peripherals) by its id. Concurrent actions run concurrently; the answer is 200 with the
/// final result, or 202 with `done: false` and the latest result when the wait runs out.
pub async fn action(State(state): State<SharedState>, Path(id): Path<String>, Json(request): Json<ActionRequest>) -> ApiResult<(StatusCode, Json<ActionResponse>)> {
    check_id(&id)?;
    run_action(&state, id, request).await
}

async fn run_action(state: &SharedState, id: String, request: ActionRequest) -> ApiResult<(StatusCode, Json<ActionResponse>)> {
    let args = action_args(&request)?;
    let view = state.orion.view().await?;
    let resource = view.resources.get(&id).ok_or_else(|| ApiError::not_found(format!("no resource {id}")))?;
    if !resource.provider_id.as_str().starts_with(PERIPHERALS_PROVIDER_PREFIX) {
        return Err(ApiError::bad_request(format!("{id} is not a helios-peripherals resource")));
    }
    let action = action_request(&id, &request.kind, args);
    perform_action(&state.orion, action, ACTION_WAIT).await
}

/// The Orion request for an action on resource `id`: a fresh id, the deadline, and the arguments.
pub fn action_request(id: &str, kind: &str, args: BTreeMap<String, TypedConfigValue>) -> OrionActionRequest {
    let seq = ACTION_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let action_id = format!("helios-api.{}.{seq}", now_ms());
    let mut request = OrionActionRequest::new(action_id, ActionTarget::Resource(ResourceId::new(id)), kind).with_deadline_ms(ACTION_DEADLINE_MS);
    request.args = args;
    request
}

/// Runs `action` through `caller` and waits up to `wait` for its result.
pub async fn perform_action<C: ActionCall>(caller: &C, action: OrionActionRequest, wait: Duration) -> ApiResult<(StatusCode, Json<ActionResponse>)> {
    let action_id = action.action_id.clone();
    let resource = action.target.id().to_string();
    let result = caller.call_action(action, wait).await?;
    let done = result.state.is_terminal();
    let status = if done { StatusCode::OK } else { StatusCode::ACCEPTED };
    Ok((status, Json(ActionResponse { action_id, resource, done, result: Some(action_result_dto(&result)) })))
}

/// An Orion result as the API reports an action: `applied` (succeeded), `failed` (failed,
/// rejected or timed out, with the reason), or `running`. `data` is the `value` output, or the
/// whole output when there are other entries.
fn action_result_dto(result: &OrionActionResult) -> ActionResult {
    let (status, error) = match &result.state {
        ActionState::Succeeded => ("applied", None),
        ActionState::Failed { reason } | ActionState::Rejected { reason } => ("failed", Some(reason.clone())),
        ActionState::TimedOut => ("failed", Some("timed out".to_string())),
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
    use orion::control_plane::ActionRequest as OrionActionRequest;

    fn http_request(kind: &str, arg: serde_json::Value) -> ActionRequest {
        ActionRequest { kind: kind.into(), arg: arg.as_object().cloned().unwrap_or_default() }
    }

    #[test]
    fn action_args_are_typed_and_checked() {
        let args = action_args(&http_request("fan.override", serde_json::json!({ "duty": 0.8, "duration_ms": 60_000 }))).expect("args");
        assert_eq!(args.get("duty"), Some(&TypedConfigValue::F64(0.8)));
        assert_eq!(args.get("duration_ms"), Some(&TypedConfigValue::UInt(60_000)));
        let args = action_args(&http_request("control.set", serde_json::json!({ "control": "level", "value": 1.0 }))).expect("args");
        assert_eq!(args.get("control"), Some(&TypedConfigValue::String("level".into())));
        assert!(action_args(&http_request("gpio.write", serde_json::json!({}))).is_err(), "unknown kind");
    }

    #[test]
    fn raw_action_args_expand_to_dotted_names_with_byte_arrays_as_bytes() {
        let request = http_request("i2c.transfer", serde_json::json!({ "bus": "i2c-1", "address": 80, "ops": [{ "write": [16] }, { "read": 2 }], "skip": null, "offset": -1 }));
        let args = action_args(&request).expect("args");
        assert_eq!(args.get("bus"), Some(&TypedConfigValue::String("i2c-1".into())));
        assert_eq!(args.get("address"), Some(&TypedConfigValue::UInt(80)));
        assert_eq!(args.get("ops.0.write"), Some(&TypedConfigValue::Bytes(vec![16])));
        assert_eq!(args.get("ops.1.read"), Some(&TypedConfigValue::UInt(2)));
        assert_eq!(args.get("offset"), Some(&TypedConfigValue::Int(-1)));
        assert!(!args.contains_key("skip"));
        // Orion's config decoding gives the same JSON back (what helios-peripherals reads).
        let decoded = orion::control_plane::config_json_value(&args).expect("decode");
        assert_eq!(decoded["ops"], serde_json::json!([{ "write": [16] }, { "read": 2 }]));

        let too_big = http_request("spi.transfer", serde_json::json!({ "transfers": vec![serde_json::json!({ "rx_len": 1 }); MAX_ARG_FIELDS + 1] }));
        assert!(action_args(&too_big).is_err());
        let numbered = http_request("gpio.claim", serde_json::json!({ "x": { "0": 1 } }));
        assert!(action_args(&numbered).is_err(), "numeric keys would read as array indices");
    }

    /// A fake Orion: each action takes `duration` and answers `Running` when the wait runs out first.
    struct FakeOrion {
        duration: std::time::Duration,
        in_flight: std::sync::atomic::AtomicUsize,
        peak: std::sync::atomic::AtomicUsize,
    }

    impl ActionCall for FakeOrion {
        async fn call_action(&self, request: OrionActionRequest, timeout: std::time::Duration) -> ApiResult<OrionActionResult> {
            {
                use std::sync::atomic::Ordering::SeqCst;
                let now = self.in_flight.fetch_add(1, SeqCst) + 1;
                self.peak.fetch_max(now, SeqCst);
                tokio::time::sleep(self.duration.min(timeout)).await;
                self.in_flight.fetch_sub(1, SeqCst);
                let state = if self.duration <= timeout { ActionState::Succeeded } else { ActionState::Running { progress: None } };
                let mut result = OrionActionResult::new(request.action_id.clone(), request.target.clone(), request.name.clone(), orion::core::NodeId::new("node-local"), state);
                if request.name == "raw.renew" {
                    result = result.with_output("value", TypedConfigValue::UInt(7));
                }
                Ok(result)
            }
        }
    }

    fn fake(duration_ms: u64) -> FakeOrion {
        FakeOrion { duration: std::time::Duration::from_millis(duration_ms), in_flight: Default::default(), peak: Default::default() }
    }

    #[tokio::test]
    async fn concurrent_raw_actions_run_at_once() {
        let orion = fake(100);
        let started = std::time::Instant::now();
        let calls = (0..4).map(|n| {
            let action = action_request("lemnos_raw_node1_io", "gpio.get", BTreeMap::from([("claim".to_string(), TypedConfigValue::String(format!("gpio-{n}")))]));
            perform_action(&orion, action, std::time::Duration::from_secs(5))
        });
        let answers = futures_util_join(calls).await;
        assert!(answers.iter().all(|(status, body)| *status == StatusCode::OK && body.done), "every action answers 200 done");
        assert_eq!(orion.peak.load(std::sync::atomic::Ordering::SeqCst), 4, "no action waits for another");
        assert!(started.elapsed() < std::time::Duration::from_millis(350), "four 100 ms actions take about 100 ms, not 400");
        let ids: std::collections::BTreeSet<_> = answers.iter().map(|(_, body)| body.action_id.clone()).collect();
        assert_eq!(ids.len(), 4, "each action has its own id");
    }

    #[tokio::test]
    async fn an_action_past_the_wait_answers_202_with_its_latest_result() {
        let orion = fake(500);
        let action = action_request("lemnos_raw_node1_io", "i2c.transfer", BTreeMap::new());
        let (status, body) = perform_action(&orion, action, std::time::Duration::from_millis(50)).await.expect("answer");
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(!body.done);
        assert_eq!(body.result.as_ref().map(|result| result.status.as_str()), Some("running"));
    }

    #[tokio::test]
    async fn results_carry_the_value_output_as_data() {
        let orion = fake(0);
        let action = action_request("lemnos_raw_node1_io", "raw.renew", BTreeMap::new());
        let (_, body) = perform_action(&orion, action, std::time::Duration::from_secs(1)).await.expect("answer");
        let result = body.result.clone().expect("result");
        assert_eq!(result.status, "applied");
        assert_eq!(result.action_kind, "raw.renew");
        assert_eq!(result.data, Some(serde_json::json!(7)));
    }

    /// Joins the futures in order (no runtime-specific join).
    async fn futures_util_join<F: std::future::Future<Output = ApiResult<(StatusCode, Json<ActionResponse>)>>>(futures: impl Iterator<Item = F>) -> Vec<(StatusCode, ActionResponse)> {
        let mut out = Vec::new();
        for (status, body) in futures_util::future::join_all(futures.map(|future| async move { future.await.expect("action") })).await {
            out.push((status, body.0));
        }
        out
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
