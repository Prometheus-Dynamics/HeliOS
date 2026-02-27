use lib_asyncapi::{AsyncApiData, AsyncApiInfo, AsyncApiPath, AsyncApiPayload, Tag};
use lib_asyncapi_macro::{asyncapi, AsyncApiSchema};
use lib_ws::WsApp;
use lib_ws_macro::ws;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use url::Url;

#[derive(Default)]
struct Counter(AtomicUsize);

#[derive(Serialize, Deserialize, Clone, AsyncApiSchema)]
struct Inner {
    #[asyncapi(example = 1)]
    val: u32,
}

#[derive(Serialize, Deserialize, Clone, AsyncApiSchema)]
struct Outer {
    inner: Inner,
    #[asyncapi(example = "demo")]
    name: String,
}

#[derive(Serialize, Deserialize, Clone, AsyncApiSchema)]
struct Stats {
    #[asyncapi(example = 0)]
    count: u32,
}

#[asyncapi(summary = "Nested", description = "Nested payload and response", tags = ["complex"], response = [Stats, bool])]
#[ws("nested.{id}")]
async fn nested_cmd(path: AsyncApiPath<(u32,)>, payload: AsyncApiPayload<Outer>, data: AsyncApiData<Counter>) {
    for _ in 0..path.0 .0 {
        data.0 .0.fetch_add(payload.0.inner.val as usize, Ordering::SeqCst);
    }
}

#[asyncapi(summary = "Loop", description = "Loop handler example", tags = ["loop"], response = bool)]
#[ws("ping")]
async fn ping() {}

#[ws(loop = "ping_loop")]
async fn ping_loop(ctx: &mut lib_ws::WsContext, _msg: &actix_web_actors::ws::Message) {
    ctx.push("ping");
}

fn main() {
    let mut app = WsApp::new();
    app.set_error_handler(|_, _, err| println!("error: {err}"));
    app.add_data(Arc::new(Counter::default()));
    app.add_command(nested_cmd);
    app.add_command(ping);
    app.add_loop_handler(Duration::from_millis(100), ping_loop);
    app.set_api_id(Url::parse("urn:helios:advanced-api").unwrap());
    app.set_api_info(AsyncApiInfo {
        title: "Advanced API".into(),
        version: "0.1.0".into(),
        summary: None,
        description: Some("Advanced example".into()),
        terms_of_service: None,
        contact: None,
        license: None,
        tags: Vec::new(),
        external_docs: None,
    });
    app.add_api_tag(Tag { name: "complex".into(), description: Some("Complex commands".into()), external_docs: None });
    app.add_api_tag(Tag { name: "loop".into(), description: Some("Looping".into()), external_docs: None });
    let doc = app.asyncapi_json();
    println!("{}", serde_json::to_string_pretty(&doc).unwrap());
}
