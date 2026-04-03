use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

use helios_engine::ipc::{EngineEvent, GraphOutputPortDescriptor};
use helios_engine::stream::StreamMetrics;
use tokio::sync::{Mutex, RwLock, broadcast};
use tokio::task::JoinHandle;
use tokio::time::{Duration, Instant};

use crate::ipc::IpcHandles;

use super::state::SystemReadModelState;

#[derive(Debug, Clone)]
pub struct SharedStreamMetricsSnapshot {
    pub stream_id: uuid::Uuid,
    pub metrics: StreamMetrics,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone)]
pub struct SharedStreamOutputsPortsSnapshot {
    pub outputs: Vec<GraphOutputPortDescriptor>,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone)]
pub struct SharedStreamOutputSample {
    pub port: String,
    pub value: Option<serde_json::Value>,
    pub error: Option<String>,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone)]
pub enum SharedStreamOutputsEvent {
    Ports(SharedStreamOutputsPortsSnapshot),
    Sample(SharedStreamOutputSample),
}

struct StreamMetricsTopic {
    tx: broadcast::Sender<Arc<SharedStreamMetricsSnapshot>>,
    latest: Arc<RwLock<Option<Arc<SharedStreamMetricsSnapshot>>>>,
}

pub(super) struct StreamMetricsHub {
    topics: Arc<RwLock<BTreeMap<uuid::Uuid, Arc<StreamMetricsTopic>>>>,
    task: Mutex<Option<JoinHandle<()>>>,
    state: RwLock<Option<Weak<IpcHandles>>>,
}

#[derive(Debug, Clone)]
struct StreamOutputsClientConfig {
    ports: Vec<String>,
    sample_interval: Duration,
    ports_interval: Duration,
}

struct StreamOutputsTopic {
    tx: broadcast::Sender<Arc<SharedStreamOutputsEvent>>,
    latest_ports: Arc<RwLock<Option<Arc<SharedStreamOutputsPortsSnapshot>>>>,
    clients: Arc<RwLock<BTreeMap<uuid::Uuid, StreamOutputsClientConfig>>>,
    task: Mutex<Option<JoinHandle<()>>>,
}

pub(super) struct StreamOutputsHub {
    topics: Arc<RwLock<BTreeMap<uuid::Uuid, Arc<StreamOutputsTopic>>>>,
    state: RwLock<Option<Weak<IpcHandles>>>,
}

impl StreamMetricsTopic {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(32);
        Self { tx, latest: Arc::new(RwLock::new(None)) }
    }
}

impl StreamOutputsTopic {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self { tx, latest_ports: Arc::new(RwLock::new(None)), clients: Arc::new(RwLock::new(BTreeMap::new())), task: Mutex::new(None) }
    }

    async fn ensure_task(&self, stream_id: uuid::Uuid, state: Option<Weak<IpcHandles>>) {
        let mut guard = self.task.lock().await;
        let needs_spawn = guard.as_ref().map(|handle| handle.is_finished()).unwrap_or(true);
        if needs_spawn {
            let tx = self.tx.clone();
            let latest_ports = self.latest_ports.clone();
            let clients = self.clients.clone();
            *guard = Some(tokio::spawn(run_stream_outputs_sampler(stream_id, tx, latest_ports, clients, state)));
        }
    }

    async fn prime_ports(&self, stream_id: uuid::Uuid, state: Option<Weak<IpcHandles>>) -> Result<Arc<SharedStreamOutputsPortsSnapshot>, String> {
        if let Some(snapshot) = self.latest_ports.read().await.clone() {
            return Ok(snapshot);
        }

        let Some(state) = state.and_then(|weak| weak.upgrade()) else {
            return Err("engine unavailable".into());
        };

        let snapshot = Arc::new(fetch_stream_outputs_ports(&state, stream_id).await?);
        let mut guard = self.latest_ports.write().await;
        if guard.is_none() {
            *guard = Some(snapshot.clone());
        }
        let _ = self.tx.send(Arc::new(SharedStreamOutputsEvent::Ports((*snapshot).clone())));
        Ok(snapshot)
    }
}

impl StreamMetricsHub {
    pub(super) fn new() -> Self {
        Self { topics: Arc::new(RwLock::new(BTreeMap::new())), task: Mutex::new(None), state: RwLock::new(None) }
    }

