use actix_web::{App, guard, test, web};
use jsonrpc_v2::{Data as RpcData, Params, Server};
use lib_api_macro::rpc;
use serde_json::json;

#[derive(Debug)]
struct SimpleError;

impl jsonrpc_v2::ErrorLike for SimpleError {}
impl std::fmt::Display for SimpleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "error")
    }
}

mod api {
    pub mod endpoints {
        pub type ApiResult<T> = Result<T, super::super::SimpleError>;
    }
}

#[derive(Clone)]
struct AppState(u32);

#[rpc("add.method")]
async fn add(data: actix_web::web::Data<AppState>, actix_web::web::Json(v): actix_web::web::Json<u32>) -> api::endpoints::ApiResult<actix_web::web::Json<u32>> {
    Ok(actix_web::web::Json(data.get_ref().0 + v))
}

#[rpc("combo.method")]
async fn combo(
    data: actix_web::web::Data<AppState>,
    id: actix_web::web::Path<u32>,
    body: actix_web::web::Json<u32>,
    q: actix_web::web::Query<u32>,
) -> api::endpoints::ApiResult<actix_web::web::Json<u32>> {
    Ok(actix_web::web::Json(data.get_ref().0 + id.into_inner() + body.into_inner() + q.into_inner()))
}

async fn add_srv(data: RpcData<actix_web::web::Data<AppState>>, Params(v): Params<u32>) -> api::endpoints::ApiResult<u32> {
    add_rpc(data, Params(v)).await
}

async fn combo_srv(data: RpcData<actix_web::web::Data<AppState>>, Params((id, body, q)): Params<(u32, u32, u32)>) -> api::endpoints::ApiResult<u32> {
    combo_rpc(data, Params(id), Params(body), Params(q)).await
}

#[lib_test::tokio_test]
async fn rpc_actix_integration() {
    let state = actix_web::web::Data::new(AppState(1));
    let server = Server::new().with_data(RpcData::new(state.clone())).with_method(ADD__METHOD, add_srv).with_method(COMBO__METHOD, combo_srv).finish();

    let app = test::init_service(App::new().service(web::service("/rpc").guard(guard::Post()).finish(server.into_actix_web_service()))).await;

    let req = test::TestRequest::post().uri("/rpc").set_payload(json!({"jsonrpc":"2.0","method":ADD__METHOD,"params":2,"id":1}).to_string()).to_request();
    let resp = test::call_service(&app, req).await;
    let body = test::read_body(resp).await;
    let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(val["result"], json!(3));

    let req = test::TestRequest::post().uri("/rpc").set_payload(json!({"jsonrpc":"2.0","method":COMBO__METHOD,"params":[2,3,4],"id":2}).to_string()).to_request();
    let resp = test::call_service(&app, req).await;
    let body = test::read_body(resp).await;
    let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(val["result"], json!(10));
}
