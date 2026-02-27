use actix_test::start;
use actix_web::{web, App, HttpRequest};
use actix_web_actors::ws;
use futures_util::{SinkExt, StreamExt};
use lib_asyncapi_macro::asyncapi;
use lib_ws::{WsApp, WsContext};
use lib_ws_macro::ws;
use serde::Deserialize;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

#[derive(Default)]
struct State {
    count: AtomicUsize,
    flag: AtomicBool,
    text: Mutex<Option<String>>,
}

// ----- Loops -----

#[asyncapi(summary = "count loop", description = "periodically send count")]
#[ws(loop = "count_loop")]
async fn count_loop(data: lib_ws::AsyncApiData<State>, ctx: &mut WsContext, _msg: &ws::Message) {
    let val = data.0.count.load(Ordering::SeqCst);
    ctx.push(serde_json::json!({ "val": val }));
}

#[asyncapi(summary = "flag loop", description = "send flag state")]
#[ws(loop = "flag_loop")]
async fn flag_loop(data: lib_ws::AsyncApiData<State>, ctx: &mut WsContext, _msg: &ws::Message) {
    let flag = data.0.flag.load(Ordering::SeqCst);
    ctx.push(serde_json::json!({ "flag": flag }));
}

#[asyncapi(summary = "text loop", description = "broadcast text")]
#[ws(loop = "text_loop")]
async fn text_loop(data: lib_ws::AsyncApiData<State>, ctx: &mut WsContext, _msg: &ws::Message) {
    let text = data.0.text.lock().unwrap().clone();
    ctx.push(serde_json::json!({ "text": text }));
}

// ----- Commands without responses -----

#[derive(Deserialize, lib_asyncapi_macro::AsyncApiSchema)]
struct SetText {
    msg: String,
}

#[asyncapi(summary = "set text", description = "update shared text")]
#[ws("text.set")]
async fn set_text(payload: lib_ws::AsyncApiPayload<SetText>, data: lib_ws::AsyncApiData<State>) {
    *data.0.text.lock().unwrap() = Some(payload.0.msg);
}

#[asyncapi(summary = "enable flag", description = "turn on flag")]
#[ws("flag.enable")]
async fn enable_flag(data: lib_ws::AsyncApiData<State>) {
    data.0.flag.store(true, Ordering::SeqCst);
}

#[asyncapi(summary = "reset", description = "clear counter")]
#[ws("reset")]
async fn reset_count(data: lib_ws::AsyncApiData<State>) {
    data.0.count.store(0, Ordering::SeqCst);
}

// ----- Commands with responses -----

#[derive(Deserialize, lib_asyncapi_macro::AsyncApiSchema)]
struct Inc {
    val: u32,
}

#[asyncapi(summary = "increment", description = "increase counter", response = u32)]
#[ws("inc")]
async fn increment(payload: lib_ws::AsyncApiPayload<Inc>, data: lib_ws::AsyncApiData<State>) -> u32 {
    let val = data.0.count.fetch_add(payload.0.val as usize, Ordering::SeqCst) + payload.0.val as usize;
    val as u32
}

#[asyncapi(summary = "get count", description = "current counter", response = u32)]
#[ws("count")]
async fn get_count(data: lib_ws::AsyncApiData<State>) -> u32 {
    data.0.count.load(Ordering::SeqCst) as u32
}

#[asyncapi(summary = "ping", description = "pong response", response = String)]
#[ws("ping")]
async fn ping() -> String {
    "pong".to_string()
}

// ----- Interconnected commands -----

#[asyncapi(summary = "chain first", description = "increments by one", response = u32)]
#[ws("chain.first")]
async fn chain_first(data: lib_ws::AsyncApiData<State>) -> u32 {
    (data.0.count.fetch_add(1, Ordering::SeqCst) + 1) as u32
}

#[asyncapi(summary = "chain second", description = "increments by two", response = u32)]
#[ws("chain.second")]
async fn chain_second(data: lib_ws::AsyncApiData<State>) -> u32 {
    (data.0.count.fetch_add(2, Ordering::SeqCst) + 2) as u32
}

#[asyncapi(summary = "chain third", description = "increments by three", response = u32)]
#[ws("chain.third")]
async fn chain_third(data: lib_ws::AsyncApiData<State>) -> u32 {
    (data.0.count.fetch_add(3, Ordering::SeqCst) + 3) as u32
}

#[actix_rt::main]
async fn main() {
    let mut srv = start(|| {
        App::new().service(web::resource("/").to(|req: HttpRequest, stream: web::Payload| async {
            let mut app = WsApp::new();
            app.add_data(Arc::new(State::default()));
            app.add_loop_handler(Duration::from_millis(10), count_loop);
            app.add_loop_handler(Duration::from_millis(15), flag_loop);
            app.add_loop_handler(Duration::from_millis(20), text_loop);
            app.add_command(set_text);
            app.add_command(enable_flag);
            app.add_command(reset_count);
            app.add_command(increment);
            app.add_command(get_count);
            app.add_command(ping);
            app.add_command(chain_first);
            app.add_command(chain_second);
            app.add_command(chain_third);
            app.start(req, stream)
        }))
    });

    let mut framed = srv.ws().await.unwrap();
    for _ in 0..3 {
        if let Some(Ok(ws::Frame::Text(txt))) = framed.next().await {
            let val: serde_json::Value = serde_json::from_slice(&txt).unwrap();
            println!("loop msg at {}", val["sent-at"].as_str().unwrap());
        }
    }
    let _ = framed.send(ws::Message::Close(Some(ws::CloseCode::Normal.into()))).await;
}