    pub(super) async fn set_state(&self, state: &Arc<IpcHandles>) {
        let mut guard = self.state.write().await;
        if guard.as_ref().and_then(|weak| weak.upgrade()).is_none() {
            *guard = Some(Arc::downgrade(state));
        }
    }

    pub(super) async fn subscribe(&self, stream_id: uuid::Uuid) -> Result<(broadcast::Receiver<Arc<SharedStreamMetricsSnapshot>>, Option<Arc<SharedStreamMetricsSnapshot>>), String> {
        self.ensure_task().await;
        let topic = self.topic(stream_id).await;
        self.prime_topic(stream_id, &topic).await?;
        let latest = topic.latest.read().await.clone();
        Ok((topic.tx.subscribe(), latest))
    }

    async fn ensure_task(&self) {
        let mut guard = self.task.lock().await;
        let needs_spawn = guard.as_ref().map(|handle| handle.is_finished()).unwrap_or(true);
        if needs_spawn {
            let topics = self.topics.clone();
            let state = self.state.read().await.clone();
            *guard = Some(tokio::spawn(run_stream_metrics_sampler(topics, state)));
        }
    }

    async fn topic(&self, stream_id: uuid::Uuid) -> Arc<StreamMetricsTopic> {
        let mut guard = self.topics.write().await;
        guard.entry(stream_id).or_insert_with(|| Arc::new(StreamMetricsTopic::new())).clone()
    }

    pub(super) async fn stats(&self) -> (u64, u64) {
        let guard = self.topics.read().await;
        let topics = guard.len() as u64;
        let subscribers = guard.values().map(|topic| topic.tx.receiver_count() as u64).sum();
        (topics, subscribers)
    }

    pub(super) async fn unsubscribe(&self, stream_id: uuid::Uuid) {
        let Some(topic) = self.find_topic(stream_id).await else {
            return;
        };
        if topic.tx.receiver_count() > 0 {
            return;
        }
        topic.latest.write().await.take();
        self.topics.write().await.remove(&stream_id);
    }

    async fn prime_topic(&self, stream_id: uuid::Uuid, topic: &Arc<StreamMetricsTopic>) -> Result<(), String> {
        if topic.latest.read().await.is_some() {
            return Ok(());
        }

        let Some(state) = self.state.read().await.as_ref().and_then(|weak| weak.upgrade()) else {
            return Err("engine unavailable".into());
        };

        let snapshot = match state.engine.get_metrics(stream_id).await {
            Ok(EngineEvent::Metrics { metrics, .. }) => Arc::new(build_stream_metrics_snapshot(stream_id, metrics)),
            Ok(EngineEvent::Nack { reason, .. }) => return Err(reason),
            Ok(_) => return Err("unexpected engine response".into()),
            Err(err) => return Err(err.to_string()),
        };

        let mut guard = topic.latest.write().await;
        if guard.is_none() {
            *guard = Some(snapshot.clone());
        }
        let _ = topic.tx.send(snapshot);
        Ok(())
    }

    async fn find_topic(&self, stream_id: uuid::Uuid) -> Option<Arc<StreamMetricsTopic>> {
        self.topics.read().await.get(&stream_id).cloned()
    }
}

impl StreamOutputsHub {
    pub(super) fn new() -> Self {
        Self { topics: Arc::new(RwLock::new(BTreeMap::new())), state: RwLock::new(None) }
    }

    pub(super) async fn set_state(&self, state: &Arc<IpcHandles>) {
        let mut guard = self.state.write().await;
        if guard.as_ref().and_then(|weak| weak.upgrade()).is_none() {
            *guard = Some(Arc::downgrade(state));
        }
    }

    pub(super) async fn subscribe(
        &self,
        stream_id: uuid::Uuid,
        sample_interval: Duration,
        ports_interval: Duration,
    ) -> Result<(uuid::Uuid, broadcast::Receiver<Arc<SharedStreamOutputsEvent>>, Arc<SharedStreamOutputsPortsSnapshot>), String> {
        let topic = self.topic(stream_id).await;
        let state = self.state.read().await.clone();
        topic.ensure_task(stream_id, state.clone()).await;

        let client_id = uuid::Uuid::new_v4();
        topic.clients.write().await.insert(client_id, StreamOutputsClientConfig { ports: Vec::new(), sample_interval, ports_interval });

        match topic.prime_ports(stream_id, state).await {
            Ok(snapshot) => Ok((client_id, topic.tx.subscribe(), snapshot)),
            Err(err) => {
                topic.clients.write().await.remove(&client_id);
                Err(err)
            }
        }
    }

