use lib_api_macro::rpc;
use std::sync::Arc;

mod api {
    pub mod endpoints {
        pub type ApiResult<T> = Result<T, ()>;
    }
}

#[rpc("test.method")]
async fn no_args() -> api::endpoints::ApiResult<actix_web::web::Json<u32>> {
    Ok(actix_web::web::Json(5))
}

#[lib_test::tokio_test]
async fn rpc_wrapper_unwraps_json() {
    assert_eq!(NO_ARGS__METHOD, "test.method");
    let val = no_args_rpc().await.unwrap();
    assert_eq!(val, 5);
}

#[rpc("sum.method")]
async fn sum(actix_web::web::Json(arg): actix_web::web::Json<u32>) -> api::endpoints::ApiResult<actix_web::web::Json<u32>> {
    Ok(actix_web::web::Json(arg + 3))
}

#[lib_test::tokio_test]
async fn rpc_wrapper_with_args() {
    let params = jsonrpc_v2::Params(2u32);
    let res = sum_rpc(params).await.unwrap();
    assert_eq!(res, 5);
}

#[derive(Clone)]
struct AppState(u32);

#[rpc("data.method")]
async fn needs_data(data: actix_web::web::Data<AppState>) -> api::endpoints::ApiResult<actix_web::web::Json<u32>> {
    Ok(actix_web::web::Json(data.get_ref().0))
}

#[lib_test::tokio_test]
async fn rpc_wrapper_with_data() {
    let params = jsonrpc_v2::Data::new(actix_web::web::Data::new(AppState(4)));
    let res = needs_data_rpc(params).await.unwrap();
    assert_eq!(res, 4);
}

#[rpc("path.method")]
async fn path_arg(id: actix_web::web::Path<u32>) -> api::endpoints::ApiResult<actix_web::web::Json<u32>> {
    Ok(actix_web::web::Json(id.into_inner() + 1))
}

#[lib_test::tokio_test]
async fn rpc_wrapper_with_path() {
    let params = jsonrpc_v2::Params(2u32);
    let res = path_arg_rpc(params).await.unwrap();
    assert_eq!(res, 3);
}

#[rpc("query.method")]
async fn query_arg(v: actix_web::web::Query<u32>) -> api::endpoints::ApiResult<actix_web::web::Json<u32>> {
    Ok(actix_web::web::Json(v.into_inner() * 2))
}

#[lib_test::tokio_test]
async fn rpc_wrapper_with_query() {
    let params = jsonrpc_v2::Params(5u32);
    let res = query_arg_rpc(params).await.unwrap();
    assert_eq!(res, 10);
}

#[rpc("combo.method")]
async fn combo_all(
    data: actix_web::web::Data<AppState>,
    id: actix_web::web::Path<u32>,
    body: actix_web::web::Json<u32>,
    q: actix_web::web::Query<u32>,
) -> api::endpoints::ApiResult<actix_web::web::Json<u32>> {
    Ok(actix_web::web::Json(data.get_ref().0 + id.into_inner() + body.into_inner() + q.into_inner()))
}

#[lib_test::tokio_test]
async fn rpc_wrapper_with_all_params() {
    assert_eq!(COMBO_ALL__METHOD, "combo.method");
    let d = jsonrpc_v2::Data::new(actix_web::web::Data::new(AppState(1)));
    let id = jsonrpc_v2::Params(2u32);
    let body = jsonrpc_v2::Params(3u32);
    let q = jsonrpc_v2::Params(4u32);
    let res = combo_all_rpc(d, id, body, q).await.unwrap();
    assert_eq!(res, 10);
}

#[rpc("async.payload")]
async fn async_payload(body: lib_asyncapi::AsyncApiPayload<u32>) -> api::endpoints::ApiResult<actix_web::web::Json<u32>> {
    Ok(actix_web::web::Json(body.into_inner() + 1))
}

#[lib_test::tokio_test]
async fn rpc_wrapper_with_async_payload() {
    let params = jsonrpc_v2::Params(9u32);
    let res = async_payload_rpc(params).await.unwrap();
    assert_eq!(res, 10);
}

#[rpc("async.path.{id}")]
async fn async_path(path: lib_asyncapi::AsyncApiPath<(u32,)>) -> api::endpoints::ApiResult<actix_web::web::Json<u32>> {
    let (id,) = path.into_inner();
    Ok(actix_web::web::Json(id * 2))
}

#[lib_test::tokio_test]
async fn rpc_wrapper_with_async_path() {
    let params = jsonrpc_v2::Params((7u32,));
    let res = async_path_rpc(params).await.unwrap();
    assert_eq!(res, 14);
}

#[rpc("async.data")]
async fn async_data(data: lib_asyncapi::AsyncApiData<AppState>) -> api::endpoints::ApiResult<actix_web::web::Json<u32>> {
    let inner = data.into_inner();
    Ok(actix_web::web::Json(inner.0 * 3))
}

#[lib_test::tokio_test]
async fn rpc_wrapper_with_async_data() {
    let payload = lib_asyncapi::AsyncApiData::new(Arc::new(AppState(4)));
    let params = jsonrpc_v2::Data::new(payload);
    let res = async_data_rpc(params).await.unwrap();
    assert_eq!(res, 12);
}
