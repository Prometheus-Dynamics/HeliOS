use lib_asyncapi_macro::asyncapi;
use lib_nt4::types::AsyncApiPath;
use lib_nt4::Nt4App;
use lib_nt4_macro::nt4;
use std::sync::Arc;

#[asyncapi(summary = "", description = "")]
#[nt4(pub, "macro.pub")]
fn pub_handler(_app: Arc<Nt4App>) -> i32 {
    0
}

#[asyncapi(summary = "", description = "")]
#[nt4(sub, "macro.sub")]
fn sub_handler(_app: Arc<Nt4App>, _path: String, _val: serde_json::Value) {}

#[asyncapi(summary = "", description = "")]
#[nt4(pubsub, "macro.pubsub")]
fn pubsub_handler(_app: Arc<Nt4App>, _path: String, _val: serde_json::Value) -> i32 {
    0
}

#[asyncapi(summary = "", description = "")]
#[nt4(sub, "demo.add.{val}")]
fn path_handler(_app: Arc<Nt4App>, _path: AsyncApiPath<(String,)>, _val: serde_json::Value) {}

#[test]
fn path_constants() {
    assert_eq!(pub_handler::PATH, "macro.pub");
    assert_eq!(pub_handler::PATTERN, "macro.pub");
    assert_eq!(sub_handler::PATH, "macro.sub");
    assert_eq!(sub_handler::PATTERN, "macro.sub");
    assert_eq!(pubsub_handler::PATH, "macro.pubsub");
    assert_eq!(pubsub_handler::PATTERN, "macro.pubsub");
    assert_eq!(path_handler::PATH, "demo.add");
    assert_eq!(path_handler::PATTERN, "demo.add.{val}");
}

#[test]
fn register_handlers() {
    let mut app = Nt4App::new();
    app.add_pub(pub_handler);
    app.add_sub(sub_handler);
    app.add_pubsub(pubsub_handler);
    app.add_sub(path_handler);
}
