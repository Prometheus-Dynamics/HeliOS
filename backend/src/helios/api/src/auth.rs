//! Device security: **open** (the default) or **secured**.
//!
//! - Open: no auth file, every request is allowed. This is how FRC robots run.
//! - Secured: a device password for people (an HttpOnly, SameSite=Strict session cookie plus a
//!   per-session CSRF token for mutations) and API tokens for tools (`Authorization: Bearer`).
//!
//! The state is one JSON file on the data partition ([`crate::auth_state`]): an argon2id
//! password hash and the SHA-256 of each API token (tokens are 256-bit random values, shown
//! once). Sessions live in memory only, so a restart of helios-api signs browsers out.
//! `helios-api auth reset` removes the file; the change is picked up on the next request.

use std::{
    collections::HashMap,
    fs,
    io::Write,
    net::IpAddr,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;

pub use crate::auth_state::AuthMode;

use crate::{
    error::{ApiError, ApiResult},
    host::now_ms,
};

pub const SESSION_COOKIE: &str = "helios_session";
pub const CSRF_HEADER: &str = "x-helios-csrf";
pub const TOKEN_PREFIX: &str = "helios_";
pub const MIN_PASSWORD_LEN: usize = 8;
pub const MAX_PASSWORD_LEN: usize = 256;
pub const MAX_LABEL_LEN: usize = 64;
pub const MAX_TOKENS: usize = 64;
/// A browser session lasts a week, then the user signs in again.
pub const SESSION_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;
const MAX_SESSIONS: usize = 64;
/// Failed sign-ins allowed per client address within [`LOGIN_WINDOW_MS`].
pub const LOGIN_MAX_FAILURES: u32 = 5;
pub const LOGIN_WINDOW_MS: u64 = 5 * 60 * 1000;
const FILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct AuthFile {
    version: u32,
    /// PHC string, argon2id.
    password_hash: String,
    password_set_at_ms: u64,
    #[serde(default)]
    tokens: Vec<StoredToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredToken {
    id: String,
    label: String,
    /// Hex SHA-256 of the whole token.
    sha256: String,
    /// The first characters of the token, for recognising it in a list.
    prefix: String,
    created_at_ms: u64,
    #[serde(default)]
    last_used_at_ms: Option<u64>,
}

/// An API token as listed (never the secret).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TokenInfo {
    pub id: String,
    pub label: String,
    pub prefix: String,
    pub created_at_ms: u64,
    pub last_used_at_ms: Option<u64>,
}

/// A freshly created token: the secret is in this answer and nowhere else.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NewToken {
    #[serde(flatten)]
    pub info: TokenInfo,
    pub token: String,
}

/// Who is calling, as decided by the auth middleware.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// The device is open: everyone is allowed.
    Open,
    /// Secured, and the request carries no valid credentials.
    Anonymous,
    Session {
        key: [u8; 32],
        csrf: String,
        expires_at_ms: u64,
    },
    Token {
        id: String,
    },
}

impl Caller {
    pub fn is_authenticated(&self) -> bool {
        !matches!(self, Self::Anonymous)
    }

    pub fn via(&self) -> Option<&'static str> {
        match self {
            Self::Open => Some("open"),
            Self::Anonymous => None,
            Self::Session { .. } => Some("session"),
            Self::Token { .. } => Some("token"),
        }
    }
}

/// A new browser session: the cookie value and its CSRF token.
#[derive(Debug, Clone)]
pub struct NewSession {
    pub cookie_value: String,
    pub csrf: String,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone)]
