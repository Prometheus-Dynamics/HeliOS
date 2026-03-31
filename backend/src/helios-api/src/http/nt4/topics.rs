use axum::{Json, http::StatusCode, response::IntoResponse};
use nt_client::subscribe::{ReceivedMessage, SubscriptionOptions};
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::time::timeout;

use super::super::streams::types::EngineErrorBody;
use super::super::streams::util::engine_error_body;
use super::transport::connect_nt4_target;
use super::types::{Nt4TopicInfo, Nt4TopicsRequest, Nt4TopicsResponse};

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
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(None, err))).into_response();
        }
    };

    let options = SubscriptionOptions { topics_only: Some(true), prefix: Some(true), all: Some(true), periodic: Some(Duration::from_millis(50)), ..Default::default() };

    let topic = entry.handle().topic(prefix.clone());
    let mut subscriber = match topic.subscribe(options).await {
        Ok(subscriber) => subscriber,
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(None, err.to_string()))).into_response();
        }
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
            Ok(Err(err)) => {
                return (StatusCode::BAD_GATEWAY, Json(engine_error_body(None, err.to_string()))).into_response();
            }
            Err(_) => break,
        }
    }

    let topics = topics_by_name.into_values().take(limit).collect::<Vec<_>>();
    (StatusCode::OK, Json(Nt4TopicsResponse { host: connected_host, port, prefix, topics })).into_response()
}
