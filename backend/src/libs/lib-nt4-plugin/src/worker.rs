use crate::rmpv_json;
use daedalus::runtime::NodeError;
use nt_client::data::{DataType, JsonString};
use nt_client::subscribe::{ReceivedMessage, SubscriptionOptions};
use nt_client::topic::Properties;
use nt_client::{Client, ClientHandle, NTAddr, NewClientOptions};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::lookup_host;
use tokio::sync::{Notify, mpsc};
use tokio::task::JoinHandle;
use tracing::warn;

pub struct Nt4WorkerHandle {
    tx: mpsc::UnboundedSender<Command>,
    values: Arc<Mutex<HashMap<String, Arc<Mutex<String>>>>>,
}

impl Nt4WorkerHandle {
    pub fn publish_json(&self, topic: String, json: String) -> bool {
        self.tx.send(Command::Publish { topic, value: PublishValue::Json(json) }).is_ok()
    }

    pub fn publish_bool(&self, topic: String, value: bool) -> bool {
        self.tx.send(Command::Publish { topic, value: PublishValue::Boolean(value) }).is_ok()
    }

    pub fn publish_int(&self, topic: String, value: i64) -> bool {
        self.tx.send(Command::Publish { topic, value: PublishValue::Int(value) }).is_ok()
    }

    pub fn publish_double(&self, topic: String, value: f64) -> bool {
        self.tx.send(Command::Publish { topic, value: PublishValue::Double(value) }).is_ok()
    }

    pub fn publish_string(&self, topic: String, value: String) -> bool {
        self.tx.send(Command::Publish { topic, value: PublishValue::String(value) }).is_ok()
    }

    pub fn subscribe_json(&self, topic: String) -> String {
        let slot = {
            let mut guard = self.values.lock().expect("values mutex poisoned");
            guard.entry(topic.clone()).or_insert_with(|| Arc::new(Mutex::new(String::new()))).clone()
        };
        let _ = self.tx.send(Command::SubscribeJson { topic, slot: slot.clone() });
        slot.lock().expect("slot mutex poisoned").clone()
    }
}

pub fn spawn(host: String, port: u16) -> Result<Nt4WorkerHandle, NodeError> {
    tokio::runtime::Handle::try_current().map_err(|_| NodeError::Handler("nt4 nodes require a tokio runtime".into()))?;

    let (tx, rx) = mpsc::unbounded_channel();
    let values: Arc<Mutex<HashMap<String, Arc<Mutex<String>>>>> = Arc::new(Mutex::new(HashMap::new()));

    let values_for_task = values.clone();
    tokio::spawn(async move {
        run_worker(host, port, rx, values_for_task).await;
    });

    Ok(Nt4WorkerHandle { tx, values })
}

enum Command {
    Publish { topic: String, value: PublishValue },
    SubscribeJson { topic: String, slot: Arc<Mutex<String>> },
}

#[derive(Debug, Clone)]
enum PublishValue {
    Json(String),
    Boolean(bool),
    Double(f64),
    Int(i64),
    String(String),
}

struct Connection {
    handle: ClientHandle,
    task: JoinHandle<()>,
}

