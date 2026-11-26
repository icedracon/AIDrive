use crate::AppState;
use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version,
    password_hash::{SaltString, rand_core::OsRng, rand_core::RngCore},
};
use axum::{
    Json, Router,
    extract::{FromRequestParts, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::sync::LazyLock;

pub const COOKIE: &str = "__Host-aidrive_session";
const MIN_PASSWORD_CHARS: usize = 15;
const MAX_PASSWORD_CHARS: usize = 256;
const SESSION_MAX_AGE_SECS: u64 = 7 * 24 * 60 * 60;
const BLOCKLIST: &[&str] = &[
    "passwordpassword",
    "123456789012345",
    "qwertyuiopasdfg",
    "aaaaaaaaaaaaaaa",
    "iloveyouiloveyou",
    "letmeinletmein1",
];

fn argon2() -> Argon2<'static> {
    Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(19 * 1024, 2, 1, None).expect("static OWASP parameters are valid"),
    )
}

// Verified against when the email is unknown so both paths cost one Argon2id run.
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| {
    argon2()
        .hash_password(
            b"aidrive-timing-equaliser",
            &SaltString::generate(&mut OsRng),
        )
        .expect("hashing a constant succeeds")
        .to_string()
});

#[derive(Debug, PartialEq, Eq)]
pub enum PasswordProblem {
    TooShort,
    TooLong,
    Blocklisted,
}

pub fn check_password(email: &str, password: &str) -> Result<(), PasswordProblem> {
    let length = password.chars().count();
    if length < MIN_PASSWORD_CHARS {
        return Err(PasswordProblem::TooShort);
    }
    if length > MAX_PASSWORD_CHARS {
        return Err(PasswordProblem::TooLong);
    }
    let lowered = password.to_lowercase();
    let email = email.trim().to_lowercase();
    if BLOCKLIST.contains(&lowered.as_str()) || (!email.is_empty() && lowered.contains(&email)) {
        return Err(PasswordProblem::Blocklisted);
    }
    Ok(())
}

pub fn new_token() -> (String, Vec<u8>) {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let token = hex(&bytes);
    let hash = token_hash(&token);
    (token, hash)
}

pub fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn session_cookie(token: &str) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{COOKIE}={token}; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age={SESSION_MAX_AGE_SECS}"
    ))
    .expect("hex token is a valid header value")
}

fn clear_cookie() -> HeaderValue {
    HeaderValue::from_static(
        "__Host-aidrive_session=; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age=0",
    )
}

pub fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, value)| {
            *name == COOKIE && value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
        })
        .map(|(_, value)| value.to_owned())
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
}

#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub &'static str);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(ErrorBody { error: self.1 })).into_response()
    }
}

pub fn unavailable(error: impl std::fmt::Display) -> ApiError {
    tracing::warn!(%error, "auth: database operation failed");
    ApiError(StatusCode::SERVICE_UNAVAILABLE, "unavailable")
}

pub fn pool(state: &AppState) -> Result<&PgPool, ApiError> {
    state
        .db
        .as_ref()
        .ok_or(ApiError(StatusCode::SERVICE_UNAVAILABLE, "unavailable"))
}

// CSRF (ADR 0002): state-changing requests must be same-origin by Fetch
// Metadata, or by an exact Origin match when that header is absent.
pub async fn require_same_origin(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    // /agent/* accepts only an explicit Authorization header, which browsers
    // never attach on their own, so there is no ambient credential to forge.
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) || request.uri().path().starts_with("/agent/")
    {
        return next.run(request).await;
    }
    let headers = request.headers();
    let allowed = match headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        Some(site) => site == "same-origin",
        None => headers
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|origin| origin == state.web_origin),
    };
    if allowed {
        next.run(request).await
    } else {
        ApiError(StatusCode::FORBIDDEN, "cross_origin_request").into_response()
    }
}