    pub(super) async fn update_client(&self, stream_id: uuid::Uuid, client_id: uuid::Uuid, ports: Vec<String>, sample_interval: Duration) -> Result<(), String> {
        let Some(topic) = self.find_topic(stream_id).await else {
            return Err("stream outputs subscription unavailable".into());
        };
        let mut clients = topic.clients.write().await;
        let Some(client) = clients.get_mut(&client_id) else {
            return Err("stream outputs subscriber missing".into());
        };
        client.ports = ports;
        client.sample_interval = sample_interval;
        Ok(())
    }

    pub(super) async fn unsubscribe(&self, stream_id: uuid::Uuid, client_id: uuid::Uuid) {
        let Some(topic) = self.find_topic(stream_id).await else {
            return;
        };
        let idle = {
            let mut clients = topic.clients.write().await;
            clients.remove(&client_id);
            clients.is_empty()
        };
        if !idle || topic.tx.receiver_count() > 0 {
            return;
        }

        *topic.latest_ports.write().await = None;

        let mut topics = self.topics.write().await;
        if topics.get(&stream_id).is_some_and(|current| Arc::ptr_eq(current, &topic)) {
            topics.remove(&stream_id);
        }
    }

    pub(super) async fn current_ports(&self, stream_id: uuid::Uuid) -> Result<Arc<SharedStreamOutputsPortsSnapshot>, String> {
        let topic = self.topic(stream_id).await;
        let state = self.state.read().await.clone();
        topic.ensure_task(stream_id, state.clone()).await;
        topic.prime_ports(stream_id, state).await
    }

    async fn topic(&self, stream_id: uuid::Uuid) -> Arc<StreamOutputsTopic> {
        let mut guard = self.topics.write().await;
        guard.entry(stream_id).or_insert_with(|| Arc::new(StreamOutputsTopic::new())).clone()
    }

    async fn find_topic(&self, stream_id: uuid::Uuid) -> Option<Arc<StreamOutputsTopic>> {
        self.topics.read().await.get(&stream_id).cloned()
    }

    pub(super) async fn stats(&self) -> (u64, u64) {
        let topics = self.topics.read().await.values().cloned().collect::<Vec<_>>();
        let topic_count = topics.len() as u64;
        let mut subscribers = 0_u64;
        for topic in topics {
            subscribers = subscribers.saturating_add(topic.clients.read().await.len() as u64);
        }
        (topic_count, subscribers)
    }
}

impl SystemReadModelState {
    pub async fn bind_stream_metrics_state(&self, state: &crate::http::AppState) {
        self.stream_metrics_hub.set_state(state.ipc()).await;
    }

    pub async fn subscribe_stream_metrics(&self, stream_id: uuid::Uuid) -> Result<(broadcast::Receiver<Arc<SharedStreamMetricsSnapshot>>, Option<Arc<SharedStreamMetricsSnapshot>>), String> {
        self.stream_metrics_hub.subscribe(stream_id).await
    }

    pub async fn unsubscribe_stream_metrics(&self, stream_id: uuid::Uuid) {
        self.stream_metrics_hub.unsubscribe(stream_id).await;
    }

    pub async fn bind_stream_outputs_state(&self, state: &crate::http::AppState) {
        self.stream_outputs_hub.set_state(state.ipc()).await;
    }

    pub async fn subscribe_stream_outputs(
        &self,
        stream_id: uuid::Uuid,
        sample_interval: Duration,
        ports_interval: Duration,
    ) -> Result<(uuid::Uuid, broadcast::Receiver<Arc<SharedStreamOutputsEvent>>, Arc<SharedStreamOutputsPortsSnapshot>), String> {
        self.stream_outputs_hub.subscribe(stream_id, sample_interval, ports_interval).await
    }

    pub async fn update_stream_outputs_subscription(&self, stream_id: uuid::Uuid, client_id: uuid::Uuid, ports: Vec<String>, sample_interval: Duration) -> Result<(), String> {
        self.stream_outputs_hub.update_client(stream_id, client_id, ports, sample_interval).await
    }

    pub async fn current_stream_outputs_ports(&self, stream_id: uuid::Uuid) -> Result<Arc<SharedStreamOutputsPortsSnapshot>, String> {
        self.stream_outputs_hub.current_ports(stream_id).await
    }