async fn run_worker(host: String, port: u16, mut rx: mpsc::UnboundedReceiver<Command>, values: Arc<Mutex<HashMap<String, Arc<Mutex<String>>>>>) {
    let mut conn: Option<Connection> = None;
    let mut publishers: HashMap<String, PublisherEntry> = HashMap::new();
    let mut subscriptions: HashMap<String, JoinHandle<()>> = HashMap::new();
    let mut subscribed_topics: HashMap<String, Arc<Mutex<String>>> = HashMap::new();

    let mut tick = tokio::time::interval(Duration::from_secs(2));

    loop {
        tokio::select! {
            _ = tick.tick() => {
                if let Some(existing) = conn.as_ref() && existing.task.is_finished() {
                    conn = None;
                    publishers.clear();
                    for (_, task) in subscriptions.drain() {
                        task.abort();
                    }
                }
                if conn.is_none() && (!subscribed_topics.is_empty() || !publishers.is_empty()) {
                    conn = connect(&host, port).await.ok();
                }
                if let Some(connection) = conn.as_ref() {
                    ensure_subscriptions(connection.handle.clone(), &mut subscriptions, &subscribed_topics).await;
                }
                // Bring in any new subscription slots from the shared map (subscribe_json can be called without a command being received yet).
                let snapshot = values.lock().ok().map(|m| m.clone()).unwrap_or_default();
                for (topic, slot) in snapshot {
                    subscribed_topics.entry(topic).or_insert(slot);
                }
            }
            cmd = rx.recv() => {
                let Some(cmd) = cmd else { break; };
                match cmd {
                    Command::SubscribeJson { topic, slot } => {
                        subscribed_topics.insert(topic.clone(), slot);
                        if conn.is_none() {
                            conn = connect(&host, port).await.ok();
                        }
                        if let Some(connection) = conn.as_ref() {
                            ensure_subscriptions(connection.handle.clone(), &mut subscriptions, &subscribed_topics).await;
                        }
                    }
                    Command::Publish { topic, value } => {
                        if conn.is_none() {
                            conn = connect(&host, port).await.ok();
                        }
                        let Some(connection) = conn.as_ref() else { continue; };
                        if let Err(err) = publish_value(&connection.handle, &mut publishers, &topic, value).await {
                            warn!(%err, "nt4 publish failed");
                            conn = None;
                            publishers.clear();
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
struct PublisherEntry {
    data_type: DataType,
    publisher: nt_client::publish::GenericPublisher,
}

async fn publish_value(handle: &ClientHandle, publishers: &mut HashMap<String, PublisherEntry>, topic: &str, value: PublishValue) -> Result<(), String> {
    let data_type = match &value {
        PublishValue::Json(_) => DataType::Json,
        PublishValue::Boolean(_) => DataType::Boolean,
        PublishValue::Double(_) => DataType::Double,
        PublishValue::Int(_) => DataType::Int,
        PublishValue::String(_) => DataType::String,
    };

    let needs_new_publisher = match publishers.get(topic) {
        Some(existing) => existing.data_type != data_type,
        None => true,
    };
    if needs_new_publisher {
        let publisher = handle.topic(topic.to_string()).generic_publish(data_type.clone(), Properties::default()).await.map_err(|e| e.to_string())?;
        publishers.insert(topic.to_string(), PublisherEntry { data_type, publisher });
    }

    let entry = publishers.get(topic).expect("publisher initialized");
    match value {
        PublishValue::Json(json) => entry.publisher.set(JsonString(json)).await.map_err(|e| e.to_string())?,
        PublishValue::Boolean(v) => entry.publisher.set(v).await.map_err(|e| e.to_string())?,
        PublishValue::Double(v) => entry.publisher.set(v).await.map_err(|e| e.to_string())?,
        PublishValue::Int(v) => entry.publisher.set(v).await.map_err(|e| e.to_string())?,
        PublishValue::String(v) => entry.publisher.set(v).await.map_err(|e| e.to_string())?,
    }
    Ok(())
}

async fn ensure_subscriptions(handle: ClientHandle, tasks: &mut HashMap<String, JoinHandle<()>>, subscribed: &HashMap<String, Arc<Mutex<String>>>) {
    for (topic, slot) in subscribed {
        if tasks.contains_key(topic) {
            continue;
        }
        let topic_name = topic.clone();
        let slot = slot.clone();
        let handle = handle.clone();
        tasks.insert(
            topic.clone(),
            tokio::spawn(async move {
                let options = SubscriptionOptions { all: Some(true), periodic: Some(Duration::from_millis(50)), ..Default::default() };

                let topic = handle.topic(topic_name.clone());
                let mut subscriber = match topic.subscribe(options).await {
                    Ok(sub) => sub,
                    Err(_) => return,
                };

                loop {
                    match subscriber.recv().await {
                        Ok(ReceivedMessage::Updated((_announced, value))) => {
                            let json_value = rmpv_json::to_json(&value);
                            if let (Ok(serialized), Ok(mut guard)) = (serde_json::to_string(&json_value), slot.lock()) {
                                *guard = serialized;
                            }
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            }),
        );
    }
}

async fn connect(host: &str, port: u16) -> Result<Connection, String> {
    let addr = resolve_ipv4(host, port).await?;
    let ready = Arc::new(Notify::new());
    let ready_connect = ready.clone();
    #[allow(clippy::needless_update)]
    let options = NewClientOptions {
        addr: NTAddr::Custom(addr),
        unsecure_port: port,
        secure_port: None,
        name: "Helios".to_string(),
        response_timeout: Duration::from_millis(750),
        ping_interval: Duration::from_millis(200),
        update_time_interval: Duration::from_secs(5),
        ..Default::default()
    };

    let client = Client::new(options);
    let handle = client.handle().clone();

    let task = tokio::spawn(async move {
        let _ = client
            .connect_setup(|_| {
                ready_connect.notify_waiters();
            })
            .await;
    });

    tokio::time::timeout(Duration::from_millis(1500), ready.notified()).await.map_err(|_| "nt4 connection timed out".to_string())?;

    Ok(Connection { handle, task })
}

async fn resolve_ipv4(host: &str, port: u16) -> Result<Ipv4Addr, String> {
    if let Ok(addr) = host.parse::<Ipv4Addr>() {
        return Ok(addr);
    }
    let candidates = lookup_host((host, port)).await.map_err(|e| format!("failed to resolve nt4 host {host}: {e}"))?;
    for addr in candidates {
        if let SocketAddr::V4(v4) = addr {
            return Ok(*v4.ip());
        }
    }
    Err(format!("no ipv4 address found for host {host}"))
}
