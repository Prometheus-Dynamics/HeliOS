use std::{any::Any, cell::Cell, collections::HashMap, rc::Rc, sync::Arc, time::Duration};

use crate::Error;
use actix::{Actor, ActorContext, AsyncContext, StreamHandler};
use actix_web::web::{Bytes, Payload};
use actix_web::{HttpRequest, HttpResponse, Result as ActixResult};
use actix_web_actors::ws;
use futures::future::{BoxFuture, LocalBoxFuture};
use futures::FutureExt;
use lib_transport::{HeartbeatConfig, HeartbeatGuard, TransportContext};
use tracing::{error, warn};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WsMessage {
    Ping,
    Pong,
    Text,
    Binary,
    Close,
    Loop,
}

type MsgHandler = Arc<dyn Fn(Arc<WsApp>, &mut ws::WebsocketContext<WsSession>, &ws::Message) -> LocalBoxFuture<'static, ()> + Send + Sync>;

type GlobalHandler = Arc<dyn Fn() -> BoxFuture<'static, ()> + Send + Sync>;

#[allow(clippy::type_complexity)]
#[derive(Clone)]
struct CmdEntry {
    path: &'static str,
    handler: Arc<dyn Fn(Arc<WsApp>, &mut ws::WebsocketContext<WsSession>, serde_json::Value) -> LocalBoxFuture<'static, ()> + Send + Sync>,
    doc: Option<lib_asyncapi::WsDoc>,
}

#[derive(Clone)]
struct LoopCfg {
    interval: Duration,
    handler: MsgHandler,
    doc: Option<lib_asyncapi::WsDoc>,
}

#[derive(Clone)]
struct GlobalLoopCfg {
    interval: Duration,
    handler: GlobalHandler,
}

#[derive(Clone)]
pub struct WsApp {
    cmd: Vec<CmdEntry>,
    msg: HashMap<WsMessage, Vec<MsgHandler>>,
    loop_handlers: Vec<LoopCfg>,
    global_loops: Vec<GlobalLoopCfg>,
    ctx: TransportContext,
    heartbeat: HeartbeatConfig,
    #[allow(clippy::type_complexity)]
    error_handler: Option<Arc<dyn Fn(Arc<WsApp>, &mut ws::WebsocketContext<WsSession>, Error) + Send + Sync>>,
}

impl Default for WsApp {
    fn default() -> Self {
        Self::new()
    }
}

impl WsApp {
    pub fn new() -> Self {
        Self { cmd: Vec::new(), msg: HashMap::new(), loop_handlers: Vec::new(), global_loops: Vec::new(), ctx: TransportContext::new(), heartbeat: HeartbeatConfig::default(), error_handler: None }
    }

    /// Create a new [`WsApp`] by cloning an existing one.
    pub fn from_base(base: &Self) -> Self {
        base.clone()
    }

    pub fn configure_heartbeat(&mut self, cfg: HeartbeatConfig) -> &mut Self {
        self.heartbeat = cfg;
        self
    }

    pub fn heartbeat(&self) -> HeartbeatConfig {
        self.heartbeat
    }

    pub fn add_command<H>(&mut self, _handler: H) -> &mut Self
    where
        H: crate::types::CommandHandler + lib_asyncapi::DocumentedCommand + 'static,
    {
        self.ctx.schemas_mut().track::<H, _>(H::register_schemas);
        let doc = H::doc();
        self.cmd.push(CmdEntry { path: H::PATH, handler: Arc::new(|app, ctx, v| H::execute(app, ctx, v)), doc });
        self
    }

    pub fn add_data<T: Any + Send + Sync>(&mut self, data: Arc<T>) -> &mut Self {
        self.ctx.add_data(data);
        self
    }