enum Loaded {
    Open,
    Secured(AuthFile),
    /// The file exists but cannot be used. Fail closed: nothing authenticates until
    /// `helios-api auth reset`.
    Unreadable(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileSignature {
    ino: u64,
    len: u64,
    mtime: i64,
    mtime_nsec: i64,
}

struct Session {
    csrf: String,
    expires_at_ms: u64,
}

struct Inner {
    loaded: Loaded,
    signature: Option<FileSignature>,
    sessions: HashMap<[u8; 32], Session>,
    /// Failed sign-ins per client address: (count, window start).
    failures: HashMap<Option<IpAddr>, (u32, u64)>,
    /// Token use seen since the file was last written (persisted with the next write, so normal
    /// API traffic never writes to flash).
    token_last_used: HashMap<String, u64>,
}

/// What `/v1/auth/status` reports.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthStatus {
    pub mode: &'static str,
    pub authenticated: bool,
    /// `open`, `session` or `token`; `null` when not authenticated.
    pub via: Option<&'static str>,
    /// Send this as `X-Helios-CSRF` with every mutating request made with the session cookie.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub csrf_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_expires_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_set_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<usize>,
    /// Set when the auth file is unreadable: every protected request is refused until
    /// `helios-api auth reset` on the device.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

pub struct Auth {
    path: PathBuf,
    inner: Mutex<Inner>,
    /// argon2id uses ~19 MiB per verification; bound how many run at once.
    hashing: Semaphore,
}

impl Auth {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let (loaded, signature) = load(&path);
        if let Loaded::Unreadable(problem) = &loaded {
            tracing::error!(path = %path.display(), %problem, "auth file is unreadable; refusing protected requests until `helios-api auth reset`");
        }
        Self { path, inner: Mutex::new(Inner { loaded, signature, sessions: HashMap::new(), failures: HashMap::new(), token_last_used: HashMap::new() }), hashing: Semaphore::new(2) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        let mut inner = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.refresh(&mut inner);
        inner
    }

    /// Reload the file when something else (`helios-api auth reset`) changed it.
    fn refresh(&self, inner: &mut Inner) {
        let signature = signature(&self.path);
        if signature == inner.signature {
            return;
        }
        let (loaded, signature) = load(&self.path);
        tracing::info!(path = %self.path.display(), mode = mode_of(&loaded).as_str(), "auth file changed outside helios-api; reloaded");
        inner.loaded = loaded;
        inner.signature = signature;
        inner.sessions.clear();
        inner.token_last_used.clear();
    }

    pub fn mode(&self) -> AuthMode {
        mode_of(&self.lock().loaded)
    }

    /// Decide who is calling from the `Authorization` header and the session cookie.
    pub fn identify(&self, bearer: Option<&str>, session_cookie: Option<&str>) -> Caller {
        let mut inner = self.lock();
        let now = now_ms();
        let tokens = match &inner.loaded {
            Loaded::Open => return Caller::Open,
            Loaded::Unreadable(_) => return Caller::Anonymous,
            Loaded::Secured(file) => &file.tokens,
        };
        if let Some(presented) = bearer {
            let digest = Sha256::digest(presented.as_bytes());
            let mut found = None;
            for token in tokens {
                let Ok(stored) = hex::decode(&token.sha256) else { continue };
                if bool::from(stored.ct_eq(digest.as_slice())) {
                    found = Some(token.id.clone());
                }
            }
            return match found {
                Some(id) => {
                    inner.token_last_used.insert(id.clone(), now);
                    Caller::Token { id }
                }
                None => Caller::Anonymous,
            };
        }
        if let Some(cookie) = session_cookie {
            let key: [u8; 32] = Sha256::digest(cookie.as_bytes()).into();
            inner.sessions.retain(|_, session| session.expires_at_ms > now);
            if let Some(session) = inner.sessions.get(&key) {
                return Caller::Session { key, csrf: session.csrf.clone(), expires_at_ms: session.expires_at_ms };
            }
        }
        Caller::Anonymous
    }

    pub fn status(&self, caller: &Caller) -> AuthStatus {
        let inner = self.lock();
        let mode = mode_of(&inner.loaded);
        let authenticated = caller.is_authenticated();
        let (password_set_at_ms, tokens) = match (&inner.loaded, authenticated) {
            (Loaded::Secured(file), true) => (Some(file.password_set_at_ms), Some(file.tokens.len())),
            _ => (None, None),
        };
        let problem = match &inner.loaded {
            Loaded::Unreadable(problem) => Some(format!("the device's auth file is unreadable ({problem}); run `helios-api auth reset` on the device")),
            _ => None,
        };
        let (csrf_token, session_expires_at_ms) = match caller {
            Caller::Session { csrf, expires_at_ms, .. } => (Some(csrf.clone()), Some(*expires_at_ms)),
            _ => (None, None),
        };
        AuthStatus { mode: mode.as_str(), authenticated, via: caller.via(), csrf_token, session_expires_at_ms, password_set_at_ms, tokens, problem }
    }

    /// Fails with 429 while `client` has used up its failed sign-ins.
    fn check_rate(&self, client: Option<IpAddr>) -> ApiResult<()> {
        let mut inner = self.lock();
        let now = now_ms();
        inner.failures.retain(|_, (_, start)| now.saturating_sub(*start) < LOGIN_WINDOW_MS);
        if let Some((count, start)) = inner.failures.get(&client)
            && *count >= LOGIN_MAX_FAILURES
        {
            let retry_s = (LOGIN_WINDOW_MS.saturating_sub(now.saturating_sub(*start))).div_ceil(1000);
            return Err(ApiError::too_many_requests(format!("too many failed sign-ins; try again in {retry_s} s")));
        }
        Ok(())
    }

    fn record_attempt(&self, client: Option<IpAddr>, ok: bool) {
        let mut inner = self.lock();
        if ok {
            inner.failures.remove(&client);
        } else {
            let now = now_ms();
            let entry = inner.failures.entry(client).or_insert((0, now));
            entry.0 += 1;
        }
    }

    /// Check the device password, rate limited per client address.
    pub async fn verify_password(&self, client: Option<IpAddr>, password: &str) -> ApiResult<()> {
        self.check_rate(client)?;
        let hash = match &self.lock().loaded {
            Loaded::Secured(file) => file.password_hash.clone(),
            Loaded::Open => return Err(ApiError::conflict("the device is open; there is no password")),
            Loaded::Unreadable(_) => return Err(ApiError::unauthorized("the device's auth file is unreadable; run `helios-api auth reset` on the device")),
        };
        let ok = {
            let _permit = self.hashing.acquire().await.map_err(|_| ApiError::internal("password hashing is shut down"))?;
            let password = password.to_owned();
            tokio::task::spawn_blocking(move || verify_hash(&hash, &password)).await.map_err(|error| ApiError::internal(error.to_string()))?
        };
        self.record_attempt(client, ok);
        if ok { Ok(()) } else { Err(ApiError::unauthorized("wrong password")) }
    }

    async fn hash_password(&self, password: &str) -> ApiResult<String> {
        check_password(password)?;
        let _permit = self.hashing.acquire().await.map_err(|_| ApiError::internal("password hashing is shut down"))?;
        let password = password.to_owned();
        tokio::task::spawn_blocking(move || hash_password(&password)).await.map_err(|error| ApiError::internal(error.to_string()))?
    }

    /// Open → secured with `password`. Returns a session for the caller.
    pub async fn enable(&self, password: &str) -> ApiResult<NewSession> {
        if self.mode() != AuthMode::Open {
            return Err(ApiError::conflict("the device is already secured"));
        }
        let password_hash = self.hash_password(password).await?;
        let mut inner = self.lock();
        if !matches!(inner.loaded, Loaded::Open) {
            return Err(ApiError::conflict("the device is already secured"));
        }
        let file = AuthFile { version: FILE_VERSION, password_hash, password_set_at_ms: now_ms(), tokens: Vec::new() };
        self.persist(&mut inner, file)?;
        inner.sessions.clear();
        inner.failures.clear();
        tracing::info!("device secured");
        new_session(&mut inner)
    }

    /// Secured → open: forget the password, the tokens and every session.
    pub fn disable(&self) -> ApiResult<()> {
        let mut inner = self.lock();
        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        inner.loaded = Loaded::Open;
        inner.signature = None;
        inner.sessions.clear();
        inner.token_last_used.clear();
        tracing::info!("device security turned off; the device is open");
        Ok(())
    }

    /// Replace the password. Every session ends; tokens stay valid. Returns a fresh session
    /// for the caller.
    pub async fn change_password(&self, new_password: &str) -> ApiResult<NewSession> {
        let password_hash = self.hash_password(new_password).await?;
        let mut inner = self.lock();
        let Loaded::Secured(mut file) = inner.loaded.clone() else {
            return Err(ApiError::conflict("the device is open; secure it first"));
        };
        file.password_hash = password_hash;
        file.password_set_at_ms = now_ms();
        self.persist(&mut inner, file)?;
        inner.sessions.clear();
        tracing::info!("device password changed; all sessions ended");
        new_session(&mut inner)
    }

    pub fn login(&self) -> ApiResult<NewSession> {
        let mut inner = self.lock();
        if !matches!(inner.loaded, Loaded::Secured(_)) {
            return Err(ApiError::conflict("the device is open; there is nothing to sign in to"));
        }
        new_session(&mut inner)
    }

    pub fn logout(&self, caller: &Caller) {
        if let Caller::Session { key, .. } = caller {
            self.lock().sessions.remove(key);
        }
    }

    pub fn tokens(&self) -> ApiResult<Vec<TokenInfo>> {
        let inner = self.lock();
        let Loaded::Secured(file) = &inner.loaded else {
            return Err(ApiError::conflict("the device is open; API tokens exist only on a secured device"));
        };
        Ok(file.tokens.iter().map(|token| token_info(token, &inner.token_last_used)).collect())
    }

    pub fn create_token(&self, label: &str) -> ApiResult<NewToken> {
        let label = label.trim();
        if label.is_empty() || label.chars().count() > MAX_LABEL_LEN || label.chars().any(char::is_control) {
            return Err(ApiError::unprocessable(format!("a token label is 1 to {MAX_LABEL_LEN} printable characters")));
        }
        let mut inner = self.lock();
        let Loaded::Secured(mut file) = inner.loaded.clone() else {
            return Err(ApiError::conflict("the device is open; secure it before creating API tokens"));
        };
        if file.tokens.len() >= MAX_TOKENS {
            return Err(ApiError::conflict(format!("at most {MAX_TOKENS} API tokens; revoke one first")));
        }
        let secret = format!("{TOKEN_PREFIX}{}", hex::encode(random_bytes::<32>()?));
        let stored = StoredToken {
            id: format!("tok_{}", hex::encode(random_bytes::<6>()?)),
            label: label.to_string(),
            sha256: hex::encode(Sha256::digest(secret.as_bytes())),
            prefix: secret[..TOKEN_PREFIX.len() + 8].to_string(),
            created_at_ms: now_ms(),
            last_used_at_ms: None,
        };
        let info = token_info(&stored, &HashMap::new());
        file.tokens.push(stored);
        self.persist(&mut inner, file)?;
        tracing::info!(id = %info.id, label = %info.label, "API token created");
        Ok(NewToken { info, token: secret })
    }

    pub fn revoke_token(&self, id: &str) -> ApiResult<()> {
        let mut inner = self.lock();
        let Loaded::Secured(mut file) = inner.loaded.clone() else {
            return Err(ApiError::conflict("the device is open; there are no API tokens"));
        };
        let before = file.tokens.len();
        file.tokens.retain(|token| token.id != id);
        if file.tokens.len() == before {
            return Err(ApiError::not_found(format!("no API token {id}")));
        }
        self.persist(&mut inner, file)?;
        inner.token_last_used.remove(id);
        tracing::info!(%id, "API token revoked");
        Ok(())
    }

    /// Write the file atomically (tmp, fsync, rename, fsync dir), 0600 in a 0700 directory.
    fn persist(&self, inner: &mut Inner, mut file: AuthFile) -> ApiResult<()> {
        for token in &mut file.tokens {
            if let Some(used) = inner.token_last_used.get(&token.id) {
                token.last_used_at_ms = Some(token.last_used_at_ms.map_or(*used, |old| old.max(*used)));
            }
        }
        write_file(&self.path, &file).map_err(|error| ApiError::internal(format!("failed to write {}: {error}", self.path.display())))?;
        inner.signature = signature(&self.path);
        inner.loaded = Loaded::Secured(file);
        inner.token_last_used.clear();
        Ok(())
    }
}

fn mode_of(loaded: &Loaded) -> AuthMode {
    match loaded {
        Loaded::Open => AuthMode::Open,
        Loaded::Secured(_) | Loaded::Unreadable(_) => AuthMode::Secured,
    }
}

fn token_info(token: &StoredToken, seen: &HashMap<String, u64>) -> TokenInfo {
    let last_used_at_ms = match (token.last_used_at_ms, seen.get(&token.id)) {
        (Some(a), Some(b)) => Some(a.max(*b)),
        (a, b) => a.or(b.copied()),
    };
    TokenInfo { id: token.id.clone(), label: token.label.clone(), prefix: token.prefix.clone(), created_at_ms: token.created_at_ms, last_used_at_ms }
}

fn new_session(inner: &mut Inner) -> ApiResult<NewSession> {
    let now = now_ms();
    inner.sessions.retain(|_, session| session.expires_at_ms > now);
    if inner.sessions.len() >= MAX_SESSIONS
        && let Some(oldest) = inner.sessions.iter().min_by_key(|(_, session)| session.expires_at_ms).map(|(key, _)| *key)
    {
        inner.sessions.remove(&oldest);
    }
    let cookie_value = hex::encode(random_bytes::<32>()?);
    let csrf = hex::encode(random_bytes::<32>()?);
    let expires_at_ms = now + SESSION_TTL_MS;
    inner.sessions.insert(Sha256::digest(cookie_value.as_bytes()).into(), Session { csrf: csrf.clone(), expires_at_ms });
    Ok(NewSession { cookie_value, csrf, expires_at_ms })
}

pub fn check_password(password: &str) -> ApiResult<()> {
    let len = password.chars().count();
    if !(MIN_PASSWORD_LEN..=MAX_PASSWORD_LEN).contains(&len) {
        return Err(ApiError::unprocessable(format!("the password must be {MIN_PASSWORD_LEN} to {MAX_PASSWORD_LEN} characters")));
    }
    Ok(())
}

fn random_bytes<const N: usize>() -> ApiResult<[u8; N]> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|error| ApiError::internal(format!("no OS randomness: {error}")))?;
    Ok(bytes)
}

