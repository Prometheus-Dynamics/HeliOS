use actix_test::start;
use actix_web::{web, App, HttpRequest};
use actix_web_actors::ws;
use futures_util::{SinkExt, StreamExt};
use lib_asyncapi::{AsyncApiData, AsyncApiPath, AsyncApiPayload};
use lib_asyncapi_macro::asyncapi;
use lib_ws::WsApp;
use lib_ws_macro::ws;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::time;

#[derive(Default)]
struct Counter(AtomicUsize);

#[ws("inc.{val}")]
async fn inc(path: AsyncApiPath<(u32,)>, data: AsyncApiData<Counter>) {
    data.0 .0.fetch_add(path.0 .0 as usize, Ordering::SeqCst);
}

impl lib_asyncapi::DocumentedCommand for inc {}

#[derive(serde::Deserialize)]
struct AddPayload {
    val: u32,
}

#[ws("add")]
async fn add(payload: AsyncApiPayload<AddPayload>, data: AsyncApiData<Counter>) {
    data.0 .0.fetch_add(payload.0.val as usize, Ordering::SeqCst);
}

impl lib_asyncapi::DocumentedCommand for add {}

#[actix_rt::test]
async fn command_injects_data_and_path() {
    let counter = Arc::new(Counter::default());
    let counter_clone = counter.clone();

    let mut srv = start(move || {
        let data = counter_clone.clone();
        App::new().service(web::resource("/").to(move |req: HttpRequest, stream: web::Payload| {
            let data = data.clone();
            async move {
                let mut app = WsApp::new();
                app.add_data(data);
                app.add_command(inc);
                app.start(req, stream)
            }
        }))
    });

    let mut framed = srv.ws().await.unwrap();
    framed.send(ws::Message::Text("{\"cmd\":\"inc.5\"}".into())).await.unwrap();
    framed.send(ws::Message::Close(Some(ws::CloseCode::Normal.into()))).await.unwrap();
    let _ = framed.next().await.unwrap();

    assert_eq!(counter.0.load(Ordering::SeqCst), 5);
}

#[actix_rt::test]
async fn command_injects_payload() {
    let counter = Arc::new(Counter::default());
    let counter_clone = counter.clone();

    let mut srv = start(move || {
        let data = counter_clone.clone();
        App::new().service(web::resource("/").to(move |req: HttpRequest, stream: web::Payload| {
            let data = data.clone();
            async move {
                let mut app = WsApp::new();
                app.add_data(data);
                app.add_command(add);
                app.start(req, stream)
            }
        }))
    });

    let mut framed = srv.ws().await.unwrap();
    framed.send(ws::Message::Text("{\"cmd\":\"add\",\"payload\":{\"val\":3}}".into())).await.unwrap();
    framed.send(ws::Message::Close(Some(ws::CloseCode::Normal.into()))).await.unwrap();
    let _ = framed.next().await.unwrap();

    assert_eq!(counter.0.load(Ordering::SeqCst), 3);
}

