//! `/v1/auth/*` and the auth middleware in front of every route.

use std::net::{IpAddr, SocketAddr};

use axum::{
    Extension, Json,
    extract::{ConnectInfo, Path, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use crate::{
    SharedState,
    auth::{AuthMode, CSRF_HEADER, Caller, NewSession, SESSION_COOKIE, SESSION_TTL_MS, secrets_equal},
    error::{ApiError, ApiResult},
};

/// Routes anyone may call on a secured device. Everything else under `/v1` needs a session or
/// a token. Paths outside `/v1` (a future static UI) are not the API and stay public.
fn is_public(method: &Method, path: &str) -> bool {
    let read = method == Method::GET || method == Method::HEAD;
    match path {
        "/v1/health" | "/v1/identity" | "/v1/auth/status" => read,
        "/v1/auth/login" | "/v1/auth/logout" => method == Method::POST,
        _ => !(path == "/v1" || path.starts_with("/v1/")),
    }
}

fn is_mutation(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim()).filter(|token| !token.is_empty())
}

fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    headers.get_all(header::COOKIE).iter().filter_map(|value| value.to_str().ok()).flat_map(|value| value.split(';')).find_map(|pair| {
        let (name, value) = pair.trim().split_once('=')?;
        (name == SESSION_COOKIE && !value.is_empty()).then_some(value)
    })
}

/// Decide who is calling, refuse what they may not do, and hand the [`Caller`] to handlers.
pub async fn middleware(State(state): State<SharedState>, mut request: Request, next: Next) -> Response {
    let headers = request.headers();
    let caller = state.auth.identify(bearer(headers), session_cookie(headers));
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    if !caller.is_authenticated() && !is_public(&method, &path) {
        let message = if bearer(request.headers()).is_some() { "this API token is not valid on this device" } else { "this device is secured: sign in or send an API token (Authorization: Bearer)" };
        return ApiError::unauthorized(message).into_response();
    }
    // Cookies ride along with cross-site requests; a mutation made with the session cookie must
    // also carry the session's CSRF token, which only the HeliOS UI can read.
    if let Caller::Session { csrf, .. } = &caller
        && is_mutation(&method)
        && path != "/v1/auth/logout"
    {
        let presented = request.headers().get(CSRF_HEADER).and_then(|value| value.to_str().ok()).unwrap_or("");
        if !secrets_equal(presented, csrf) {
            return ApiError::forbidden(format!("missing or wrong {CSRF_HEADER} header (read csrf_token from /v1/auth/status)")).into_response();
        }
    }
    request.extensions_mut().insert(caller);
    next.run(request).await
}

fn client_ip(connect: Option<Extension<ConnectInfo<SocketAddr>>>) -> Option<IpAddr> {
    connect.map(|Extension(ConnectInfo(addr))| addr.ip())
}

fn session_cookie_header(session: &NewSession) -> HeaderValue {
    let value = format!("{SESSION_COOKIE}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}", session.cookie_value, SESSION_TTL_MS / 1000);
    HeaderValue::from_str(&value).expect("hex cookie value")
}

fn clear_cookie_header() -> HeaderValue {
    HeaderValue::from_static("helios_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0")
}

fn with_session(state: &SharedState, session: NewSession) -> Response {
    let caller = Caller::Session { key: [0; 32], csrf: session.csrf.clone(), expires_at_ms: session.expires_at_ms };
    let status = state.auth.status(&caller);
    let mut response = Json(status).into_response();
    response.headers_mut().insert(header::SET_COOKIE, session_cookie_header(&session));
    response
}

fn caller_of(caller: Option<Extension<Caller>>) -> Caller {
    caller.map_or(Caller::Anonymous, |Extension(caller)| caller)
}

