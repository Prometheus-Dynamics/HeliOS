//! The event stream (`GET /v1/events`, Server-Sent Events).
//!
//! A watcher follows Orion on one event stream (desired-state changes and this node's `host.*`
//! metrics; Orion pushes both), from the first subscriber on, and while anyone is subscribed
//! publishes what changed: `pipeline` and `resource` events (observed changes within one
//! host-metrics sample, see [`follow`]), `metrics` with every host-facts sample
//! (`ORION_NODE_HOST_FACTS_REFRESH_MS`, 2 s on the image) and `orion` when Orion becomes reachable
//! or unreachable. The device package updater's state is a local file, read once a second for
//! `update` events. Each SSE message has the event type as its `event:` name and an [`ApiEvent`]
//! JSON object as its data.

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
use orion::control_plane::{ClientEventKind, StateSnapshot, StatusChange, TypedConfigValue};
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, broadcast, mpsc};
use tokio_stream::wrappers::BroadcastStream;

use crate::{
    SharedState,
    host::now_ms,
    orion::{Orion, StateView},
    routes::{
        self,
        system::{HostSample, metrics_from},
    },
};

/// How often the updater's state file is read (and, while Orion is down, metrics published).
const UPDATE_INTERVAL: Duration = Duration::from_secs(1);
const METRICS_WHILE_DOWN_EVERY: u32 = 2;
/// Wait between attempts to reach Orion's event stream.
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ApiEvent {
    #[serde(rename = "type")]
    pub kind: String,
    pub at_ms: u64,
    pub data: serde_json::Value,
}

pub struct EventHub {
    tx: broadcast::Sender<ApiEvent>,
    subscribed: Notify,
}

impl EventHub {
    pub fn new(capacity: usize) -> Self {
        Self { tx: broadcast::channel(capacity).0, subscribed: Notify::new() }
    }

    pub fn publish(&self, kind: &str, data: serde_json::Value) {
        // No subscribers is fine: events are only for whoever listens now.
        let _ = self.tx.send(ApiEvent { kind: kind.to_string(), at_ms: now_ms(), data });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ApiEvent> {
        let receiver = self.tx.subscribe();
        self.subscribed.notify_one();
        receiver
    }

    pub fn subscribers(&self) -> usize {
        self.tx.receiver_count()
    }

    /// Returns once someone is subscribed.
    pub async fn wait_for_subscriber(&self) {
        while self.subscribers() == 0 {
            self.subscribed.notified().await;
        }
    }
}

/// What the watcher compares between polls.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Digest {
    pub pipelines: BTreeMap<String, serde_json::Value>,
    pub resources: BTreeMap<String, serde_json::Value>,
}

