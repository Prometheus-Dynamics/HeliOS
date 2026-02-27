use axum::{Json, extract::State, response::IntoResponse};
use helios_peripherals::dto::{SensorScope, SensorSnapshot};
use tracing::error;
use utoipa::ToSchema;

use super::super::AppState;
use super::super::error::{ApiError, ApiResult};
use helios_peripherals::dto::SensorKind;
use serde_json::Value as JsonValue;

#[derive(Debug, ToSchema, serde::Serialize, Default, Clone)]
pub struct PowerSourcePayload {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bus: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watts: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volts: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shunt_volts: Option<f64>,
}

#[derive(Debug, ToSchema, serde::Serialize, Default, Clone)]
pub struct PowerStatusPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watts: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volts: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<PowerSourcePayload>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<String>>,
}

#[utoipa::path(
    get,
    path = "/device/power",
    tag = "Device",
    responses(
        (status = 200, description = "Power status", body = PowerStatusPayload),
        (status = 400, description = "Bad request", body = super::super::error::ErrorBody),
        (status = 503, description = "Peripherals IPC unavailable", body = super::super::error::ErrorBody)
    )
)]
pub async fn power_status(State(state): State<AppState>) -> ApiResult<impl IntoResponse> {
    let Some(peripherals) = state.ensure_sensors().await else {
        return Err(ApiError::service_unavailable("peripherals IPC unavailable"));
    };

    match peripherals.sensor_snapshot(SensorScope::Device).await {
        Ok(Ok(snapshot)) => Ok(Json(power_status_from_snapshot(&snapshot))),
        Ok(Err(reason)) => Err(ApiError::bad_request(reason)),
        Err(err) => {
            error!(%err, "failed to fetch power status");
            Err(ApiError::bad_gateway("failed to fetch power status"))
        }
    }
}

pub(crate) fn power_status_from_snapshot(values: &SensorSnapshot) -> PowerStatusPayload {
    let mut status = PowerStatusPayload::default();
    let Some(JsonValue::Object(map)) = values.get(&SensorKind::Power).and_then(|data| data.to_value().ok()) else {
        return status;
    };

    status.watts = map.get("watts").and_then(as_f64);
    status.amps = map.get("amps").and_then(as_f64);
    status.volts = map.get("volts").and_then(as_f64);
    status.updated_at = map.get("updated_at").and_then(as_string);

    if let Some(JsonValue::Array(entries)) = map.get("sources") {
        let sources: Vec<PowerSourcePayload> = entries
            .iter()
            .filter_map(|value| match value {
                JsonValue::Object(source) => Some(PowerSourcePayload {
                    label: source.get("label").and_then(as_string).unwrap_or_else(|| "Source".into()),
                    bus: source.get("bus").and_then(as_u32),
                    address: source.get("address").and_then(as_string),
                    watts: source.get("watts").and_then(as_f64),
                    volts: source.get("volts").and_then(as_f64),
                    amps: source.get("amps").and_then(as_f64),
                    shunt_volts: source.get("shunt_volts").and_then(as_f64),
                }),
                _ => None,
            })
            .collect();
        if !sources.is_empty() {
            status.sources = Some(sources);
        }
    }

    if let Some(JsonValue::Array(entries)) = map.get("errors") {
        let errors: Vec<String> = entries
            .iter()
            .filter_map(|value| match value {
                JsonValue::String(msg) if !msg.trim().is_empty() => Some(msg.trim().to_string()),
                _ => None,
            })
            .collect();
        if !errors.is_empty() {
            status.errors = Some(errors);
        }
    }
    if let Some(JsonValue::String(error)) = map.get("error")
        && !error.trim().is_empty()
    {
        status.errors = Some(vec![error.trim().to_string()]);
    }

    status
}

fn as_f64(value: &JsonValue) -> Option<f64> {
    match value {
        JsonValue::Number(v) => v.as_f64(),
        JsonValue::String(v) => v.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn as_string(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::String(v) if !v.trim().is_empty() => Some(v.trim().to_string()),
        _ => None,
    }
}

fn as_u32(value: &JsonValue) -> Option<u32> {
    match value {
        JsonValue::Number(v) => v.as_u64().and_then(|n| u32::try_from(n).ok()),
        JsonValue::String(v) => v.trim().parse::<u32>().ok(),
        _ => None,
    }
}