    pub fn add_handler<H: crate::types::MessageHandler + 'static>(&mut self, msg: WsMessage) -> &mut Self {
        self.msg.entry(msg).or_default().push(Arc::new(|app, mctx, m| H::execute(app, mctx, m)));
        self
    }

    pub fn add_loop_handler<H>(&mut self, interval: Duration, _handler: H) -> &mut Self
    where
        H: crate::types::MessageHandler + lib_asyncapi::DocumentedLoop + 'static,
    {
        self.ctx.schemas_mut().track::<H, _>(H::register_schemas);
        self.loop_handlers.push(LoopCfg { interval, handler: Arc::new(|app, ctx, m| H::execute(app, ctx, m)), doc: H::doc() });
        self
    }

    pub fn add_global_loop<F, Fut>(&mut self, interval: Duration, f: F) -> &mut Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        self.global_loops.push(GlobalLoopCfg { interval, handler: Arc::new(move || f().boxed()) });
        self
    }

    pub fn set_error_handler<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(Arc<WsApp>, &mut ws::WebsocketContext<WsSession>, Error) + Send + Sync + 'static,
    {
        self.error_handler = Some(Arc::new(f));
        self
    }

    pub fn set_api_info(&mut self, info: lib_asyncapi::AsyncApiInfo) -> &mut Self {
        self.ctx.metadata_mut().set_info(info);
        self
    }

    pub fn set_api_id(&mut self, id: Url) -> &mut Self {
        self.ctx.metadata_mut().set_id(id);
        self
    }

    pub fn add_api_tag(&mut self, tag: lib_asyncapi::Tag) -> &mut Self {
        self.ctx.metadata_mut().add_tag(tag);
        self
    }

    pub fn set_external_docs(&mut self, docs: lib_asyncapi::ExternalDocs) -> &mut Self {
        self.ctx.metadata_mut().set_external_docs(docs);
        self
    }

    pub fn add_server<S: Into<String>>(&mut self, name: S, server: lib_asyncapi::Server) -> &mut Self {
        self.ctx.metadata_mut().add_server(name, server);
        self
    }

    pub fn handle_error(app: Arc<Self>, ctx: &mut ws::WebsocketContext<WsSession>, err: Error) {
        if let Some(h) = &app.error_handler {
            h(app.clone(), ctx, err);
        } else {
            error!("ws handler error: {err}");
        }
    }

    pub fn start(self, req: HttpRequest, stream: Payload) -> ActixResult<HttpResponse> {
        let heartbeat = self.heartbeat;
        ws::start(WsSession { app: Arc::new(self), heartbeat: HeartbeatGuard::new(heartbeat) }, &req, stream)
    }

    pub fn spawn_global_loops(self: Arc<Self>) -> Vec<tokio::task::JoinHandle<()>> {
        self.global_loops
            .iter()
            .map(|cfg| {
                let cfg = cfg.clone();
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(cfg.interval);
                    loop {
                        interval.tick().await;
                        (cfg.handler)().await;
                    }
                })
            })
            .collect()
    }

    pub fn get_data<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.ctx.get_data::<T>()
    }

    pub fn docs(&self) -> lib_asyncapi::WsDocList {
        self.cmd.iter().filter_map(|c| c.doc.clone()).chain(self.loop_handlers.iter().filter_map(|l| l.doc.clone())).collect()
    }

    pub fn asyncapi_doc(&self) -> lib_asyncapi::AsyncApiDoc {
        let docs: Vec<_> = self.cmd.iter().filter_map(|c| c.doc.clone()).collect();
        let loops: Vec<_> = self.loop_handlers.iter().filter_map(|l| l.doc.clone()).collect();
        self.ctx.asyncapi_doc(&docs, &loops)
    }

    pub fn asyncapi_json(&self) -> serde_json::Value {
        let docs: Vec<_> = self.cmd.iter().filter_map(|c| c.doc.clone()).collect();
        let loops: Vec<_> = self.loop_handlers.iter().filter_map(|l| l.doc.clone()).collect();
        self.ctx.asyncapi_json(&docs, &loops)
    }
}

