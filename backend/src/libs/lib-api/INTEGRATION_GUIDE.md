# Helios API Stack Integration Guide

This guide explains how to host the `lib-api` crates outside of the Helios main
binary. It highlights the shared transport utilities, heartbeat configuration,
and provides baseline authentication patterns for JSON-RPC, WebSocket, and NT4
services.

## Shared Building Blocks

All API transports share the `lib-transport` crate. It provides:

- `TransportContext` – stores shared data (`AsyncApiDataMap`), AsyncAPI metadata
  (servers, tags, docs), and manages a per-transport schema registry.
- `HeartbeatConfig`/`HeartbeatGuard` – configure liveness probes for long-lived
  connections. Both `WsApp` and `Nt4App` expose `configure_heartbeat` to tweak
  intervals and timeouts.
- A common `Error` type (`lib_transport::Error`) re-exported from
  `lib-ws::error` and `lib-nt4::error` for consistent diagnostics.

Inject dependencies with `app.add_data(Arc::new(MyState))` and retrieve them via
`app.get_data::<MyState>()`. This is the same data map used inside the Helios
application.

## Standalone WebSocket Service

```rust
use actix_web::{http::header, web, App, HttpRequest, HttpResponse, HttpServer};
use lib_ws::{ws, AsyncApiData, WsApp, WsContext};
use lib_transport::HeartbeatConfig;
use std::sync::Arc;

#[derive(Clone)]
struct SessionState {
    token: String,
}

#[ws("demo.echo", summary = "Echo", description = "Echo incoming payload" )]
async fn echo(ctx: WsContext, payload: lib_asyncapi::AsyncApiPayload<String>) {
    ctx.push(payload.into_inner());
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let mut app = WsApp::new();
    app.configure_heartbeat(HeartbeatConfig::new(std::time::Duration::from_secs(10), std::time::Duration::from_secs(30)));
    app.add_command(echo);
    app.add_data(Arc::new(SessionState { token: "secret".into() }));

    HttpServer::new(move || {
        App::new()
            .wrap(actix_web::middleware::Logger::default())
            .app_data(web::Data::new(app.clone()))
            .route("/ws", web::get().to(ws_entry))
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}

async fn ws_entry(req: HttpRequest, stream: web::Payload, app: web::Data<WsApp>) -> actix_web::Result<HttpResponse> {
    // Reject unauthenticated clients before the websocket upgrade.
    if !authorised(&req, app.get_data::<SessionState>().map(|s| s.token.clone())) {
        return Ok(HttpResponse::Unauthorized().finish());
    }
    app.get_ref().clone().start(req, stream)
}

fn authorised(req: &HttpRequest, expected: Option<String>) -> bool {
    match expected {
        Some(token) => req.headers().get(header::AUTHORIZATION).map(|h| h == token).unwrap_or(false),
        None => false,
    }
}
```

The session will emit heartbeat pings every 10 seconds and close if the peer
misses two responses. Handler metadata automatically contributes to AsyncAPI
schemas via the shared registry.

## NetworkTables (NT4) Bridge

```rust
use lib_nt4::Nt4App;
use lib_transport::HeartbeatConfig;
use nt_client::NewClientOptions;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let mut app = Nt4App::with_options(NewClientOptions::new("helios"));
    app.configure_heartbeat(HeartbeatConfig::new(std::time::Duration::from_millis(20), std::time::Duration::from_millis(200)));
    app.add_data(Arc::new(MyRegistry::default()));
    app.add_pub(my_publish_handler);
    app.add_sub(my_subscribe_handler);

    let app = Arc::new(app);
    app.clone().start().await;
}
```

Heartbeat guards log warnings when a publish loop misses its deadline, helping
surface connection stalls early.

## JSON-RPC Authentication

The `lib-api-macro::rpc` macro now understands `actix_web::web::ReqData<T>` and
`lib_asyncapi` extractors (`AsyncApiData`, `AsyncApiPayload`, `AsyncApiPath`).
Use Actix middleware to attach auth context and fetch it inside handlers:

```rust
#[derive(Clone)]
struct AuthContext { user_id: uuid::Uuid }

#[rpc("user.info")]
async fn user_info(ctx: actix_web::web::ReqData<AuthContext>) -> ApiResult<actix_web::web::Json<UserDto>> {
    let dto = fetch_user(ctx.into_inner().user_id).await?;
    Ok(actix_web::web::Json(dto))
}
```

Register the context with Actix in your JSON-RPC service setup:

```rust
let auth_ctx = actix_web::web::ReqData::new(AuthContext { user_id });
let server = jsonrpc_v2::Server::new()
    .with_data(jsonrpc_v2::Data::new(auth_ctx))
    .with_method(USER_INFO__METHOD, user_info_rpc)
    .finish();
```

Combining `ReqData` with the shared transport error type keeps auth failures and
JSON parsing errors aligned across transports.

## AsyncAPI Aggregation

Each handler registers its schema once via `TransportContext::schemas_mut()`.
When composing services from several crates, merge contexts by cloning the base
`WsApp`/`Nt4App` and adding handlers selectively. The schema registry deduplicates
registrations using the handler type ID, so reusing a handler in multiple routes
keeps the generated spec clean.

For cross-crate documentation, export the AsyncAPI JSON with
`app.asyncapi_json()` and publish it alongside service metadata.
