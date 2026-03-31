use axum::{Json, http::StatusCode, response::IntoResponse};
use base64::Engine;
use nt_client::subscribe::{ReceivedMessage, SubscriptionOptions};
use std::time::Duration;
use tokio::time::timeout;

use super::super::streams::types::EngineErrorBody;
use super::super::streams::util::engine_error_body;
use super::transport::connect_nt4_target;
use super::types::{Nt4ValueRequest, Nt4ValueResponse};

pub(super) fn rmpv_to_json(value: &rmpv::Value) -> serde_json::Value {
    use rmpv::Value as V;
    match value {
        V::Nil => serde_json::Value::Null,
        V::Boolean(b) => serde_json::Value::Bool(*b),
        V::Integer(int) => {
            if let Some(i) = int.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(u) = int.as_u64() {
                serde_json::Value::Number(serde_json::Number::from(u))
            } else {
                serde_json::Value::Null
            }
        }
        V::F32(f) => serde_json::json!(*f),
        V::F64(f) => serde_json::json!(*f),
        V::String(s) => serde_json::Value::String(s.as_str().unwrap_or_default().to_string()),
        V::Binary(bytes) => serde_json::json!({
            "encoding": "base64",
            "data": base64::engine::general_purpose::STANDARD.encode(bytes),
        }),
        V::Array(items) => serde_json::Value::Array(items.iter().map(rmpv_to_json).collect()),
        V::Map(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map {
                let key = match k {
                    V::String(s) => s.as_str().unwrap_or_default().to_string(),
                    other => other.to_string(),
                };
                obj.insert(key, rmpv_to_json(v));
            }
            serde_json::Value::Object(obj)
        }
        other => serde_json::Value::String(other.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/nt4/value",
    tag = "NT4",
    request_body = Nt4ValueRequest,
    responses(
        (status = 200, description = "Topic value", body = Nt4ValueResponse),
        (status = 400, description = "Invalid payload", body = EngineErrorBody),
        (status = 404, description = "No value available", body = EngineErrorBody),
        (status = 502, description = "NT4 error", body = EngineErrorBody)
    )
)]
pub(crate) async fn read_value(Json(req): Json<Nt4ValueRequest>) -> axum::response::Response {
    let settings = crate::http::device::nt4::load_settings().await;
    if !settings.subscriptions_enabled {
        return (StatusCode::CONFLICT, Json(engine_error_body(None, "nt4 subscriptions are disabled in device settings"))).into_response();
    }

    let host = req.host.trim();
    if host.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(engine_error_body(None, "host is required"))).into_response();
    }
    let port = req.port.unwrap_or(5810);
    let topic_name = req.topic.trim();
    if topic_name.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(engine_error_body(None, "topic is required"))).into_response();
    }
    let timeout_ms = req.timeout_ms.unwrap_or(1200).clamp(150, 15_000);

    let (entry, connected_host) = match connect_nt4_target(host, port, timeout_ms).await {
        Ok(result) => result,
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(None, err))).into_response();
        }
    };

    let options = SubscriptionOptions { all: Some(true), periodic: Some(Duration::from_millis(50)), ..Default::default() };

    let topic = entry.handle().topic(topic_name.to_string());
    let mut subscriber = match topic.subscribe(options).await {
        Ok(subscriber) => subscriber,
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(None, err.to_string()))).into_response();
        }
    };

    if let Some(initial) = subscriber.topics().await.into_values().find(|announced| announced.name() == topic_name).and_then(|announced| announced.value().cloned().map(|value| (announced, value))) {
        let (announced, value) = initial;
        let data_type = Some(format!("{:?}", announced.r#type()));
        let json_value = rmpv_to_json(&value);
        return (StatusCode::OK, Json(Nt4ValueResponse { host: connected_host, port, topic: topic_name.to_string(), data_type, value: json_value })).into_response();
    }

    let updated = match timeout(Duration::from_millis(timeout_ms), async {
        loop {
            match subscriber.recv().await {
                Ok(ReceivedMessage::Updated((announced, value))) => return Ok((announced, value)),
                Ok(ReceivedMessage::Announced(announced)) | Ok(ReceivedMessage::UpdateProperties(announced)) => {
                    if let Some(value) = announced.value().cloned() {
                        return Ok((announced, value));
                    }
                }
                Ok(_) => {}
                Err(err) => return Err(err.to_string()),
            }
        }
    })
    .await
    {
        Ok(Ok(pair)) => pair,
        Ok(Err(err)) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(None, err))).into_response();
        }
        Err(_) => {
            return (StatusCode::NOT_FOUND, Json(engine_error_body(None, "no value received before timeout"))).into_response();
        }
    };

    let (announced, value) = updated;
    let data_type = Some(format!("{:?}", announced.r#type()));
    let json_value = rmpv_to_json(&value);

    (StatusCode::OK, Json(Nt4ValueResponse { host: connected_host, port, topic: topic_name.to_string(), data_type, value: json_value })).into_response()
}
