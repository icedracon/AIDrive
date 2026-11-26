// Local trust (migration 0014): on a single-person install bound to loopback,
// the computer's account is the owner, so a local assistant connects itself.
// Off unless AIDRIVE_LOCAL_TRUST=1; every route answers 404 otherwise.
//
// Headers cannot tell a browser from a local program (anything can send
// Origin or Sec-Fetch-Site), so owner rights rest on secrets in the user's
// AIDrive folders: claiming a device needs local.key, which only the API
// writes, and the web app gets an owner session only through the one-time
// link a device mints (`aidrive open`). Pilot review 2026-11.

use crate::{
    AppState,
    agents::LOCAL_SCOPES,
    auth::{ApiError, new_token, pool, session_cookie, token_hash, unavailable},
    workspaces::{normalize_name, scoped},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
    sync::OnceLock,
};

pub const DEVICE_PREFIX: &str = "aidrive_device_";
pub const LOCAL_KEY_HEADER: &str = "x-aidrive-local-key";
static LOCAL_KEY_HASH: OnceLock<Vec<u8>> = OnceLock::new();

pub fn data_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("AIDRIVE_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("AIDrive")
}

// A fresh key on every start, readable by this user only; a program that
// wants to claim an owner device must read it from disk.
pub fn init_local_key() -> io::Result<PathBuf> {
    let dir = data_root();
    fs::create_dir_all(&dir)?;
    let path = dir.join("local.key");
    let (key, hash) = new_token();
    write_private(&path, &key)?;
    let _ = LOCAL_KEY_HASH.set(hash);
    Ok(path)
}

#[cfg(test)]
pub fn set_local_key_for_tests(key: &str) {
    let _ = LOCAL_KEY_HASH.set(token_hash(key));
}

fn write_private(path: &FsPath, value: &str) -> io::Result<()> {
    let tmp = path.with_extension("key.tmp");
    let _ = fs::remove_file(&tmp);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    io::Write::write_all(&mut options.open(&tmp)?, value.as_bytes())?;
    fs::rename(&tmp, path)
}

fn require_local_key(headers: &HeaderMap) -> Result<(), ApiError> {
    let presented = headers
        .get(LOCAL_KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(token_hash);
    match (LOCAL_KEY_HASH.get(), presented) {
        (Some(expected), Some(presented)) if *expected == presented => Ok(()),
        _ => Err(ApiError(StatusCode::FORBIDDEN, "local_key_required")),
    }
}

#[cfg(test)]
pub static FORCE_LOCAL_TRUST: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn local_trust_enabled() -> bool {
    #[cfg(test)]
    if FORCE_LOCAL_TRUST.load(std::sync::atomic::Ordering::SeqCst) {
        return true;
    }
    let loopback = std::env::var("AIDRIVE_API_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".into())
        .parse::<SocketAddr>()
        .is_ok_and(|a| a.ip().is_loopback());
    loopback && std::env::var("AIDRIVE_LOCAL_TRUST").is_ok_and(|v| v == "1")
}

fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "not_found")
}

fn invalid() -> ApiError {
    ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request")
}

fn loopback_host(host: &str) -> bool {
    let name = match host.rsplit_once(':') {
        Some((name, port)) if !name.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    };
    matches!(name, "127.0.0.1" | "localhost" | "[::1]")
}

// Browsers attach Origin / Fetch Metadata and a DNS-rebound page carries a
// foreign Host; a local command does neither. This keeps web pages, which
// could pick their own device secret, from claiming the owner. A local
// command talks to the API directly, never through the web app's /api
// proxy: a proxied request may come from another computer whose Host the
// proxy cannot vouch for (SECURITY-REVIEW-2026-11 F1).
fn require_local_caller(headers: &HeaderMap) -> Result<(), ApiError> {
    if !local_trust_enabled() {
        return Err(not_found());
    }
    let host_ok = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .is_some_and(loopback_host);
    let proxied = ["x-forwarded-host", "x-forwarded-for", "forwarded"]
        .iter()
        .any(|h| headers.contains_key(*h));
    if !host_ok
        || proxied
        || headers.contains_key(header::ORIGIN)
        || headers.contains_key("sec-fetch-site")
    {
        return Err(ApiError(StatusCode::FORBIDDEN, "local_command_only"));
    }
    Ok(())
}

fn hex32(value: &str) -> Option<Vec<u8>> {
    (value.len() == 64)
        .then(|| {
            (0..32)
                .map(|i| u8::from_str_radix(&value[2 * i..2 * i + 2], 16).ok())
                .collect()
        })
        .flatten()
}

fn device_token(headers: &HeaderMap) -> Result<String, ApiError> {
    let denied = || ApiError(StatusCode::UNAUTHORIZED, "invalid_device_token");
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(denied)?;
    let (scheme, token) = value.split_once(' ').ok_or_else(denied)?;
    let secret = token.strip_prefix(DEVICE_PREFIX).ok_or_else(denied)?;
    if !scheme.eq_ignore_ascii_case("Bearer")
        || secret.len() != 64
        || !secret.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(denied());
    }
    Ok(token.to_owned())
}