pub async fn status(State(state): State<SharedState>, caller: Option<Extension<Caller>>) -> Response {
    Json(state.auth.status(&caller_of(caller))).into_response()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordBody {
    pub password: String,
}

/// Sign in with the device password: sets the session cookie, answers the status (with the
/// CSRF token).
pub async fn login(State(state): State<SharedState>, connect: Option<Extension<ConnectInfo<SocketAddr>>>, Json(body): Json<PasswordBody>) -> ApiResult<Response> {
    if state.auth.mode() == AuthMode::Open {
        return Err(ApiError::conflict("the device is open; there is nothing to sign in to"));
    }
    state.auth.verify_password(client_ip(connect), &body.password).await?;
    Ok(with_session(&state, state.auth.login()?))
}

pub async fn logout(State(state): State<SharedState>, caller: Option<Extension<Caller>>) -> Response {
    state.auth.logout(&caller_of(caller));
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(header::SET_COOKIE, clear_cookie_header());
    response
}

/// Open → secured. Signs the caller in.
pub async fn enable(State(state): State<SharedState>, Json(body): Json<PasswordBody>) -> ApiResult<Response> {
    let session = state.auth.enable(&body.password).await?;
    Ok(with_session(&state, session))
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisableBody {
    #[serde(default)]
    pub password: Option<String>,
}

/// Secured → open. A browser session must re-enter the password; an API token is enough on
/// its own.
pub async fn disable(State(state): State<SharedState>, caller: Option<Extension<Caller>>, connect: Option<Extension<ConnectInfo<SocketAddr>>>, body: Option<Json<DisableBody>>) -> ApiResult<Response> {
    let caller = caller_of(caller);
    match &caller {
        Caller::Open => return Err(ApiError::conflict("the device is already open")),
        Caller::Anonymous => return Err(ApiError::unauthorized("sign in to turn security off")),
        Caller::Session { .. } => {
            let password = body.and_then(|Json(body)| body.password).ok_or_else(|| ApiError::forbidden("enter the device password to turn security off"))?;
            state.auth.verify_password(client_ip(connect), &password).await.map_err(reconfirm_error)?;
        }
        Caller::Token { .. } => {}
    }
    state.auth.disable()?;
    let mut response = Json(state.auth.status(&Caller::Open)).into_response();
    response.headers_mut().insert(header::SET_COOKIE, clear_cookie_header());
    Ok(response)
}

/// A wrong re-entered password is a 403, not a 401: the caller is signed in and stays so.
fn reconfirm_error(error: ApiError) -> ApiError {
    if error.code == crate::error::ErrorCode::Unauthorized { ApiError::forbidden(error.message) } else { error }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangePasswordBody {
    pub current_password: String,
    pub new_password: String,
}

/// Change the password. Ends every browser session (the caller gets a new one); tokens stay.
pub async fn change_password(
    State(state): State<SharedState>,
    caller: Option<Extension<Caller>>,
    connect: Option<Extension<ConnectInfo<SocketAddr>>>,
    Json(body): Json<ChangePasswordBody>,
) -> ApiResult<Response> {
    if caller_of(caller) == Caller::Open {
        return Err(ApiError::conflict("the device is open; secure it to set a password"));
    }
    state.auth.verify_password(client_ip(connect), &body.current_password).await.map_err(reconfirm_error)?;
    Ok(with_session(&state, state.auth.change_password(&body.new_password).await?))
}

pub async fn list_tokens(State(state): State<SharedState>) -> ApiResult<Response> {
    Ok(Json(state.auth.tokens()?).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateTokenBody {
    pub label: String,
}

/// 201 with the token. The secret is shown in this answer only.
pub async fn create_token(State(state): State<SharedState>, Json(body): Json<CreateTokenBody>) -> ApiResult<Response> {
    Ok((StatusCode::CREATED, Json(state.auth.create_token(&body.label)?)).into_response())
}

pub async fn revoke_token(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    super::check_id(&id)?;
    state.auth.revoke_token(&id)?;
    Ok(StatusCode::NO_CONTENT)
}
