use std::sync::Arc;

pub use lib_asyncapi::{AsyncApiData, AsyncApiPath, AsyncApiPayload, FromSegments};

use futures::future::BoxFuture;
use nt_client::data::NetworkTableData;
use rmpv::Value as MsgValue;

pub type Value = MsgValue;

pub type DataMap = lib_asyncapi::AsyncApiDataMap;

pub trait PubHandler {
    type Output: NetworkTableData + Send + 'static;
    /// Prefix used for registering the topic with NT4
    const PATH: &'static str;
    /// Full pattern including path variables used for extraction
    const PATTERN: &'static str;
    fn execute(app: Arc<crate::Nt4App>) -> BoxFuture<'static, Self::Output>;
}

pub trait SubHandler {
    const PATH: &'static str;
    const PATTERN: &'static str;
    fn execute(app: Arc<crate::Nt4App>, path: String, val: serde_json::Value) -> BoxFuture<'static, ()>;
}

pub trait PubSubHandler {
    type Output: NetworkTableData + Default + Send + 'static;
    const PATH: &'static str;
    const PATTERN: &'static str;
    fn execute(app: Arc<crate::Nt4App>, path: String, val: serde_json::Value) -> BoxFuture<'static, Self::Output>;
}

pub type SubHandlerFn = Arc<dyn Fn(Arc<crate::Nt4App>, String, serde_json::Value) -> BoxFuture<'static, ()> + Send + Sync>;