fn map_error(error: sqlx::Error) -> ApiError {
    match error.as_database_error().and_then(|e| e.code()).as_deref() {
        Some("AD409") => ApiError(StatusCode::CONFLICT, "not_single_person_install"),
        Some("AD429") => ApiError(StatusCode::TOO_MANY_REQUESTS, "too_many_devices"),
        Some("23505") => ApiError(StatusCode::CONFLICT, "already_used"),
        Some("23514") => invalid(),
        _ => unavailable(error),
    }
}

async fn owner_of(state: &AppState, headers: &HeaderMap) -> Result<String, ApiError> {
    let token = device_token(headers)?;
    sqlx::query_scalar::<_, Option<String>>("SELECT app.resolve_owner_device($1)::text")
        .bind(token_hash(&token))
        .fetch_one(pool(state)?)
        .await
        .map_err(unavailable)?
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "invalid_device_token"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    token_sha256: String,
    label: String,
}

#[derive(Serialize)]
struct Claimed {
    owner_email: String,
    created_owner: bool,
}

async fn claim(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Claim>,
) -> Result<(StatusCode, Json<Claimed>), ApiError> {
    require_local_caller(&headers)?;
    require_local_key(&headers)?;
    let label = body.label.trim();
    let hash = hex32(&body.token_sha256).ok_or_else(invalid)?;
    if label.is_empty() || label.chars().count() > 80 || label.chars().any(char::is_control) {
        return Err(invalid());
    }
    let (owner_email, created_owner): (String, bool) =
        sqlx::query_as("SELECT email, created FROM app.claim_local_device($1, $2)")
            .bind(hash)
            .bind(label)
            .fetch_one(pool(&state)?)
            .await
            .map_err(map_error)?;
    Ok((
        StatusCode::CREATED,
        Json(Claimed {
            owner_email,
            created_owner,
        }),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Connect {
    client_name: String,
    token_sha256: String,
    workspace_name: String,
    scope: Option<String>,
}

#[derive(Serialize)]
struct Connected {
    workspace_id: String,
    workspace_name: String,
    scope: String,
    created_workspace: bool,
    agent_id: String,
}

// Reuses the space the owner already has with this exact name, otherwise
// creates it; the grant is the same kind the web app issues.
async fn connect(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Connect>,
) -> Result<(StatusCode, Json<Connected>), ApiError> {
    require_local_caller(&headers)?;
    let user = owner_of(&state, &headers).await?;
    let name = body.client_name.trim();
    let hash = hex32(&body.token_sha256).ok_or_else(invalid)?;
    let workspace_name = normalize_name(&body.workspace_name).ok_or_else(invalid)?;
    let scope = body.scope.as_deref().unwrap_or("files:write");
    if name.is_empty()
        || name.chars().count() > 80
        || name.chars().any(char::is_control)
        || !LOCAL_SCOPES.contains(&scope)
    {
        return Err(invalid());
    }
    let mut tx = scoped(pool(&state)?, &user, None).await?;
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id::text FROM app.my_workspaces() WHERE role = 'owner' AND name = $1 ORDER BY id LIMIT 1",
    )
    .bind(&workspace_name)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_error)?;
    let created_workspace = existing.is_none();
    let workspace_id = match existing {
        Some(id) => id,
        None => sqlx::query_scalar("SELECT app.create_workspace($1)::text")
            .bind(&workspace_name)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_error)?,
    };
    sqlx::query("SELECT set_config('app.workspace_id', $1, true)")
        .bind(&workspace_id)
        .execute(&mut *tx)
        .await
        .map_err(map_error)?;
    if created_workspace {
        crate::review::set_mode(&mut tx, &workspace_id, crate::review::NEW_SPACE_MODE).await?;
    }
    let agent_id: String = sqlx::query_scalar("SELECT gen_random_uuid()::text")
        .fetch_one(&mut *tx)
        .await
        .map_err(map_error)?;
    sqlx::query("SELECT app.issue_agent_grant($1::uuid, $2::uuid, $3, $4, $5)")
        .bind(&workspace_id)
        .bind(&agent_id)
        .bind(name)
        .bind(hash)
        .bind(scope)
        .execute(&mut *tx)
        .await
        .map_err(map_error)?;
    tx.commit().await.map_err(map_error)?;
    Ok((
        StatusCode::CREATED,
        Json(Connected {
            workspace_id,
            workspace_name,
            scope: scope.to_owned(),
            created_workspace,
            agent_id,
        }),
    ))
}

#[derive(Serialize)]
struct LoginLink {
    url: String,
    expires_in: u32,
}

