use std::sync::Arc;

pub use lib_asyncapi::{AsyncApiData, AsyncApiPath, AsyncApiPayload, FromSegments};

use futures::future::LocalBoxFuture;
use serde_json;

pub use lib_asyncapi::AsyncApiDataMap as DataMap;

pub trait MessageHandler {
    fn execute(app: Arc<crate::app::WsApp>, ctx: &mut actix_web_actors::ws::WebsocketContext<crate::app::WsSession>, msg: &actix_web_actors::ws::Message) -> LocalBoxFuture<'static, ()>;
}

pub trait CommandHandler {
    const PATH: &'static str;
    fn execute(app: Arc<crate::app::WsApp>, ctx: &mut actix_web_actors::ws::WebsocketContext<crate::app::WsSession>, val: serde_json::Value) -> LocalBoxFuture<'static, ()>;
}
