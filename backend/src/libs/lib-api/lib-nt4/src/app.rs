use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value as JsonValue;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{error, warn};
use url::Url;

use crate::types::{PubHandler, PubSubHandler, SubHandler, SubHandlerFn};
use lib_asyncapi::{AsyncApiDoc, AsyncApiInfo, ExternalDocs, Server, Tag, WsDoc, WsDocList};
use lib_transport::{HeartbeatConfig, HeartbeatGuard, TransportContext};
use nt_client::{data::r#type::NetworkTableData, error::ReconnectError, reconnect, ClientHandle, NewClientOptions};

#[derive(Clone)]
struct PubEntry {
    spawn: Arc<dyn Fn(Arc<Nt4App>, ClientHandle, HeartbeatConfig) -> JoinHandle<()> + Send + Sync>,
    doc: Option<WsDoc>,
}

#[derive(Clone)]
struct SubEntry {
    path: &'static str,
    handler: SubHandlerFn,
    doc: Option<WsDoc>,
}

#[derive(Clone)]
struct PubSubEntry {
    spawn: Arc<dyn Fn(Arc<Nt4App>, ClientHandle, HeartbeatConfig) -> JoinHandle<()> + Send + Sync>,
    doc: Option<WsDoc>,
}

pub struct Nt4App {
    options: NewClientOptions,
    pub_handlers: Vec<PubEntry>,
    sub_handlers: Vec<SubEntry>,
    pubsub_handlers: Vec<PubSubEntry>,
    ctx: TransportContext,
    heartbeat: HeartbeatConfig,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl Default for Nt4App {
    fn default() -> Self {
        Self::new()
    }
}

impl Nt4App {
    pub fn new() -> Self {
        Self::with_options(Default::default())
    }

    pub fn with_options(options: NewClientOptions) -> Self {
        Self {
            options,
            pub_handlers: Vec::new(),
            sub_handlers: Vec::new(),
            pubsub_handlers: Vec::new(),
            ctx: TransportContext::new(),
            heartbeat: HeartbeatConfig::default(),
            tasks: Mutex::new(Vec::new()),
        }
    }

    pub fn set_options(&mut self, options: NewClientOptions) -> &mut Self {
        self.options = options;
        self
    }

    pub fn configure_heartbeat(&mut self, cfg: HeartbeatConfig) -> &mut Self {
        self.heartbeat = cfg;
        self
    }

    pub fn set_update_interval(&mut self, interval: Duration) -> &mut Self {
        self.heartbeat = HeartbeatConfig::new(interval, self.heartbeat.timeout());
        self
    }

    pub fn update_interval(&self) -> Duration {
        self.heartbeat.interval()
    }

    pub fn heartbeat(&self) -> HeartbeatConfig {
        self.heartbeat
    }

    pub fn add_data<T: Any + Send + Sync>(&mut self, data: Arc<T>) {
        self.ctx.add_data(data);
    }

    pub fn get_data<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.ctx.get_data::<T>()
    }

    pub fn set_api_info(&mut self, info: AsyncApiInfo) -> &mut Self {
        self.ctx.metadata_mut().set_info(info);
        self
    }

    pub fn set_api_id(&mut self, id: Url) -> &mut Self {
        self.ctx.metadata_mut().set_id(id);
        self
    }

    pub fn add_api_tag(&mut self, tag: Tag) -> &mut Self {
        self.ctx.metadata_mut().add_tag(tag);
        self
    }

    pub fn set_external_docs(&mut self, docs: ExternalDocs) -> &mut Self {
        self.ctx.metadata_mut().set_external_docs(docs);
        self
    }

    pub fn add_server<S: Into<String>>(&mut self, name: S, server: Server) -> &mut Self {
        self.ctx.metadata_mut().add_server(name, server);
        self
    }

    pub fn add_pub<H>(&mut self, _h: H) -> &mut Self
    where
        H: PubHandler + lib_asyncapi::DocumentedCommand + 'static,
    {
        self.ctx.schemas_mut().track::<H, _>(H::register_schemas);
        let spawn = Arc::new(|app: Arc<Nt4App>, handle: ClientHandle, hb_cfg: HeartbeatConfig| {
            tokio::spawn(async move {
                let topic = handle.topic(H::PATH);
                let publisher = match topic.generic_publish_bypass(<H::Output as NetworkTableData>::data_type(), Default::default()).await {
                    Ok(p) => p,
                    Err(err) => {
                        error!(error = ?err, path = H::PATH, "failed to initialise NT4 publisher");
                        return;
                    }
                };
                let mut ticker = tokio::time::interval(hb_cfg.interval());
                let mut guard = HeartbeatGuard::new(hb_cfg);
                loop {
                    ticker.tick().await;
                    if let Err(err) = guard.ensure_alive() {
                        warn!(path = H::PATH, %err, "publish loop missed heartbeat deadline");
                    }
                    let val = H::execute(app.clone()).await;
                    match publisher.set(val).await {
                        Ok(_) => guard.mark(),
                        Err(err) => warn!(error = ?err, path = H::PATH, "failed to publish NT4 value"),
                    }
                }
            })
        });
        let doc = H::doc();
        self.pub_handlers.push(PubEntry { spawn, doc });
        self
    }

    pub fn add_sub<H>(&mut self, _h: H) -> &mut Self
    where
        H: SubHandler + lib_asyncapi::DocumentedCommand + 'static,
    {
        self.ctx.schemas_mut().track::<H, _>(H::register_schemas);
        let doc = H::doc();
        self.sub_handlers.push(SubEntry { path: H::PATH, handler: Arc::new(|app, p, val| H::execute(app, p, val)), doc });
        self
    }

    pub fn add_pubsub<H>(&mut self, _h: H) -> &mut Self
    where
        H: PubSubHandler + lib_asyncapi::DocumentedCommand + 'static,
    {
        self.ctx.schemas_mut().track::<H, _>(H::register_schemas);
        let spawn = Arc::new(|app: Arc<Nt4App>, handle: ClientHandle, hb_cfg: HeartbeatConfig| {
            tokio::spawn(async move {
                let mut guard = HeartbeatGuard::new(hb_cfg);
                let topic = handle.topic(H::PATH);
                let has_var = H::PATTERN.contains('{');
                let mut sub = match topic.subscribe(Default::default()).await {
                    Ok(sub) => sub,
                    Err(err) => {
                        error!(error = ?err, path = H::PATH, "failed to subscribe to NT4 topic");
                        return;
                    }
                };
                let static_pub = if !has_var {
                    match topic.generic_publish_bypass(<H::Output as NetworkTableData>::data_type(), Default::default()).await {
                        Ok(publisher) => {
                            if let Err(err) = publisher.set(<H::Output as Default>::default()).await {
                                warn!(error = ?err, path = H::PATH, "failed to seed NT4 pubsub publisher");
                            } else {
                                guard.mark();
                            }
                            Some(publisher)
                        }
                        Err(err) => {
                            error!(error = ?err, path = H::PATH, "failed to initialise NT4 pubsub publisher");
                            None
                        }
                    }
                } else {
                    None
                };

                while let Ok(msg) = sub.recv().await {
                    if let Err(err) = guard.ensure_alive() {
                        warn!(path = H::PATH, %err, "pubsub loop missed heartbeat deadline");
                    }
                    if let nt_client::subscribe::ReceivedMessage::Updated((topic, val)) = msg {
                        let val_json = serde_json::to_value(&val).unwrap_or(JsonValue::Null);
                        let out = H::execute(app.clone(), topic.name().to_string(), val_json).await;
                        if let Some(pubh) = &static_pub {
                            if let Err(err) = pubh.set(out).await {
                                warn!(error = ?err, path = H::PATH, "failed to publish NT4 pubsub result");
                            } else {
                                guard.mark();
                            }
                        } else {
                            let p_topic = handle.topic(topic.name());
                            match p_topic.generic_publish_bypass(<H::Output as NetworkTableData>::data_type(), Default::default()).await {
                                Ok(publisher) => {
                                    if let Err(err) = publisher.set(out).await {
                                        warn!(error = ?err, path = H::PATH, "failed to publish dynamic NT4 result");
                                    } else {
                                        guard.mark();
                                    }
                                }
                                Err(err) => warn!(error = ?err, path = topic.name(), "failed to initialise dynamic NT4 publisher"),
                            }
                        }
                    }
                }
            })
        });
        let doc = H::doc();
        self.pubsub_handlers.push(PubSubEntry { spawn, doc });
        self
    }

    pub async fn start(self: Arc<Self>) {
        let app = self.clone();
        let options = self.options.clone();
        let handle = tokio::spawn(async move {
            let _ = reconnect(options, |client| {
                let app = app.clone();
                async move {
                    let handle = client.handle().clone();
                    app.spawn_handlers(handle).await;
                    client.connect().await.map_err(|e| ReconnectError::Nonfatal(e.into()))
                }
            })
            .await;
        });
        self.tasks.lock().await.push(handle);
    }

    async fn spawn_handlers(self: Arc<Self>, handle: ClientHandle) {
        let hb = self.heartbeat;
        for entry in self.pub_handlers.clone() {
            let app = self.clone();
            let handle_cloned = handle.clone();
            let spawn = entry.spawn.clone();
            let join = (spawn)(app, handle_cloned, hb);
            self.tasks.lock().await.push(join);
        }
        for entry in self.pubsub_handlers.clone() {
            let app = self.clone();
            let handle_cloned = handle.clone();
            let spawn = entry.spawn.clone();
            let join = (spawn)(app, handle_cloned, hb);
            self.tasks.lock().await.push(join);
        }
        for entry in self.sub_handlers.clone() {
            let app = self.clone();
            let topic = handle.topic(entry.path);
            let h = entry.handler.clone();
            let join = tokio::spawn(async move {
                let mut sub = match topic.subscribe(Default::default()).await {
                    Ok(sub) => sub,
                    Err(err) => {
                        error!(error = ?err, path = entry.path, "failed to subscribe to NT4 topic");
                        return;
                    }
                };
                while let Ok(msg) = sub.recv().await {
                    if let nt_client::subscribe::ReceivedMessage::Updated((topic, val)) = msg {
                        let val_json = serde_json::to_value(&val).unwrap_or(JsonValue::Null);
                        h(app.clone(), topic.name().to_string(), val_json).await;
                    }
                }
            });
            self.tasks.lock().await.push(join);
        }
    }

    pub fn docs(&self) -> WsDocList {
        self.pub_handlers.iter().filter_map(|p| p.doc.clone()).chain(self.sub_handlers.iter().filter_map(|s| s.doc.clone())).chain(self.pubsub_handlers.iter().filter_map(|p| p.doc.clone())).collect()
    }

    pub fn asyncapi_doc(&self) -> AsyncApiDoc {
        let docs = self.docs();
        self.ctx.asyncapi_doc(&docs, &[])
    }

    pub fn asyncapi_json(&self) -> serde_json::Value {
        self.ctx.asyncapi_json(&self.docs(), &[])
    }
}