async fn login_link(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<LoginLink>, ApiError> {
    require_local_caller(&headers)?;
    let device = device_token(&headers)?;
    let (code, code_hash) = new_token();
    let issued: bool = sqlx::query_scalar("SELECT app.issue_local_login($1, $2)")
        .bind(token_hash(&device))
        .bind(code_hash)
        .fetch_one(pool(&state)?)
        .await
        .map_err(map_error)?;
    if !issued {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid_device_token"));
    }
    Ok(Json(LoginLink {
        url: format!("{}/api/local/login/{code}", state.web_origin),
        expires_in: 120,
    }))
}

// Opened in the browser through the web app's /api proxy, so the session
// cookie lands on the web origin. Single use, two minutes.
async fn redeem(State(state): State<AppState>, Path(code): Path<String>) -> Response {
    let home = |query: &str| {
        let mut response = StatusCode::SEE_OTHER.into_response();
        if let Ok(value) = HeaderValue::from_str(&format!("{}/{query}", state.web_origin)) {
            response.headers_mut().insert(header::LOCATION, value);
        }
        response
    };
    if !local_trust_enabled() || code.len() != 64 || !code.bytes().all(|b| b.is_ascii_hexdigit()) {
        return home("?signin=expired");
    }
    let Ok(db) = pool(&state) else {
        return home("?signin=unavailable");
    };
    let (session, session_hash) = new_token();
    let redeemed: Result<bool, _> = sqlx::query_scalar("SELECT app.redeem_local_login($1, $2)")
        .bind(token_hash(&code))
        .bind(session_hash)
        .fetch_one(db)
        .await;
    match redeemed {
        Ok(true) => {
            let mut response = home("");
            response
                .headers_mut()
                .insert(header::SET_COOKIE, session_cookie(&session));
            response
        }
        _ => home("?signin=expired"),
    }
}

#[derive(Serialize)]
struct Mode {
    local: bool,
}

// Lets the web app hide the sign-in form; says nothing else.
async fn mode() -> Json<Mode> {
    Json(Mode {
        local: local_trust_enabled(),
    })
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/local/mode", get(mode))
        .route("/agent/local/claim", post(claim))
        .route("/agent/local/connect", post(connect))
        .route("/agent/local/login", post(login_link))
        .route("/local/login/{code}", get(redeem))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_loopback_hosts_count_as_local() {
        for host in [
            "127.0.0.1:8080",
            "localhost:8080",
            "localhost",
            "[::1]:8080",
        ] {
            assert!(loopback_host(host), "{host}");
        }
        for host in [
            "evil.test:8080",
            "127.0.0.1.evil.test",
            "localhost.evil.test:80",
            "",
            ":8080",
        ] {
            assert!(!loopback_host(host), "{host}");
        }
    }

    #[test]
    fn browser_requests_are_refused_even_on_loopback() {
        FORCE_LOCAL_TRUST.store(true, std::sync::atomic::Ordering::SeqCst);
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("127.0.0.1:8080"));
        assert!(require_local_caller(&headers).is_ok());
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://evil.test"),
        );
        assert!(require_local_caller(&headers).is_err());
        let mut rebound = HeaderMap::new();
        rebound.insert(header::HOST, HeaderValue::from_static("evil.test:8080"));
        assert!(require_local_caller(&rebound).is_err());
    }

    // F1: the web app's /api proxy rewrites Host to the API's loopback
    // address and passes the caller's (forgeable) Host as X-Forwarded-Host,
    // so a LAN client reaching the proxy looked like a local command.
    #[test]
    fn proxied_requests_are_not_local_commands() {
        FORCE_LOCAL_TRUST.store(true, std::sync::atomic::Ordering::SeqCst);
        for (name, value) in [
            ("x-forwarded-host", "localhost:3000"),
            ("x-forwarded-for", "192.168.1.20"),
            ("forwarded", "for=192.168.1.20"),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(header::HOST, HeaderValue::from_static("127.0.0.1:8080"));
            headers.insert(name, HeaderValue::from_static(value));
            assert_eq!(
                require_local_caller(&headers).map_err(|e| e.1),
                Err("local_command_only"),
                "{name}"
            );
        }
    }

    #[test]
    fn claiming_needs_the_local_key() {
        set_local_key_for_tests("test-local-key");
        let mut headers = HeaderMap::new();
        assert!(require_local_key(&headers).is_err());
        headers.insert(LOCAL_KEY_HEADER, HeaderValue::from_static("guess"));
        assert!(require_local_key(&headers).is_err());
        headers.insert(LOCAL_KEY_HEADER, HeaderValue::from_static("test-local-key"));
        assert!(require_local_key(&headers).is_ok());
    }

    #[test]
    fn device_tokens_have_their_own_prefix() {
        let mut headers = HeaderMap::new();
        let token = format!("{DEVICE_PREFIX}{}", "a".repeat(64));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        assert_eq!(device_token(&headers).unwrap(), token);
        let agent = format!("Bearer aidrive_agent_{}", "a".repeat(64));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&agent).unwrap(),
        );
        assert!(device_token(&headers).is_err());
    }
}