// IDs stay text end to end; the API never inspects them, and sqlx's uuid
// feature would add a dependency for no behavioural gain.
pub struct CurrentUser {
    pub id: String,
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let token = cookie_token(&parts.headers)
            .ok_or(ApiError(StatusCode::UNAUTHORIZED, "unauthenticated"))?;
        let id: Option<String> = sqlx::query_scalar("SELECT app.resolve_session($1)::text")
            .bind(token_hash(&token))
            .fetch_one(pool(state)?)
            .await
            .map_err(unavailable)?;
        id.map(|id| CurrentUser { id })
            .ok_or(ApiError(StatusCode::UNAUTHORIZED, "unauthenticated"))
    }
}

#[derive(Deserialize)]
struct Credentials {
    email: String,
    password: String,
}

#[derive(Serialize)]
struct SessionUser {
    id: String,
    email: String,
}

#[derive(Serialize)]
struct SessionBody {
    user: SessionUser,
}

async fn hash_password(password: String) -> Result<String, ApiError> {
    tokio::task::spawn_blocking(move || {
        argon2()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|hash| hash.to_string())
    })
    .await
    .map_err(unavailable)?
    .map_err(unavailable)
}

async fn verify_password(password: String, stored: Option<String>) -> bool {
    tokio::task::spawn_blocking(move || {
        let candidate = stored.as_deref().unwrap_or(DUMMY_HASH.as_str());
        let ok = PasswordHash::new(candidate)
            .is_ok_and(|hash| argon2().verify_password(password.as_bytes(), &hash).is_ok());
        ok && stored.is_some()
    })
    .await
    .unwrap_or(false)
}

async fn start_session(db: &PgPool, user: &str) -> Result<HeaderValue, ApiError> {
    let (token, hash) = new_token();
    sqlx::query("SELECT app.create_session($1::uuid, $2)")
        .bind(user)
        .bind(hash)
        .execute(db)
        .await
        .map_err(unavailable)?;
    Ok(session_cookie(&token))
}

async fn load_user(db: &PgPool, user: &str) -> Result<SessionUser, ApiError> {
    let mut tx = db.begin().await.map_err(unavailable)?;
    sqlx::query("SELECT set_config('app.user_id', $1, true)")
        .bind(user)
        .execute(&mut *tx)
        .await
        .map_err(unavailable)?;
    let (id, email): (String, String) =
        sqlx::query_as("SELECT id::text, email FROM users WHERE id = $1::uuid")
            .bind(user)
            .fetch_optional(&mut *tx)
            .await
            .map_err(unavailable)?
            .ok_or(ApiError(StatusCode::UNAUTHORIZED, "unauthenticated"))?;
    tx.commit().await.map_err(unavailable)?;
    Ok(SessionUser { id, email })
}