/// The events that turn `before` into `after`.
pub fn diff(before: &Digest, after: &Digest) -> Vec<(&'static str, serde_json::Value)> {
    let mut out = Vec::new();
    diff_maps("pipeline", &before.pipelines, &after.pipelines, &mut out);
    diff_maps("resource", &before.resources, &after.resources, &mut out);
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

/// What the Orion feed hands the watcher.
#[derive(Debug)]
enum OrionFeed {
    /// The desired/observed state after a change (the first one on every connection).
    State(Box<StateSnapshot>),
    /// Changed `host.*` keys of this node (the first one on every connection is a bootstrap).
    Host(StatusChange),
    /// Orion is not reachable, or the stream ended; the feed reconnects.
    Down(String),
}

/// The task following Orion's event stream. Dropping it disconnects.
///
/// It stays connected while nobody is subscribed (the node pushes one small host sample per
/// interval), so subscribers coming and going do not leave disconnected sessions on the node.
struct Feed {
    task: tokio::task::JoinHandle<()>,
    rx: mpsc::Receiver<OrionFeed>,
}

impl Feed {
    fn spawn(orion: Orion) -> Self {
        let (tx, rx) = mpsc::channel(64);
        let task = tokio::spawn(async move {
            loop {
                let message = match follow(&orion, &tx).await {
                    Ok(()) => return, // The watcher is gone.
                    Err(message) => message,
                };
                if tx.send(OrionFeed::Down(message)).await.is_err() {
                    return;
                }
                tokio::time::sleep(RECONNECT_DELAY).await;
            }
        });
        Self { task, rx }
    }
}

/// One connection to Orion's event stream: forwards state snapshots and host changes until the
/// stream fails (`Err`) or the watcher goes away (`Ok`).
///
/// The state subscription includes observed changes (a pipeline's session state, resource
/// health and leases), starting with a bootstrap snapshot, so nothing is re-read or polled.
async fn follow(orion: &Orion, tx: &mpsc::Sender<OrionFeed>) -> Result<(), String> {
    let mut events = orion.state_and_host_events().await.map_err(|error| error.message)?;
    loop {
        let batch = events.next_events().await.map_err(|error| format!("Orion's event stream ended: {error}"))?;
        for event in batch {
            let item = match event.event {
                ClientEventKind::StateSnapshot(snapshot) => OrionFeed::State(snapshot),
                ClientEventKind::Status(change) => OrionFeed::Host(change),
                _ => continue,
            };
            if tx.send(item).await.is_err() {
                return Ok(());
            }
        }
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The watcher's state.
#[derive(Default)]
struct Watch {
    feed: Option<Feed>,
    /// What subscribers were last told; reset when the last one leaves, so the next one starts
    /// from a fresh baseline (and an `orion` event).
    seen: Seen,
    /// Desired revision of the newest state applied on this connection.
    desired_revision: Option<u64>,
    /// The node's current `host.*` status keys.
    host: BTreeMap<String, TypedConfigValue>,
    ticks: u32,
}

#[derive(Default)]
struct Seen {
    active: bool,
    last: Option<Digest>,
    last_update: Option<serde_json::Value>,
    orion_up: Option<bool>,
}

impl Watch {
    fn on_orion(&mut self, state: &SharedState, item: OrionFeed) {
        let active = self.seen.active;
        match item {
            OrionFeed::State(snapshot) => {
                // The baseline read and a queued watch event can cross at connect: keep the newer.
                let revision = snapshot.state.desired.revision.get();
                if self.desired_revision.is_some_and(|newest| revision < newest) {
                    return;
                }
                self.desired_revision = Some(revision);
                if !active {
                    return;
                }
                let view = StateView::from_snapshot(*snapshot);
                if self.seen.orion_up != Some(true) {
                    state.events.publish("orion", serde_json::json!({ "reachable": true, "desired_revision": view.desired_revision }));
                    self.seen.orion_up = Some(true);
                }
                let digest = Digest { pipelines: routes::pipelines::digest(&view), resources: routes::resources::digest(&view) };
                if let Some(before) = &self.seen.last {
                    for (kind, data) in diff(before, &digest) {
                        state.events.publish(kind, data);
                    }
                }
                self.seen.last = Some(digest);
            }
            OrionFeed::Host(change) => {
                // Folded in while idle too: the bootstrap comes once per connection.
                if !apply_host_change(&mut self.host, change) || !active {
                    return;
                }
                if let Ok(metrics) = serde_json::to_value(metrics_from(Some(&HostSample::from_status(&self.host)))) {
                    state.events.publish("metrics", metrics);
                }
            }
            OrionFeed::Down(message) => {
                self.host.clear();
                // A restarted node may start from an older revision.
                self.desired_revision = None;
                if active && self.seen.orion_up != Some(false) {
                    state.events.publish("orion", serde_json::json!({ "reachable": false, "message": message }));
                    self.seen.orion_up = Some(false);
                }
            }
        }
    }

    async fn on_tick(&mut self, state: &SharedState) {
        self.ticks = self.ticks.wrapping_add(1);
        // Without Orion there are no host samples; keep `metrics` coming with what HeliOS reads
        // itself (throttling, disk).
        if self.seen.orion_up == Some(false)
            && self.ticks.is_multiple_of(METRICS_WHILE_DOWN_EVERY)
            && let Ok(metrics) = serde_json::to_value(metrics_from(None))
        {
            state.events.publish("metrics", metrics);
        }
        // OS updates do not go through Orion: the writer's state file is read directly.
        let update = serde_json::to_value(routes::update::current_status(state).await).ok();
        if self.seen.last_update.is_some()
            && self.seen.last_update != update
            && let Some(update) = &update
        {
            state.events.publish("update", update.clone());
        }
        self.seen.last_update = update;
    }
}

/// Folds a `host.*` status change into `host`; `false` when nothing changed.
pub fn apply_host_change(host: &mut BTreeMap<String, TypedConfigValue>, change: StatusChange) -> bool {
    if change.is_empty() && !change.bootstrap {
        return false;
    }
    if change.bootstrap {
        host.clear();
    }
    for entry in change.updated.into_iter().filter(|entry| entry.key.starts_with("host.")) {
        host.insert(entry.key, entry.value);
    }
    for key in change.expired {
        host.remove(&key.key);
    }
    true
}

/// Follow Orion and the device package updater's state and publish the changes while there are
/// subscribers. Nothing connects to Orion before the first subscriber.
pub fn spawn_state_watcher(state: SharedState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut watch = Watch::default();
        let mut interval = tokio::time::interval(UPDATE_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            if watch.feed.is_none() {
                state.events.wait_for_subscriber().await;
            }
            let active = state.events.subscribers() > 0;
            if active != watch.seen.active {
                watch.seen = Seen { active, ..Seen::default() };
                // A new feed starts with Orion's bootstrap snapshot. A running one pushes the next
                // snapshot only on a change, so read it once now: the new subscriber gets `orion`
                // and a baseline at once.
                if active && watch.feed.is_some() {
                    match state.orion.snapshot().await {
                        Ok(snapshot) => watch.on_orion(&state, OrionFeed::State(Box::new(snapshot))),
                        Err(error) => {
                            state.events.publish("orion", serde_json::json!({ "reachable": false, "message": error.message }));
                            watch.seen.orion_up = Some(false);
                        }
                    }
                }
            }
            let feed = watch.feed.get_or_insert_with(|| Feed::spawn(state.orion.clone()));
            let item = tokio::select! {
                item = feed.rx.recv() => Some(item),
                _ = interval.tick() => None,
            };
            match item {
                Some(Some(item)) => watch.on_orion(&state, item),
                // The feed task ended; the next turn starts a new one.
                Some(None) => watch.feed = None,
                None if active => watch.on_tick(&state).await,
                None => {}
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
        let after = Digest { pipelines: BTreeMap::from([("a".into(), serde_json::json!("stopped")), ("c".into(), serde_json::json!("starting"))]), ..Digest::default() };
        let events = diff(&before, &after);
        let changes: Vec<_> = events.iter().map(|(kind, data)| format!("{kind}:{}:{}", data["id"].as_str().unwrap_or("-"), data["change"].as_str().unwrap_or("-"))).collect();
        assert!(changes.contains(&"pipeline:a:updated".to_string()));
        assert!(changes.contains(&"pipeline:c:added".to_string()));
        assert!(changes.contains(&"pipeline:b:removed".to_string()));
    }

    #[test]
    fn host_changes_fold_into_metrics() {
        use orion::control_plane::{StatusEntry, StatusKey, StatusSubject};
        let node = StatusSubject::Node("raze".into());
        let entry = |key: &str, value| StatusEntry::new(node.clone(), key, value);
        let mut host = BTreeMap::new();
        let bootstrap = StatusChange {
            bootstrap: true,
            updated: vec![
                entry("host.cpu_busy_milli", TypedConfigValue::UInt(250)),
                entry("host.cpu1_busy_milli", TypedConfigValue::UInt(500)),
                entry("host.cpu0_busy_milli", TypedConfigValue::UInt(100)),
                entry("host.load1_milli", TypedConfigValue::UInt(1500)),
                entry("host.load5_milli", TypedConfigValue::UInt(1000)),
                entry("host.load15_milli", TypedConfigValue::UInt(500)),
                entry("host.memory_total_bytes", TypedConfigValue::UInt(4 << 30)),
                entry("host.memory_available_bytes", TypedConfigValue::UInt(3 << 30)),
                entry("host.temperature.cpu-thermal", TypedConfigValue::Int(52_500)),
                entry("host.temperature.rp1_adc", TypedConfigValue::Int(48_000)),
            ],
            expired: Vec::new(),
        };
        assert!(apply_host_change(&mut host, bootstrap));
        let sample = HostSample::from_status(&host);
        assert_eq!(sample.cpu_busy_milli, Some(250));
        assert_eq!(sample.cpu_core_busy_milli, vec![100, 500]);
        assert_eq!(sample.load_milli, Some([1500, 1000, 500]));
        let metrics = metrics_from(Some(&sample));
        assert_eq!(metrics.cpu, Some(0.25));
        assert_eq!(metrics.cpu_cores, vec![0.1, 0.5]);
        assert_eq!(metrics.temperature_c, Some(52.5));
        assert_eq!(metrics.load, Some([1.5, 1.0, 0.5]));

        let expire = StatusChange {
            bootstrap: false,
            updated: vec![entry("host.cpu_busy_milli", TypedConfigValue::UInt(900))],
            expired: vec![StatusKey { subject: node.clone(), key: "host.temperature.cpu-thermal".into() }],
        };
        assert!(apply_host_change(&mut host, expire));
        let metrics = metrics_from(Some(&HostSample::from_status(&host)));
        assert_eq!(metrics.cpu, Some(0.9));
        assert_eq!(metrics.temperature_c, Some(48.0));
        assert!(!apply_host_change(&mut host, StatusChange { bootstrap: false, updated: Vec::new(), expired: Vec::new() }));
    }

    #[test]
    fn filter_parses_types() {
        let filter = EventFilter { types: Some("update, pipeline,".into()) };
        assert_eq!(filter.kinds().expect("kinds"), BTreeSet::from(["update".to_string(), "pipeline".to_string()]));
    }
}
