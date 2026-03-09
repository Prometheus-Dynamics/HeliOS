use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::post};
use base64::Engine;
use nt_client::subscribe::ReceivedMessage;
use nt_client::subscribe::SubscriptionOptions;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use utoipa::ToSchema;

use super::streams::types::EngineErrorBody;
use super::streams::util::engine_error_body;

use crate::nt4;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct Nt4TopicsRequest {
    pub host: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub scan_ms: Option<u64>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, ToSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Nt4TopicInfo {
    pub name: String,
    pub data_type: String,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub properties: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, ToSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Nt4TopicsResponse {
    pub host: String,
    pub port: u16,
    pub prefix: String,
    pub topics: Vec<Nt4TopicInfo>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct Nt4ValueRequest {
    pub host: String,
    #[serde(default)]
    pub port: Option<u16>,
    pub topic: String,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, ToSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Nt4ValueResponse {
    pub host: String,
    pub port: u16,
    pub topic: String,
    #[serde(default)]
    pub data_type: Option<String>,
    pub value: serde_json::Value,
}

pub fn router() -> Router<super::AppState> {
    Router::new().route("/topics", post(list_topics)).route("/value", post(read_value))
}

fn team_number_to_rio_ip(team: u32) -> Option<Ipv4Addr> {
    if team == 0 || team > 25_599 {
        return None;
    }
    let a = (team / 100) as u8;
    let b = (team % 100) as u8;
    Some(Ipv4Addr::new(10, a, b, 2))
}

fn team_number_from_rio_ip(ip: Ipv4Addr) -> Option<u32> {
    let [a0, a1, a2, a3] = ip.octets();
    if a0 != 10 || a3 != 2 || (a1 == 0 && a2 == 0) {
        return None;
    }
    let team = (a1 as u32) * 100 + (a2 as u32);
    if team == 0 || team > 25_599 {
        return None;
    }
    Some(team)
}

fn team_number_from_rio_hostname(host: &str) -> Option<u32> {
    let normalized = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let remainder = normalized.strip_prefix("roborio-")?;
    let team_raw = remainder.strip_suffix("-frc.local").or_else(|| remainder.strip_suffix("-frc"))?;
    let team: u32 = team_raw.parse().ok()?;
    if team == 0 || team > 25_599 {
        return None;
    }
    Some(team)
}

fn host_candidates(host: &str) -> Vec<String> {
    let trimmed = host.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(4);
    out.push(trimmed.to_string());

    let team = if let Ok(ip) = trimmed.parse::<Ipv4Addr>() { team_number_from_rio_ip(ip) } else { team_number_from_rio_hostname(trimmed) };

    if let Some(team) = team {
        out.push(format!("roborio-{team}-frc.local"));
        if let Some(rio_ip) = team_number_to_rio_ip(team) {
            out.push(rio_ip.to_string());
        }
        // USB gadget networking path used by roboRIO when directly tethered.
        out.push("172.22.11.2".to_string());
    }

    let mut deduped = Vec::with_capacity(out.len());
    for candidate in out {
        if !deduped.iter().any(|entry| entry == &candidate) {
            deduped.push(candidate);
        }
    }
    deduped
}

async fn connect_nt4_target(requested_host: &str, port: u16, timeout_ms: u64) -> Result<(Arc<crate::nt4::pool::Nt4ClientEntry>, String), String> {
    let candidates = host_candidates(requested_host);
    if candidates.is_empty() {
        return Err("host is required".to_string());
    }

    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms.clamp(150, 15_000));
    let mut attempts = Vec::new();

    for (idx, candidate) in candidates.iter().enumerate() {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let remaining_candidates = (candidates.len() - idx) as u32;
        let per_candidate = std::cmp::max(Duration::from_millis(150), remaining / remaining_candidates);
        let wait = std::cmp::min(remaining, per_candidate);

        let entry = match nt4::pool().get_or_connect(candidate, port, "HeliOS-nt4").await {
            Ok(entry) => entry,
            Err(err) => {
                attempts.push(format!("{candidate}: {err}"));
                continue;
            }
        };

        match entry.wait_ready(wait).await {
            Ok(()) => return Ok((entry, candidate.clone())),
            Err(err) => {
                attempts.push(format!("{candidate}: {err}"));
                let _ = nt4::pool().disconnect(candidate, port).await;
            }
        }
    }

    if attempts.is_empty() {
        return Err(format!("nt4 connection timed out for host {requested_host}:{port}"));
    }
    Err(format!("unable to connect to nt4 at {requested_host}:{port} (tried: {})", attempts.join("; ")))
}

fn topic_info_from_announced(announced: &nt_client::topic::AnnouncedTopic) -> Nt4TopicInfo {
    Nt4TopicInfo { name: announced.name().to_string(), data_type: format!("{:?}", announced.r#type()), properties: serde_json::to_value(announced.properties()).ok() }
}

#[utoipa::path(
    post,
    path = "/nt4/topics",
    tag = "NT4",
    request_body = Nt4TopicsRequest,
    responses(
        (status = 200, description = "Discovered topics", body = Nt4TopicsResponse),
        (status = 400, description = "Invalid payload", body = EngineErrorBody),
        (status = 502, description = "NT4 error", body = EngineErrorBody)
    )
)]
pub(crate) async fn list_topics(Json(req): Json<Nt4TopicsRequest>) -> axum::response::Response {
    let settings = crate::http::device::nt4::load_settings().await;
    if !settings.subscriptions_enabled {
        return (StatusCode::CONFLICT, Json(engine_error_body(None, "nt4 subscriptions are disabled in device settings"))).into_response();
    }

    let host = req.host.trim();
    if host.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(engine_error_body(None, "host is required"))).into_response();
    }
    let port = req.port.unwrap_or(5810);
    let prefix = req.prefix.unwrap_or_else(|| "/".to_string());
    let timeout_ms = req.timeout_ms.unwrap_or(1500).clamp(150, 15_000);
    let scan_ms = req.scan_ms.unwrap_or(350).clamp(50, 5000);
    let limit = req.limit.unwrap_or(512).clamp(1, 10_000);

    let (entry, connected_host) = match connect_nt4_target(host, port, timeout_ms).await {
        Ok(result) => result,
        Err(err) => return (StatusCode::BAD_GATEWAY, Json(engine_error_body(None, err))).into_response(),
    };

    let options = SubscriptionOptions { topics_only: Some(true), prefix: Some(true), all: Some(true), periodic: Some(Duration::from_millis(50)), ..Default::default() };

    let topic = entry.handle().topic(prefix.clone());
    let mut subscriber = match topic.subscribe(options).await {
        Ok(subscriber) => subscriber,
        Err(err) => return (StatusCode::BAD_GATEWAY, Json(engine_error_body(None, err.to_string()))).into_response(),
    };

    let deadline = tokio::time::Instant::now() + Duration::from_millis(scan_ms);
    let mut topics_by_name: BTreeMap<String, Nt4TopicInfo> = BTreeMap::new();

    let existing_topics = subscriber.topics().await;
    for announced in existing_topics.into_values() {
        let info = topic_info_from_announced(&announced);
        topics_by_name.entry(info.name.clone()).or_insert(info);
        if topics_by_name.len() >= limit {
            break;
        }
    }

    loop {
        if topics_by_name.len() >= limit {
            break;
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match timeout(remaining, subscriber.recv()).await {
            Ok(Ok(ReceivedMessage::Announced(announced))) => {
                let info = topic_info_from_announced(&announced);
                topics_by_name.insert(info.name.clone(), info);
            }
            Ok(Ok(ReceivedMessage::UpdateProperties(announced))) => {
                let info = topic_info_from_announced(&announced);
                topics_by_name.insert(info.name.clone(), info);
            }
            Ok(Ok(_)) => {}
            Ok(Err(err)) => return (StatusCode::BAD_GATEWAY, Json(engine_error_body(None, err.to_string()))).into_response(),
            Err(_) => break,
        }
    }

    let topics = topics_by_name.into_values().take(limit).collect::<Vec<_>>();

    (StatusCode::OK, Json(Nt4TopicsResponse { host: connected_host, port, prefix, topics })).into_response()
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
        Err(err) => return (StatusCode::BAD_GATEWAY, Json(engine_error_body(None, err))).into_response(),
    };

    let options = SubscriptionOptions { all: Some(true), periodic: Some(Duration::from_millis(50)), ..Default::default() };

    let topic = entry.handle().topic(topic_name.to_string());
    let mut subscriber = match topic.subscribe(options).await {
        Ok(subscriber) => subscriber,
        Err(err) => return (StatusCode::BAD_GATEWAY, Json(engine_error_body(None, err.to_string()))).into_response(),
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
        Ok(Err(err)) => return (StatusCode::BAD_GATEWAY, Json(engine_error_body(None, err))).into_response(),
        Err(_) => return (StatusCode::NOT_FOUND, Json(engine_error_body(None, "no value received before timeout"))).into_response(),
    };

    let (announced, value) = updated;
    let data_type = Some(format!("{:?}", announced.r#type()));
    let json_value = rmpv_to_json(&value);

    (StatusCode::OK, Json(Nt4ValueResponse { host: connected_host, port, topic: topic_name.to_string(), data_type, value: json_value })).into_response()
}

fn rmpv_to_json(value: &rmpv::Value) -> serde_json::Value {
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

#[cfg(test)]
mod tests {
    use super::{host_candidates, team_number_from_rio_hostname, team_number_from_rio_ip};
    use std::net::Ipv4Addr;

    #[test]
    fn parses_team_from_rio_ip() {
        assert_eq!(team_number_from_rio_ip(Ipv4Addr::new(10, 25, 4, 2)), Some(2504));
        assert_eq!(team_number_from_rio_ip(Ipv4Addr::new(10, 0, 0, 2)), None);
        assert_eq!(team_number_from_rio_ip(Ipv4Addr::new(172, 22, 11, 2)), None);
    }

    #[test]
    fn parses_team_from_rio_hostname() {
        assert_eq!(team_number_from_rio_hostname("roborio-254-frc.local"), Some(254));
        assert_eq!(team_number_from_rio_hostname("roborio-6328-frc"), Some(6328));
        assert_eq!(team_number_from_rio_hostname("roborio-254-frc.local."), Some(254));
        assert_eq!(team_number_from_rio_hostname("example.local"), None);
    }

    #[test]
    fn expands_rio_host_fallbacks() {
        let list = host_candidates("10.6.32.2");
        assert!(list.iter().any(|entry| entry == "10.6.32.2"));
        assert!(list.iter().any(|entry| entry == "roborio-632-frc.local"));
        assert!(list.iter().any(|entry| entry == "172.22.11.2"));
    }
}
