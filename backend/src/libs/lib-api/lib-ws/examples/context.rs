use actix_test::start;
use actix_web::{web, App, HttpRequest};
use actix_web_actors::ws;
use futures_util::{SinkExt, StreamExt};
use lib_ws::types::CommandHandler;
use lib_ws::{WsApp, WsContext};
use lib_ws_macro::ws;
use std::time::Duration;

#[ws(loop = "echo")]
async fn echo_loop(ctx: &mut WsContext, _msg: &ws::Message) {
    ctx.push(serde_json::json!({"val": 1}));
}

struct EchoCmd;

impl CommandHandler for EchoCmd {
    const PATH: &'static str = "echo";

    fn execute(_app: std::sync::Arc<WsApp>, ctx: &mut actix_web_actors::ws::WebsocketContext<lib_ws::app::WsSession>, val: serde_json::Value) -> futures::future::LocalBoxFuture<'static, ()> {
        let mut wctx = WsContext::new(Self::PATH, ctx);
        if let Some(v) = val.get("payload") {
            wctx.push(v);
        }
        Box::pin(async {})
    }
}

impl lib_asyncapi::DocumentedCommand for EchoCmd {}

#[actix_rt::main]
async fn main() {
    let mut srv = start(|| {
        App::new().service(web::resource("/").to(|req: HttpRequest, stream: web::Payload| async {
            let mut app = WsApp::new();
            app.add_loop_handler(Duration::from_millis(5), echo_loop);
            app.add_command(EchoCmd);
            app.start(req, stream)
        }))
    });

    let mut framed = srv.ws().await.unwrap();
    if let Some(Ok(ws::Frame::Text(txt))) = framed.next().await {
        let val: serde_json::Value = serde_json::from_slice(&txt).unwrap();
        println!("first sent at {}", val["sent-at"].as_str().unwrap());
    }
    framed.send(ws::Message::Text("{\"cmd\":\"echo\",\"payload\":true}".into())).await.unwrap();
    if let Some(Ok(ws::Frame::Text(txt))) = framed.next().await {
        let val: serde_json::Value = serde_json::from_slice(&txt).unwrap();
        println!("echo sent at {}", val["sent-at"].as_str().unwrap());
    }
    let _ = framed.send(ws::Message::Close(Some(ws::CloseCode::Normal.into()))).await;
}