fn hash_password(password: &str) -> ApiResult<String> {
    let salt = SaltString::encode_b64(&random_bytes::<16>()?).map_err(|error| ApiError::internal(error.to_string()))?;
    // Argon2::default() is argon2id v19 with the OWASP-recommended parameters (19 MiB, t=2, p=1).
    Argon2::default().hash_password(password.as_bytes(), &salt).map(|hash| hash.to_string()).map_err(|error| ApiError::internal(format!("password hashing failed: {error}")))
}

fn verify_hash(hash: &str, password: &str) -> bool {
    // verify_password compares the outputs in constant time.
    PasswordHash::new(hash).is_ok_and(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
}

/// Compare two secrets without leaking where they differ.
pub fn secrets_equal(a: &str, b: &str) -> bool {
    a.len() == b.len() && bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}

fn signature(path: &Path) -> Option<FileSignature> {
    fs::metadata(path).ok().map(|meta| FileSignature { ino: meta.ino(), len: meta.len(), mtime: meta.mtime(), mtime_nsec: meta.mtime_nsec() })
}

fn load(path: &Path) -> (Loaded, Option<FileSignature>) {
    let signature = signature(path);
    let loaded = match fs::read(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Loaded::Open,
        Err(error) => Loaded::Unreadable(error.to_string()),
        Ok(bytes) => match serde_json::from_slice::<AuthFile>(&bytes) {
            Ok(file) if file.version == FILE_VERSION && PasswordHash::new(&file.password_hash).is_ok() => Loaded::Secured(file),
            Ok(file) if file.version != FILE_VERSION => Loaded::Unreadable(format!("unsupported version {}", file.version)),
            Ok(_) => Loaded::Unreadable("invalid password hash".into()),
            Err(error) => Loaded::Unreadable(error.to_string()),
        },
    };
    (loaded, signature)
}

fn write_file(path: &Path, file: &AuthFile) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(file).map_err(std::io::Error::other)?;
    {
        let mut out = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        out.write_all(&bytes)?;
        out.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    fs::File::open(dir)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn enable_login_tokens_and_disable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("auth").join("auth.json");
        let auth = Auth::new(&path);
        assert_eq!(auth.mode(), AuthMode::Open);
        assert_eq!(auth.identify(None, None), Caller::Open);
        assert!(auth.enable("short").await.is_err(), "too short");

        let session = auth.enable("correct horse").await.expect("enable");
        assert_eq!(auth.mode(), AuthMode::Secured);
        assert_eq!(fs::metadata(&path).expect("file").permissions().mode() & 0o777, 0o600);
        let text = fs::read_to_string(&path).expect("read");
        assert!(text.contains("$argon2id$") && !text.contains("correct horse"));

        assert!(matches!(auth.identify(None, Some(&session.cookie_value)), Caller::Session { .. }));
        assert_eq!(auth.identify(None, Some("nope")), Caller::Anonymous);
        assert!(auth.verify_password(None, "correct horse").await.is_ok());
        assert!(auth.verify_password(None, "wrong horse").await.is_err());

        let token = auth.create_token("Atlas").expect("token");
        assert!(token.token.starts_with(TOKEN_PREFIX));
        assert!(!fs::read_to_string(&path).expect("read").contains(&token.token), "only the hash is stored");
        assert_eq!(auth.identify(Some(&token.token), None), Caller::Token { id: token.info.id.clone() });
        assert_eq!(auth.identify(Some("helios_bogus"), None), Caller::Anonymous);
        auth.revoke_token(&token.info.id).expect("revoke");
        assert_eq!(auth.identify(Some(&token.token), None), Caller::Anonymous);

        auth.disable().expect("disable");
        assert_eq!(auth.mode(), AuthMode::Open);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn sign_ins_are_rate_limited() {
        let dir = tempfile::tempdir().expect("tempdir");
        let auth = Auth::new(dir.path().join("auth.json"));
        auth.enable("correct horse").await.expect("enable");
        let client = Some(IpAddr::from([10, 0, 0, 9]));
        for _ in 0..LOGIN_MAX_FAILURES {
            assert_eq!(auth.verify_password(client, "wrong").await.expect_err("wrong").code, crate::error::ErrorCode::Unauthorized);
        }
        assert_eq!(auth.verify_password(client, "correct horse").await.expect_err("limited").code, crate::error::ErrorCode::TooManyRequests);
        assert!(auth.verify_password(Some(IpAddr::from([10, 0, 0, 10])), "correct horse").await.is_ok(), "other clients are unaffected");
    }

    #[tokio::test]
    async fn external_reset_and_corrupt_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("auth.json");
        let auth = Auth::new(&path);
        let session = auth.enable("correct horse").await.expect("enable");
        assert!(crate::auth_state::reset(&path).expect("reset"));
        assert_eq!(auth.mode(), AuthMode::Open);
        assert_eq!(auth.identify(None, Some(&session.cookie_value)), Caller::Open);

        fs::write(&path, b"garbage").expect("write");
        assert_eq!(auth.mode(), AuthMode::Secured, "fails closed");
        assert_eq!(auth.identify(None, None), Caller::Anonymous);
        assert!(auth.status(&Caller::Anonymous).problem.is_some());
    }
}
