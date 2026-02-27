#[cfg(not(feature = "macros"))]
fn main() {
    panic!("Enable the `lib-ws` `macros` feature to run this example");
}

#[cfg(feature = "macros")]
mod example {
    use actix_web::{http::header, web, App, HttpRequest, HttpResponse, HttpServer};
    use lib_asyncapi_macro::asyncapi;
    use lib_transport::HeartbeatConfig;
    use lib_ws::WsApp;
    use lib_ws_macro::ws;
    use std::sync::Arc;

    #[derive(Clone)]
    pub struct SessionState {
        pub secret: String,
    }

    #[asyncapi(summary = "Echo payload", description = "Simple echo command for authenticated clients", tags = ["auth"])]
    #[ws("demo.echo")]
    pub async fn echo(payload: lib_asyncapi::AsyncApiPayload<String>) -> String {
        payload.into_inner()
    }

    pub fn authorised(req: &HttpRequest, state: &SessionState) -> bool {
        req.headers().get(header::AUTHORIZATION).and_then(|val| val.to_str().ok()).map(|token| token == state.secret).unwrap_or(false)
    }

    pub async fn ws_entry(req: HttpRequest, stream: web::Payload, app: web::Data<WsApp>) -> actix_web::Result<HttpResponse> {
        let state = app.get_data::<SessionState>().expect("state");
        if !authorised(&req, &state) {
            return Ok(HttpResponse::Unauthorized().finish());
        }
        app.get_ref().clone().start(req, stream)
    }

    pub async fn run() -> std::io::Result<()> {
        let mut app = WsApp::new();
        app.configure_heartbeat(HeartbeatConfig::new(std::time::Duration::from_secs(5), std::time::Duration::from_secs(15)));
        app.add_data(Arc::new(SessionState { secret: "Bearer demo-token".into() }));
        app.add_command(echo);

        HttpServer::new(move || App::new().app_data(web::Data::new(app.clone())).route("/ws", web::get().to(ws_entry))).bind(("127.0.0.1", 9000))?.run().await
    }
}

#[cfg(feature = "macros")]
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    example::run().await
}
