use lib_asyncapi_macro::{AsyncApiSchema, asyncapi};
use lib_ws_macro::ws;
extern crate actix_web_actors as actix_ws;
extern crate futures;
extern crate lib_ws;
extern crate serde_json;
use lib_asyncapi::{AsyncApiPath, AsyncApiPayload, DocumentedCommand, DocumentedLoop};
use serde::Deserialize;

#[ws]
async fn simple() {}

#[ws("test.cmd")]
async fn with_cmd() {}

#[derive(Deserialize, AsyncApiSchema)]
struct TestPayload {
    #[asyncapi(example = 7)]
    val: u32,
}

#[ws]
async fn with_payload(_p: AsyncApiPayload<TestPayload>) {}

#[ws("cmd.payload")]
async fn cmd_payload(_p: AsyncApiPayload<TestPayload>) {}

#[asyncapi(summary = "obj", description = "object payload", tags = ["o"])]
#[ws("cmd.obj")]
async fn cmd_obj(_p: AsyncApiPayload<TestPayload>) {}

#[asyncapi(summary = "tagged", description = "with tags", tags = ["a", "b"])]
#[ws("tag.test")]
async fn tagged(_p: AsyncApiPayload<u32>) {}

#[asyncapi(summary = "param", description = "with params", tags = ["p"])]
#[ws("cmd.{id}.{name}")]
async fn param_cmd(_path: AsyncApiPath<(u32, String)>) {}

#[asyncapi(summary = "single", description = "single param", tags = ["s"]) ]
#[ws("cmd.{id}")]
async fn single_param(_path: AsyncApiPath<u32>) {}

#[asyncapi(summary = "resp", description = "has response", tags = ["r"], response = u32)]
#[ws("cmd.response")]
async fn cmd_resp() {}

#[ws(loop = "loop_simple")]
async fn loop_simple(_ctx: &mut actix_ws::ws::WebsocketContext<lib_ws::app::WsSession>, _msg: &actix_ws::ws::Message) {}

#[ws(loop = "async_loop")]
async fn async_loop() {}

#[lib_test::tokio_test]
async fn ws_macro_generates_structs() {
    simple::call().await;
    with_cmd::call().await;
    let payload = TestPayload { val: 1 };
    let _ = payload.val;
    with_payload::call(AsyncApiPayload(payload)).await;
    let payload = TestPayload { val: 2 };
    let _ = payload.val;
    cmd_payload::call(AsyncApiPayload(payload)).await;
    assert_eq!(simple::PATH, "");
    assert_eq!(with_cmd::PATH, "test.cmd");
    assert_eq!(with_payload::PATH, "");
    assert_eq!(cmd_payload::PATH, "cmd.payload");
    assert_eq!(loop_simple::PATH, "loop_simple");
    async_loop::call().await;
    assert_eq!(async_loop::PATH, "async_loop");
}

#[lib_test::tokio_test]
async fn asyncapi_macro_collects_docs() {
    let doc = tagged::doc().expect("missing doc");
    assert_eq!(doc.summary, "tagged");
    assert_eq!(doc.description, "with tags");
    assert_eq!(doc.tags, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(doc.payload.as_ref().map(|s| s.name), Some("U32"));
    assert!(doc.params.is_empty());
    assert_eq!(tagged::PATH, "tag.test");
}

#[lib_test::tokio_test]
async fn asyncapi_macro_collects_params() {
    let doc = param_cmd::doc().expect("missing doc");
    assert_eq!(doc.summary, "param");
    assert_eq!(doc.description, "with params");
    assert_eq!(doc.tags, vec!["p".to_string()]);
    assert!(doc.payload.is_none());
    assert_eq!(doc.params.len(), 2);
    assert_eq!(doc.params[0].0, "id");
    assert_eq!(doc.params[0].1.name, "U32");
    assert_eq!(doc.params[1].0, "name");
    assert_eq!(doc.params[1].1.name, "String");
    assert_eq!(param_cmd::PATH, "cmd.{id}.{name}");
}

#[lib_test::tokio_test]
async fn asyncapi_macro_collects_response() {
    let doc = cmd_resp::doc().expect("missing doc");
    assert_eq!(doc.summary, "resp");
    assert_eq!(doc.description, "has response");
    assert_eq!(doc.tags, vec!["r".to_string()]);
    assert!(doc.payload.is_none());
    assert_eq!(doc.responses.len(), 1);
    assert_eq!(doc.responses[0].name, "U32");
    assert!(doc.params.is_empty());
    assert_eq!(cmd_resp::PATH, "cmd.response");
}

#[asyncapi(summary = "multi", description = "multi response", tags = ["m"], response = [u32, bool])]
#[ws("cmd.multi")]
async fn cmd_multi() {}

#[lib_test::tokio_test]
async fn asyncapi_macro_collects_multiple_responses() {
    let doc = cmd_multi::doc().expect("missing doc");
    assert_eq!(doc.summary, "multi");
    assert_eq!(doc.description, "multi response");
    assert_eq!(doc.tags, vec!["m".to_string()]);
    assert!(doc.payload.is_none());
    assert_eq!(doc.responses.len(), 2);
    assert_eq!(doc.responses[0].name, "U32");
    assert_eq!(doc.responses[1].name, "Boolean");
    assert!(doc.params.is_empty());
    assert_eq!(cmd_multi::PATH, "cmd.multi");
}

#[lib_test::tokio_test]
async fn asyncapi_macro_derives_object_payload() {
    let doc = cmd_obj::doc().expect("missing doc");
    assert_eq!(doc.summary, "obj");
    assert!(doc.payload.is_some());
    assert_eq!(doc.payload.as_ref().unwrap().name, stringify!(TestPayload));
}

#[lib_test::tokio_test]
async fn asyncapi_macro_single_param() {
    let doc = single_param::doc().expect("missing doc");
    assert_eq!(doc.summary, "single");
    assert_eq!(doc.params.len(), 1);
    assert_eq!(doc.params[0].0, "id");
    assert_eq!(doc.params[0].1.name, "U32");
    assert_eq!(single_param::PATH, "cmd.{id}");
}

#[asyncapi(summary = "loopdoc", description = "loop handler", tags = ["loop"], response = bool)]
#[ws(loop = "documented_loop")]
async fn documented_loop(_ctx: &mut actix_ws::ws::WebsocketContext<lib_ws::app::WsSession>, _msg: &actix_ws::ws::Message) {}

#[lib_test::tokio_test]
async fn asyncapi_loop_collects_doc() {
    let doc = documented_loop::doc().expect("missing doc");
    assert_eq!(doc.summary, "loopdoc");
    assert_eq!(doc.description, "loop handler");
    assert_eq!(doc.tags, vec!["loop".to_string()]);
    assert!(doc.payload.is_none());
    assert_eq!(doc.responses.len(), 1);
    assert_eq!(doc.responses[0].name, "Boolean");
    assert!(doc.params.is_empty());
    assert_eq!(documented_loop::PATH, "documented_loop");
}