#[actix_rt::test]
async fn error_handler_called_on_invalid_payload() {
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();

    let mut srv = start(move || {
        let err_counter = counter_clone.clone();
        App::new().service(web::resource("/").to(move |req: HttpRequest, stream: web::Payload| {
            let err_counter = err_counter.clone();
            async move {
                let mut app = WsApp::new();
                app.set_error_handler(move |_, _ctx, _err| {
                    err_counter.fetch_add(1, Ordering::SeqCst);
                });
                app.add_command(add);
                app.start(req, stream)
            }
        }))
    });

    let mut framed = srv.ws().await.unwrap();
    framed.send(ws::Message::Text("{\"cmd\":\"add\",\"payload\":{}}".into())).await.unwrap();
    framed.send(ws::Message::Close(Some(ws::CloseCode::Normal.into()))).await.unwrap();
    let _ = framed.next().await.unwrap();

    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn ws_path_extracts() {
    let path = lib_asyncapi::AsyncApiPath::<(u32, String)>::extract("cmd.{id}.{name}", "cmd.3.bob").unwrap();
    assert_eq!(path.0 .0, 3);
    assert_eq!(path.0 .1, "bob");
}

#[asyncapi(summary = "prim", description = "primitive", tags = ["p"])]
#[ws("prim")]
async fn prim_cmd(_p: AsyncApiPayload<u32>) {}

#[asyncapi(summary = "prim64", description = "primitive64", tags = ["p"])]
#[ws("prim64")]
async fn prim64_cmd(_p: AsyncApiPayload<u64>) {}

#[asyncapi(summary = "primsz", description = "primitive usize", tags = ["p"])]
#[ws("primsz")]
async fn prim_usize_cmd(_p: AsyncApiPayload<usize>) {}

#[test]
fn asyncapi_inlines_primitive_payload() {
    let mut app = WsApp::new();
    app.add_command(prim_cmd);
    let doc = app.asyncapi_json();
    println!("{}", serde_json::to_string_pretty(&doc).unwrap());
    assert!(doc["components"]["schemas"].get("U32").is_none());
    let payload = &doc["channels"]["prim"]["messages"]["prim"]["payload"];
    assert!(payload.get("$ref").is_none());
    assert_eq!(payload["type"], "object");
    assert_eq!(payload["properties"]["payload"]["type"], "integer");
    assert_eq!(payload["properties"]["sent-at"]["type"], "string");
}

#[test]
fn asyncapi_inlines_u64_payload() {
    let mut app = WsApp::new();
    app.add_command(prim64_cmd);
    let doc = app.asyncapi_json();
    assert!(doc["components"]["schemas"].get("U64").is_none());
    let payload = &doc["channels"]["prim64"]["messages"]["prim64"]["payload"];
    assert!(payload.get("$ref").is_none());
    assert_eq!(payload["type"], "object");
    assert_eq!(payload["properties"]["payload"]["type"], "integer");
    assert_eq!(payload["properties"]["sent-at"]["type"], "string");
}

#[test]
fn asyncapi_inlines_usize_payload() {
    let mut app = WsApp::new();
    app.add_command(prim_usize_cmd);
    let doc = app.asyncapi_json();
    assert!(doc["components"]["schemas"].get("Usize").is_none());
    let payload = &doc["channels"]["primsz"]["messages"]["primsz"]["payload"];
    assert!(payload.get("$ref").is_none());
    assert_eq!(payload["type"], "object");
    assert_eq!(payload["properties"]["payload"]["type"], "integer");
    assert_eq!(payload["properties"]["sent-at"]["type"], "string");
}

#[asyncapi(summary = "loopmsg", description = "loop docs", tags = ["loop"], response = u32)]
#[ws(loop = "docs_loop")]
async fn docs_loop(_ctx: &mut ws::WebsocketContext<lib_ws::app::WsSession>, _msg: &ws::Message) {}

#[test]
fn asyncapi_includes_loop_docs() {
    let mut app = WsApp::new();
    app.add_loop_handler(Duration::from_millis(10), docs_loop);
    let doc = app.asyncapi_json();
    assert!(doc["channels"].get("docs_loop").is_some());
}

#[ws(loop = "data_loop")]
async fn data_loop(data: AsyncApiData<Counter>, _ctx: &mut ws::WebsocketContext<lib_ws::app::WsSession>, _msg: &ws::Message) {
    data.0 .0.fetch_add(1, Ordering::SeqCst);
}

#[actix_rt::test]
async fn loop_injects_data() {
    let counter = Arc::new(Counter::default());
    let counter_clone = counter.clone();

    let mut srv = start(move || {
        let data = counter_clone.clone();
        App::new().service(web::resource("/").to(move |req: HttpRequest, stream: web::Payload| {
            let data = data.clone();
            async move {
                let mut app = WsApp::new();
                app.add_data(data);
                app.add_loop_handler(Duration::from_millis(10), data_loop);
                app.start(req, stream)
            }
        }))
    });

    let mut framed = srv.ws().await.unwrap();
    time::sleep(Duration::from_millis(30)).await;
    framed.send(ws::Message::Close(Some(ws::CloseCode::Normal.into()))).await.unwrap();
    let _ = framed.next().await.unwrap();

    assert!(counter.0.load(Ordering::SeqCst) > 0);
}

#[ws(loop = "echo")]
async fn echo_loop(ctx: &mut lib_ws::WsContext, _msg: &ws::Message) {
    ctx.push(serde_json::json!({"val": 1}));
}

#[actix_rt::test]
async fn server_client_roundtrip() {
    let mut srv = start(|| {
        App::new().service(web::resource("/").to(|req: HttpRequest, stream: web::Payload| async {
            let mut app = WsApp::new();
            app.add_loop_handler(Duration::from_millis(5), echo_loop);
            app.start(req, stream)
        }))
    });

    let mut framed = srv.ws().await.unwrap();
    if let Some(Ok(ws::Frame::Text(txt))) = framed.next().await {
        let val: serde_json::Value = serde_json::from_slice(&txt).unwrap();
        assert_eq!(val["cmd"], "echo");
        assert_eq!(val["payload"]["val"], 1);
        assert!(val.get("sent-at").and_then(|v| v.as_str()).is_some());
    } else {
        panic!("expected text frame");
    }
    framed.send(ws::Message::Close(Some(ws::CloseCode::Normal.into()))).await.unwrap();
    let _ = framed.next().await.unwrap();
}
