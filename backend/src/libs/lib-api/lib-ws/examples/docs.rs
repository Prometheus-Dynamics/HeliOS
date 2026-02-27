use lib_asyncapi::{AsyncApiData, AsyncApiInfo, AsyncApiPath, AsyncApiPayload, ExternalDocs, Server, Tag};
use lib_asyncapi_macro::{asyncapi, AsyncApiSchema};
use lib_ws::WsApp;
use lib_ws_macro::ws;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use url::Url;

#[derive(Default)]
struct Counter(AtomicUsize);

#[derive(Serialize, Deserialize, Clone, AsyncApiSchema)]
struct Change {
    #[asyncapi(example = 5)]
    val: u32,
}

use lib_ws::types::FromSegments;

impl FromSegments for Change {
    fn from_segments(segs: &[&str]) -> Option<Self> {
        if segs.len() != 1 {
            return None;
        }
        Some(Self { val: segs[0].parse().ok()? })
    }
}

impl std::str::FromStr for Change {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self { val: s.parse()? })
    }
}

#[derive(Serialize, Deserialize, Clone, AsyncApiSchema)]
struct Value {
    #[asyncapi(example = 42)]
    value: u32,
}

#[asyncapi(summary = "Increment", description = "Add value from path", tags = ["counter"], response = u32)]
#[ws("inc.{val}")]
async fn inc(path: AsyncApiPath<Change>, data: AsyncApiData<Counter>) {
    data.0 .0.fetch_add(path.0.val as usize, Ordering::SeqCst);
}

#[asyncapi(summary = "Add", description = "Add payload value", tags = ["counter"], response = u32)]
#[ws("add")]
async fn add(payload: AsyncApiPayload<Change>, data: AsyncApiData<Counter>) {
    data.0 .0.fetch_add(payload.0.val as usize, Ordering::SeqCst);
}

#[asyncapi(summary = "Get", description = "Read current value", tags = ["counter"], response = Value)]
#[ws("get")]
async fn get(data: AsyncApiData<Counter>) {
    let _val = data.0 .0.load(Ordering::SeqCst);
}

#[asyncapi(summary = "Reset", description = "Reset value to zero", tags = ["counter"], response = [Value, bool])]
#[ws("reset")]
async fn reset(data: AsyncApiData<Counter>) {
    let _old = data.0 .0.swap(0, Ordering::SeqCst);
}

fn main() {
    let mut app = WsApp::new();
    app.add_data(Arc::new(Counter::default()));
    app.add_command(inc);
    app.add_command(add);
    app.add_command(get);
    app.add_command(reset);
    app.set_api_id(Url::parse("urn:helios:counter-api").unwrap());
    app.add_server("local", Server { host: "localhost:8080".into(), protocol: "ws".into(), protocol_version: None, description: Some("Local WebSocket".into()) });
    app.set_api_info(AsyncApiInfo {
        title: "Counter API".into(),
        version: "0.1.0".into(),
        summary: None,
        description: Some("Example WebSocket API".into()),
        terms_of_service: None,
        contact: None,
        license: None,
        tags: Vec::new(),
        external_docs: None,
    });
    app.add_api_tag(Tag {
        name: "counter".into(),
        description: Some("Counter commands".into()),
        external_docs: Some(ExternalDocs { description: Some("Tag docs".into()), url: "https://example.com/tag".into() }),
    });
    app.set_external_docs(ExternalDocs { description: Some("Project repo".into()), url: "https://example.com".into() });
    let doc = app.asyncapi_json();
    println!("{}", serde_json::to_string_pretty(&doc).unwrap());
}
