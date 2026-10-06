//! The event stream (`GET /v1/events`, Server-Sent Events).
//!
//! While anyone is subscribed, a watcher reads Orion once a second and publishes what changed:
//! `pipeline`, `resource` and `update` events, plus `metrics` every two seconds and `orion` when
//! Orion becomes reachable or unreachable. Each SSE message has the event type as its `event:`
//! name and an [`ApiEvent`] JSON object as its data.

use std::{
    collections::{BTreeMap, BTreeSet},
    convert::Infallible,
    time::Duration,
};

use axum::{
    extract::{Query, State},
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use crate::{SharedState, host::now_ms, routes};

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const METRICS_EVERY: u32 = 2;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ApiEvent {
    #[serde(rename = "type")]
    pub kind: String,
    pub at_ms: u64,
    pub data: serde_json::Value,
}

pub struct EventHub {
    tx: broadcast::Sender<ApiEvent>,
}

impl EventHub {
    pub fn new(capacity: usize) -> Self {
        Self { tx: broadcast::channel(capacity).0 }
    }

    pub fn publish(&self, kind: &str, data: serde_json::Value) {
        // No subscribers is fine: events are only for whoever listens now.
        let _ = self.tx.send(ApiEvent { kind: kind.to_string(), at_ms: now_ms(), data });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ApiEvent> {
        self.tx.subscribe()
    }

    pub fn subscribers(&self) -> usize {
        self.tx.receiver_count()
    }
}

/// What the watcher compares between polls.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Digest {
    pub pipelines: BTreeMap<String, serde_json::Value>,
    pub resources: BTreeMap<String, serde_json::Value>,
    pub update: Option<serde_json::Value>,
}

/// The events that turn `before` into `after`.
pub fn diff(before: &Digest, after: &Digest) -> Vec<(&'static str, serde_json::Value)> {
    let mut out = Vec::new();
    diff_maps("pipeline", &before.pipelines, &after.pipelines, &mut out);
    diff_maps("resource", &before.resources, &after.resources, &mut out);
    if before.update != after.update
        && let Some(update) = &after.update
    {
        out.push(("update", update.clone()));
    }
    out
}

fn diff_maps(kind: &'static str, before: &BTreeMap<String, serde_json::Value>, after: &BTreeMap<String, serde_json::Value>, out: &mut Vec<(&'static str, serde_json::Value)>) {
    for (id, value) in after {
        if before.get(id) != Some(value) {
            out.push((kind, serde_json::json!({ "id": id, "change": if before.contains_key(id) { "updated" } else { "added" }, "value": value })));
        }
    }
    for id in before.keys().filter(|id| !after.contains_key(*id)) {
        out.push((kind, serde_json::json!({ "id": id, "change": "removed" })));
    }
}

/// Poll Orion while there are subscribers and publish the changes.
pub fn spawn_state_watcher(state: SharedState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last: Option<Digest> = None;
        let mut orion_up: Option<bool> = None;
        let mut tick: u32 = 0;
        let mut interval = tokio::time::interval(POLL_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if state.events.subscribers() == 0 {
                // Start from a fresh baseline when someone subscribes again.
                last = None;
                orion_up = None;
                continue;
            }
            tick = tick.wrapping_add(1);
            if tick.is_multiple_of(METRICS_EVERY)
                && let Ok(metrics) = serde_json::to_value(routes::system::metrics_now(&state).await)
            {
                state.events.publish("metrics", metrics);
            }
            match state.orion.view().await {
                Ok(view) => {
                    if orion_up != Some(true) {
                        state.events.publish("orion", serde_json::json!({ "reachable": true, "desired_revision": view.desired_revision }));
                        orion_up = Some(true);
                    }
                    let digest = Digest {
                        pipelines: routes::pipelines::digest(&view),
                        resources: routes::resources::digest(&view),
                        update: serde_json::to_value(routes::update::status_from(&state.config, Some(&view))).ok(),
                    };
                    if let Some(before) = &last {
                        for (kind, data) in diff(before, &digest) {
                            state.events.publish(kind, data);
                        }
                    }
                    last = Some(digest);
                }
                Err(error) => {
                    if orion_up != Some(false) {
                        state.events.publish("orion", serde_json::json!({ "reachable": false, "message": error.message }));
                        orion_up = Some(false);
                    }
                }
            }
        }
    })
}

#[derive(Debug, Deserialize, Default)]
pub struct EventFilter {
    /// Comma-separated event types; all when absent.
    pub types: Option<String>,
}

impl EventFilter {
    pub fn kinds(&self) -> Option<BTreeSet<String>> {
        self.types.as_ref().map(|types| types.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect())
    }
}

pub fn sse_stream(receiver: broadcast::Receiver<ApiEvent>, kinds: Option<BTreeSet<String>>, first: Vec<ApiEvent>) -> impl Stream<Item = Result<Event, Infallible>> {
    let initial = futures_util::stream::iter(first.into_iter().map(|event| Ok(to_sse(&event))));
    let live = BroadcastStream::new(receiver).filter_map(move |item| {
        let kinds = kinds.clone();
        async move {
            match item {
                Ok(event) if kinds.as_ref().is_none_or(|kinds| kinds.contains(&event.kind)) => Some(Ok(to_sse(&event))),
                Ok(_) => None,
                // A slow client missed events: tell it to refetch.
                Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(missed)) => {
                    Some(Ok(to_sse(&ApiEvent { kind: "lagged".into(), at_ms: now_ms(), data: serde_json::json!({ "missed": missed }) })))
                }
            }
        }
    });
    initial.chain(live)
}

pub fn to_sse(event: &ApiEvent) -> Event {
    Event::default().event(event.kind.clone()).data(serde_json::to_string(event).unwrap_or_default())
}

pub async fn events(State(state): State<SharedState>, Query(filter): Query<EventFilter>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let receiver = state.events.subscribe();
    let hello = ApiEvent { kind: "hello".into(), at_ms: now_ms(), data: serde_json::json!({ "api_version": crate::API_VERSION, "version": crate::VERSION }) };
    Sse::new(sse_stream(receiver, filter.kinds(), vec![hello])).keep_alive(KeepAlive::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_reports_added_updated_removed() {
        let before = Digest { pipelines: BTreeMap::from([("a".into(), serde_json::json!("running")), ("b".into(), serde_json::json!("running"))]), ..Digest::default() };
        let after = Digest {
            pipelines: BTreeMap::from([("a".into(), serde_json::json!("stopped")), ("c".into(), serde_json::json!("starting"))]),
            update: Some(serde_json::json!({"phase": "idle"})),
            ..Digest::default()
        };
        let events = diff(&before, &after);
        let changes: Vec<_> = events.iter().map(|(kind, data)| format!("{kind}:{}:{}", data["id"].as_str().unwrap_or("-"), data["change"].as_str().unwrap_or("-"))).collect();
        assert!(changes.contains(&"pipeline:a:updated".to_string()));
        assert!(changes.contains(&"pipeline:c:added".to_string()));
        assert!(changes.contains(&"pipeline:b:removed".to_string()));
        assert!(events.iter().any(|(kind, _)| *kind == "update"));
    }

    #[test]
    fn filter_parses_types() {
        let filter = EventFilter { types: Some("update, pipeline,".into()) };
        assert_eq!(filter.kinds().expect("kinds"), BTreeSet::from(["update".to_string(), "pipeline".to_string()]));
    }
}
