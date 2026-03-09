use nt_client::{Client, ClientHandle, NTAddr, NewClientOptions, subscribe::ReceivedMessage, subscribe::SubscriptionOptions};
use std::{
    collections::BTreeSet,
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Duration,
};
use tokio::{
    net::lookup_host,
    sync::{Mutex, Notify},
    task::JoinHandle,
    time::timeout,
};

static NEXT_CLIENT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Nt4ClientKey {
    pub addr: Ipv4Addr,
    pub port: u16,
}

#[derive(Debug)]
pub struct Nt4ClientEntry {
    id: u64,
    handle: ClientHandle,
    ready: Arc<Notify>,
    connected: AtomicBool,
}

impl Nt4ClientEntry {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn handle(&self) -> &ClientHandle {
        &self.handle
    }

    pub async fn wait_ready(&self, timeout_duration: Duration) -> Result<(), String> {
        if self.connected.load(Ordering::Acquire) {
            return Ok(());
        }
        timeout(timeout_duration, self.ready.notified()).await.map_err(|_| "nt4 connection timed out".to_string())?;
        if self.connected.load(Ordering::Acquire) { Ok(()) } else { Err("nt4 connection timed out".to_string()) }
    }

    pub async fn list_topics_prefix(&self, prefix: &str, wait: Duration) -> Result<Vec<String>, String> {
        let options = SubscriptionOptions { topics_only: Some(true), prefix: Some(true), all: Some(true), periodic: Some(Duration::from_millis(50)), ..Default::default() };

        let topic = self.handle.topic(prefix.to_string());
        let mut subscriber = topic.subscribe(options).await.map_err(|e| e.to_string())?;

        let deadline = tokio::time::Instant::now() + wait;
        let mut out: BTreeSet<String> = BTreeSet::new();

        for announced in subscriber.topics().await.into_values() {
            out.insert(announced.name().to_string());
        }

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match timeout(remaining, subscriber.recv()).await {
                Ok(Ok(ReceivedMessage::Announced(topic))) => {
                    out.insert(topic.name().to_string());
                }
                Ok(Ok(_)) => {}
                Ok(Err(err)) => return Err(err.to_string()),
                Err(_) => break,
            }
        }

        Ok(out.into_iter().collect())
    }
}

#[derive(Debug, Default)]
pub struct Nt4ClientPool {
    clients: Arc<Mutex<HashMap<Nt4ClientKey, Nt4ClientSlot>>>,
}

#[derive(Debug)]
struct Nt4ClientSlot {
    id: u64,
    entry: Arc<Nt4ClientEntry>,
    connect_task: JoinHandle<()>,
}

impl Nt4ClientPool {
    pub fn new() -> Self {
        Self { clients: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub async fn get_or_connect(&self, host: &str, port: u16, name: &str) -> Result<Arc<Nt4ClientEntry>, String> {
        let addr = resolve_ipv4(host, port).await?;
        let key = Nt4ClientKey { addr, port };

        {
            let clients = self.clients.lock().await;
            if let Some(existing) = clients.get(&key) {
                return Ok(existing.entry.clone());
            }
        }

        let id = NEXT_CLIENT_ID.fetch_add(1, Ordering::Relaxed);
        let ready = Arc::new(Notify::new());

        let options = NewClientOptions {
            addr: NTAddr::Custom(addr),
            unsecure_port: port,
            secure_port: None,
            name: name.to_string(),
            response_timeout: Duration::from_millis(750),
            ping_interval: Duration::from_millis(200),
            update_time_interval: Duration::from_secs(5),
        };

        let client = Client::new(options);
        let handle = client.handle().clone();

        let entry = Arc::new(Nt4ClientEntry { id, handle, ready, connected: AtomicBool::new(false) });

        let clients_cleanup = self.clients.clone();
        let key_cleanup = key.clone();
        let entry_cleanup = entry.clone();
        let connect_task = tokio::spawn(async move {
            let connect_result = client
                .connect_setup(|_| {
                    entry_cleanup.connected.store(true, Ordering::Release);
                    entry_cleanup.ready.notify_waiters();
                })
                .await;

            // Any exit path (disconnect / error) should mark the slot unhealthy so we retry next time.
            if connect_result.is_err() || entry_cleanup.connected.swap(false, Ordering::AcqRel) {
                entry_cleanup.ready.notify_waiters();
            }

            let mut clients = clients_cleanup.lock().await;
            let should_remove = clients.get(&key_cleanup).map(|slot| slot.id == id).unwrap_or(false);
            if should_remove {
                clients.remove(&key_cleanup);
            }
        });

        let mut clients = self.clients.lock().await;
        clients.insert(key, Nt4ClientSlot { id, entry: entry.clone(), connect_task });
        Ok(entry)
    }

    pub async fn disconnect(&self, host: &str, port: u16) -> Result<bool, String> {
        let addr = resolve_ipv4(host, port).await?;
        let key = Nt4ClientKey { addr, port };

        let slot = {
            let mut clients = self.clients.lock().await;
            clients.remove(&key)
        };
        if let Some(slot) = slot {
            slot.connect_task.abort();
            return Ok(true);
        }
        Ok(false)
    }
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