async fn register(
    State(state): State<AppState>,
    Json(body): Json<Credentials>,
) -> Result<Response, ApiError> {
    if !state.allow_signup {
        return Err(ApiError(StatusCode::FORBIDDEN, "signup_disabled"));
    }
    match check_password(&body.email, &body.password) {
        Err(PasswordProblem::TooShort) => {
            return Err(ApiError(
                StatusCode::UNPROCESSABLE_ENTITY,
                "password_too_short",
            ));
        }
        Err(PasswordProblem::TooLong) => {
            return Err(ApiError(
                StatusCode::UNPROCESSABLE_ENTITY,
                "password_too_long",
            ));
        }
        Err(PasswordProblem::Blocklisted) => {
            return Err(ApiError(
                StatusCode::UNPROCESSABLE_ENTITY,
                "password_blocklisted",
            ));
        }
        Ok(()) => {}
    }
    let db = pool(&state)?;
    let hash = hash_password(body.password).await?;
    let user: String = sqlx::query_scalar("SELECT app.register_user($1, $2)::text")
        .bind(&body.email)
        .bind(hash)
        .fetch_one(db)
        .await
        .map_err(
            |error| match error.as_database_error().and_then(|e| e.code()) {
                Some(code) if code == "23505" => ApiError(StatusCode::CONFLICT, "email_taken"),
                Some(code) if code == "23514" => {
                    ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_email")
                }
                _ => unavailable(error),
            },
        )?;
    let cookie = start_session(db, &user).await?;
    let user = load_user(db, &user).await?;
    Ok((
        StatusCode::CREATED,
        [(header::SET_COOKIE, cookie)],
        Json(SessionBody { user }),
    )
        .into_response())
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<Credentials>,
) -> Result<Response, ApiError> {
    if body.password.chars().count() > MAX_PASSWORD_CHARS {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid_credentials"));
    }
    let db = pool(&state)?;
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT user_id::text, password_hash FROM app.login_lookup($1)")
            .bind(&body.email)
            .fetch_optional(db)
            .await
            .map_err(unavailable)?;
    let (user, stored) = match row {
        Some((user, hash)) => (Some(user), Some(hash)),
        None => (None, None),
    };
    let password = body.password;
    if !verify_password(password, stored).await {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid_credentials"));
    }
    let user = user.expect("verified implies a stored hash");
    let cookie = start_session(db, &user).await?;
    let user = load_user(db, &user).await?;
    Ok(([(header::SET_COOKIE, cookie)], Json(SessionBody { user })).into_response())
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    if let Some(token) = cookie_token(&headers) {
        sqlx::query("SELECT app.revoke_session($1)")
            .bind(token_hash(&token))
            .execute(pool(&state)?)
            .await
            .map_err(unavailable)?;
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, clear_cookie())],
    )
        .into_response())
}

async fn session(
    State(state): State<AppState>,
    current: CurrentUser,
) -> Result<Json<SessionBody>, ApiError> {
    let user = load_user(pool(&state)?, &current.id).await?;
    Ok(Json(SessionBody { user }))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/session", get(session))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_rules_follow_nist_length_and_blocklist() {
        assert_eq!(
            check_password("a@x.test", "short"),
            Err(PasswordProblem::TooShort)
        );
        assert_eq!(
            check_password("a@x.test", &"x".repeat(257)),
            Err(PasswordProblem::TooLong)
        );
        assert_eq!(
            check_password("a@x.test", "PasswordPassword"),
            Err(PasswordProblem::Blocklisted)
        );
        assert_eq!(
            check_password("owner@x.test", "my owner@x.test pass"),
            Err(PasswordProblem::Blocklisted)
        );
        assert_eq!(check_password("a@x.test", "correct horse battery"), Ok(()));
        assert_eq!(check_password("a@x.test", "ünïcödé-ünïcödé"), Ok(()));
    }

    #[test]
    fn tokens_are_256_bit_hex_and_only_hash_is_stored() {
        let (token, hash) = new_token();
        assert_eq!(token.len(), 64);
        assert_eq!(hash, token_hash(&token));
        assert_eq!(hash.len(), 32);
        assert_ne!(new_token().0, token);
    }

    #[test]
    fn cookie_attributes_match_adr() {
        let cookie = session_cookie(&"a".repeat(64));
        let cookie = cookie.to_str().unwrap();
        for part in [
            "__Host-aidrive_session=",
            "Path=/",
            "Secure",
            "HttpOnly",
            "SameSite=Strict",
        ] {
            assert!(cookie.contains(part), "{cookie}");
        }
        assert!(!cookie.contains("Domain"));
    }

    #[test]
    fn cookie_parser_rejects_malformed_tokens() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("other=1; __Host-aidrive_session=nothex"),
        );
        assert_eq!(cookie_token(&headers), None);
        let good = "b".repeat(64);
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("x=y; __Host-aidrive_session={good}")).unwrap(),
        );
        assert_eq!(cookie_token(&headers), Some(good));
    }

    #[test]
    fn argon2_parameters_meet_owasp_minimum() {
        let hash = argon2()
            .hash_password(b"correct horse battery", &SaltString::generate(&mut OsRng))
            .unwrap()
            .to_string();
        assert!(
            hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
            "{hash}"
        );
    }
}