    pub async fn unsubscribe_stream_outputs(&self, stream_id: uuid::Uuid, client_id: uuid::Uuid) {
        self.stream_outputs_hub.unsubscribe(stream_id, client_id).await;
    }
}

async fn run_stream_metrics_sampler(topics: Arc<RwLock<BTreeMap<uuid::Uuid, Arc<StreamMetricsTopic>>>>, state: Option<Weak<IpcHandles>>) {
    let Some(state) = state.and_then(|weak| weak.upgrade()) else {
        return;
    };

    let mut events = state.engine.subscribe_events();
    let mut saw_receiver = stream_metrics_has_receivers(&topics).await;

    loop {
        let has_receivers = stream_metrics_has_receivers(&topics).await;
        saw_receiver |= has_receivers;
        if saw_receiver && !has_receivers {
            break;
        }

        match events.recv().await {
            Ok(EngineEvent::Metrics { stream_id, metrics, .. }) | Ok(EngineEvent::MetricsUpdate { stream_id, metrics }) => {
                let Some(topic) = stream_metrics_topic(&topics, stream_id).await else {
                    continue;
                };
                let snapshot = Arc::new(build_stream_metrics_snapshot(stream_id, metrics));
                *topic.latest.write().await = Some(snapshot.clone());
                let _ = topic.tx.send(snapshot);
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

async fn run_stream_outputs_sampler(
    stream_id: uuid::Uuid,
    tx: broadcast::Sender<Arc<SharedStreamOutputsEvent>>,
    latest_ports: Arc<RwLock<Option<Arc<SharedStreamOutputsPortsSnapshot>>>>,
    clients: Arc<RwLock<BTreeMap<uuid::Uuid, StreamOutputsClientConfig>>>,
    state: Option<Weak<IpcHandles>>,
) {
    let Some(state) = state.and_then(|weak| weak.upgrade()) else {
        return;
    };

    let mut ticker = tokio::time::interval(Duration::from_millis(100));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut last_ports_refresh = Instant::now().checked_sub(Duration::from_secs(30)).unwrap_or_else(Instant::now);
    let mut next_samples = BTreeMap::<String, Instant>::new();
    let mut saw_receiver = tx.receiver_count() > 0;

    loop {
        ticker.tick().await;

        let receiver_count = tx.receiver_count();
        let client_snapshot = clients.read().await.clone();
        let has_activity = receiver_count > 0 || !client_snapshot.is_empty();
        saw_receiver |= has_activity;
        if saw_receiver && !has_activity {
            break;
        }

        let now = Instant::now();
        let ports_interval = aggregate_stream_outputs_ports_interval(&client_snapshot);
        let active_ports = aggregate_stream_output_ports(&client_snapshot);

        next_samples.retain(|port, _| active_ports.contains_key(port));

        let needs_ports_refresh = latest_ports.read().await.is_none() || now.duration_since(last_ports_refresh) >= ports_interval;
        if needs_ports_refresh {
            if let Ok(snapshot) = fetch_stream_outputs_ports(&state, stream_id).await {
                let snapshot = Arc::new(snapshot);
                let changed = latest_ports.read().await.as_ref().map(|current| current.outputs != snapshot.outputs).unwrap_or(true);
                *latest_ports.write().await = Some(snapshot.clone());
                if changed {
                    let _ = tx.send(Arc::new(SharedStreamOutputsEvent::Ports((*snapshot).clone())));
                }
            }
            last_ports_refresh = now;
        }

        for (port, interval) in &active_ports {
            let due = next_samples.get(port).copied().unwrap_or_else(|| now.checked_sub(*interval).unwrap_or(now));
            if now < due {
                continue;
            }
            let sample = fetch_stream_output_sample(&state, stream_id, port.clone()).await;
            let _ = tx.send(Arc::new(SharedStreamOutputsEvent::Sample(sample)));
            next_samples.insert(port.clone(), now + *interval);
        }
    }
}

fn build_stream_metrics_snapshot(stream_id: uuid::Uuid, metrics: StreamMetrics) -> SharedStreamMetricsSnapshot {
    SharedStreamMetricsSnapshot { stream_id, metrics, timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64 }
}

async fn fetch_stream_outputs_ports(state: &Arc<IpcHandles>, stream_id: uuid::Uuid) -> Result<SharedStreamOutputsPortsSnapshot, String> {
    match state.engine.list_graph_outputs_event(stream_id).await {
        Ok(EngineEvent::GraphOutputs { outputs, .. }) => Ok(SharedStreamOutputsPortsSnapshot { outputs, timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64 }),
        Ok(EngineEvent::Nack { code, reason, .. }) => Err(format!("engine rejected outputs list: {code:?}: {reason}")),
        Ok(_) => Err("unexpected engine response listing outputs".into()),
        Err(err) => Err(format!("engine error listing outputs: {err}")),
    }
}

async fn fetch_stream_output_sample(state: &Arc<IpcHandles>, stream_id: uuid::Uuid, port: String) -> SharedStreamOutputSample {
    let timestamp_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    match state.engine.get_graph_output_sample_event(stream_id, port.clone()).await {
        Ok(EngineEvent::GraphOutputSample { value, .. }) => SharedStreamOutputSample { port, value: Some(value.into()), error: None, timestamp_ms },
        Ok(EngineEvent::Nack { code, reason, .. }) => SharedStreamOutputSample { port, value: None, error: Some(format!("{code:?}: {reason}")), timestamp_ms },
        Ok(other) => SharedStreamOutputSample { port, value: None, error: Some(format!("unexpected engine response: {other:?}")), timestamp_ms },
        Err(err) => SharedStreamOutputSample { port, value: None, error: Some(err.to_string()), timestamp_ms },
    }
}

fn aggregate_stream_output_ports(clients: &BTreeMap<uuid::Uuid, StreamOutputsClientConfig>) -> BTreeMap<String, Duration> {
    let mut ports = BTreeMap::<String, Duration>::new();
    for client in clients.values() {
        for port in &client.ports {
            ports
                .entry(port.clone())
                .and_modify(|current| {
                    if client.sample_interval < *current {
                        *current = client.sample_interval;
                    }
                })
                .or_insert(client.sample_interval);
        }
    }
    ports
}

fn aggregate_stream_outputs_ports_interval(clients: &BTreeMap<uuid::Uuid, StreamOutputsClientConfig>) -> Duration {
    clients.values().map(|client| client.ports_interval).min().unwrap_or_else(|| Duration::from_secs(2))
}

async fn stream_metrics_topic(topics: &Arc<RwLock<BTreeMap<uuid::Uuid, Arc<StreamMetricsTopic>>>>, stream_id: uuid::Uuid) -> Option<Arc<StreamMetricsTopic>> {
    topics.read().await.get(&stream_id).cloned()
}

async fn stream_metrics_has_receivers(topics: &Arc<RwLock<BTreeMap<uuid::Uuid, Arc<StreamMetricsTopic>>>>) -> bool {
    topics.read().await.values().any(|topic| topic.tx.receiver_count() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stream_outputs_unsubscribe_prunes_idle_topic() {
        let hub = StreamOutputsHub::new();
        let stream_id = uuid::Uuid::new_v4();
        let topic = hub.topic(stream_id).await;
        let client_id = uuid::Uuid::new_v4();

        topic.clients.write().await.insert(client_id, StreamOutputsClientConfig { ports: Vec::new(), sample_interval: Duration::from_millis(10), ports_interval: Duration::from_millis(20) });

        let receiver = topic.tx.subscribe();
        assert!(hub.find_topic(stream_id).await.is_some());

        drop(receiver);
        hub.unsubscribe(stream_id, client_id).await;

        let (topic_count, subscriber_count) = hub.stats().await;
        assert_eq!(topic_count, 0);
        assert_eq!(subscriber_count, 0);
        assert!(hub.find_topic(stream_id).await.is_none());
    }

    #[tokio::test]
    async fn stream_metrics_unsubscribe_prunes_idle_topic() {
        let hub = StreamMetricsHub::new();
        let stream_id = uuid::Uuid::new_v4();
        let topic = hub.topic(stream_id).await;
        let latest = Arc::new(SharedStreamMetricsSnapshot { stream_id, metrics: StreamMetrics::default(), timestamp_ms: 1 });
        *topic.latest.write().await = Some(latest);

        let receiver = topic.tx.subscribe();
        assert!(hub.find_topic(stream_id).await.is_some());

        drop(receiver);
        hub.unsubscribe(stream_id).await;

        let (topic_count, subscriber_count) = hub.stats().await;
        assert_eq!(topic_count, 0);
        assert_eq!(subscriber_count, 0);
        assert!(hub.find_topic(stream_id).await.is_none());
    }
}