pub struct WsSession {
    app: Arc<WsApp>,
    heartbeat: HeartbeatGuard,
}

impl Actor for WsSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let hb_cfg = self.app.heartbeat;
        ctx.run_interval(hb_cfg.interval(), move |act, ctx| {
            if let Err(err) = act.heartbeat.ensure_alive() {
                warn!(%err, "websocket heartbeat timeout, closing session");
                ctx.close(Some(ws::CloseReason::from(ws::CloseCode::Error)));
                ctx.stop();
                return;
            }
            ctx.ping(b"helios-heartbeat");
        });

        for cfg in &self.app.loop_handlers {
            let h = cfg.handler.clone();
            let interval = cfg.interval;
            let app = self.app.clone();
            let running = Rc::new(Cell::new(false));
            ctx.run_interval(interval, move |_, ctx| {
                if running.replace(true) {
                    return;
                }
                let h = h.clone();
                let app = app.clone();
                let running_inner = running.clone();
                let msg = Box::new(ws::Message::Ping(Bytes::new()));
                let msg_ref: &'static ws::Message = unsafe { &*(&*msg as *const _) };
                let fut = h(app, ctx, msg_ref);
                let fut = async move {
                    fut.await;
                    drop(msg);
                    running_inner.set(false);
                };
                ctx.spawn(actix::fut::wrap_future::<_, Self>(fut));
            });
        }
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsSession {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        if let Ok(m) = msg {
            match &m {
                ws::Message::Text(t) => match serde_json::from_str::<serde_json::Value>(t) {
                    Ok(val) => {
                        self.heartbeat.mark();
                        if let Some(cmd) = val.get("cmd").and_then(|c| c.as_str()) {
                            for entry in &self.app.cmd {
                                if lib_asyncapi::AsyncApiPath::<()>::extract(entry.path, cmd).is_some() {
                                    let fut = (entry.handler)(self.app.clone(), ctx, val.clone());
                                    ctx.spawn(actix::fut::wrap_future::<_, Self>(fut));
                                    return;
                                }
                            }
                        }
                        if let Some(list) = self.app.msg.get(&WsMessage::Text) {
                            for h in list {
                                let fut = h(self.app.clone(), ctx, &m);
                                ctx.spawn(actix::fut::wrap_future::<_, Self>(fut));
                            }
                        }
                    }
                    Err(e) => WsApp::handle_error(self.app.clone(), ctx, e.into()),
                },
                ws::Message::Binary(_) => {
                    self.heartbeat.mark();
                    if let Some(list) = self.app.msg.get(&WsMessage::Binary) {
                        for h in list {
                            let fut = h(self.app.clone(), ctx, &m);
                            ctx.spawn(actix::fut::wrap_future::<_, Self>(fut));
                        }
                    }
                }
                ws::Message::Ping(bytes) => {
                    self.heartbeat.mark();
                    ctx.pong(bytes);
                    if let Some(list) = self.app.msg.get(&WsMessage::Ping) {
                        for h in list {
                            let fut = h(self.app.clone(), ctx, &m);
                            ctx.spawn(actix::fut::wrap_future::<_, Self>(fut));
                        }
                    }
                }
                ws::Message::Pong(_) => {
                    self.heartbeat.mark();
                    if let Some(list) = self.app.msg.get(&WsMessage::Pong) {
                        for h in list {
                            let fut = h(self.app.clone(), ctx, &m);
                            ctx.spawn(actix::fut::wrap_future::<_, Self>(fut));
                        }
                    }
                }
                ws::Message::Close(_) => {
                    if let Some(list) = self.app.msg.get(&WsMessage::Close) {
                        for h in list {
                            let fut = h(self.app.clone(), ctx, &m);
                            ctx.spawn(actix::fut::wrap_future::<_, Self>(fut));
                        }
                    }
                    ctx.close(None);
                    ctx.stop();
                }
                ws::Message::Continuation(_) => {}
                ws::Message::Nop => {}
            }
        }
    }
}
