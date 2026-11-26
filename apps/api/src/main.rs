mod activity;
mod agents;
mod artifacts;
mod auth;
#[cfg(feature = "embedded-db")]
mod cli;
#[cfg(feature = "embedded-db")]
mod detach;
#[cfg(feature = "embedded-db")]
mod embedded;
mod files;
mod handoffs;
mod knowledge;
mod local;
mod messages;
mod pairing;
mod precheck;
mod review;
mod search;
mod sessions;
mod web;
mod workspaces;

use axum::{Json, Router, extract::State, http::StatusCode, middleware, routing::get};
use serde::Serialize;
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{env, net::SocketAddr, str::FromStr, time::Duration};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

const READINESS_BUDGET: Duration = Duration::from_secs(3);

#[derive(Clone)]
struct AppState {
    db: Option<PgPool>,
    web_origin: String,
    allow_signup: bool,
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    service: &'static str,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Check {
    Ok,
    Failed,
    NotConfigured,
    Skipped,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
struct Checks {
    database: Check,
    role: Check,
    schema: Check,
}

impl Checks {
    fn ready(&self) -> bool {
        [self.database, self.role, self.schema]
            .iter()
            .all(|check| *check == Check::Ok)
    }
}

#[derive(Serialize)]
struct Readiness {
    status: &'static str,
    service: &'static str,
    checks: Checks,
}

// Catalog-only probe: aidrive_app cannot read the migration ledger, and
// serving as a superuser, BYPASSRLS role or table owner would silently
// disable row-level security.
const PROBE_SQL: &str = r#"
SELECT
    r.rolname <> 'aidrive_app' OR r.rolsuper OR r.rolbypassrls
        OR r.rolcreaterole OR r.rolcreatedb
        OR pg_has_role(current_user, 'aidrive_owner', 'MEMBER')
        OR EXISTS (SELECT 1 FROM pg_catalog.pg_tables t
                   WHERE t.schemaname = 'public' AND t.tableowner = current_user) AS privileged,
    to_regprocedure('app.has_role(uuid,text)') IS NOT NULL
        AND to_regprocedure('app.create_workspace(text)') IS NOT NULL
        AND to_regprocedure('app.resolve_session(bytea)') IS NOT NULL
        AND to_regprocedure('app.my_workspaces()') IS NOT NULL
        AND to_regprocedure('app.read_agent_workspace(uuid,bytea)') IS NOT NULL
        AND to_regprocedure('app.begin_agent_request(uuid,bytea,text)') IS NOT NULL
        AND to_regprocedure('app.decide_handoff(uuid,uuid,text,text)') IS NOT NULL
        AND to_regprocedure('app.workspace_activity(uuid,timestamptz,integer)') IS NOT NULL
        AND to_regprocedure('app.decide_pairing(text,uuid,text,text)') IS NOT NULL
        AND to_regprocedure('app.add_session_events(uuid,uuid,jsonb,boolean)') IS NOT NULL
        AND to_regprocedure('app.decide_item(uuid,uuid,text,text,text,text)') IS NOT NULL
        AND to_regprocedure('app.retire_item(uuid,uuid,text)') IS NOT NULL
        AND to_regprocedure('app.undo_item(uuid,uuid,text)') IS NOT NULL
        AND to_regprocedure('app.open_local_session(bytea)') IS NOT NULL
        AND to_regprocedure('app.review_metrics(uuid,integer)') IS NOT NULL
        AND to_regprocedure('app.set_session_visibility(uuid,uuid,text)') IS NOT NULL
        AND to_regprocedure('app.send_message(uuid,uuid,text,text,text,uuid,jsonb)') IS NOT NULL
        AND to_regprocedure('app.register_artifact(uuid,text,text,text,text,text,timestamptz,text,bigint,text,boolean)') IS NOT NULL
        AND to_regprocedure('app.require_role(uuid,integer,uuid)') IS NOT NULL
        AND to_regprocedure('app.write_text_file(uuid,uuid,text,uuid,uuid,text,text,uuid)') IS NOT NULL
        -- Search folds case through the database ctype; a plain C locale leaves
        -- Armenian capitals unfolded and silently breaks Armenian search.
        AND lower('Բ') = 'բ'
        AND to_regclass('public.session_event_chunks') IS NOT NULL
        AND EXISTS (SELECT 1 FROM pg_catalog.pg_attribute a
                    WHERE a.attrelid = to_regclass('public.session_events')
                      AND a.attname = 'capture_version' AND NOT a.attisdropped)
        AND EXISTS (SELECT 1 FROM pg_catalog.pg_trigger g
                    WHERE g.tgname = 'files_protect_provenance'
                      AND g.tgrelid = to_regclass('public.files')) AS schema_present
FROM pg_catalog.pg_roles r
WHERE r.rolname = current_user
"#;

async fn probe(pool: &PgPool) -> Checks {
    let failed = Checks {
        database: Check::Failed,
        role: Check::Skipped,
        schema: Check::Skipped,
    };
    let result = tokio::time::timeout(
        READINESS_BUDGET,
        sqlx::query_as::<_, (bool, bool)>(PROBE_SQL).fetch_one(pool),
    )
    .await;
    match result {
        Ok(Ok((privileged, schema_present))) => Checks {
            database: Check::Ok,
            role: if privileged { Check::Failed } else { Check::Ok },
            schema: if schema_present {
                Check::Ok
            } else {
                Check::Failed
            },
        },
        Ok(Err(error)) => {
            tracing::warn!(
                kind = error_kind(&error),
                "readiness: database probe failed"
            );
            failed
        }
        Err(_) => {
            tracing::warn!("readiness: database probe timed out");
            failed
        }
    }
}

// Error kinds only: driver messages can echo connection details.
fn error_kind(error: &sqlx::Error) -> &'static str {
    match error {
        sqlx::Error::PoolTimedOut => "pool_timeout",
        sqlx::Error::Io(_) => "io",
        sqlx::Error::Tls(_) => "tls",
        sqlx::Error::Database(_) => "database",
        sqlx::Error::Protocol(_) => "protocol",
        _ => "other",
    }
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        service: "aidrive-api",
    })
}

async fn ready(State(state): State<AppState>) -> (StatusCode, Json<Readiness>) {
    let checks = match &state.db {
        Some(pool) => probe(pool).await,
        None => Checks {
            database: Check::NotConfigured,
            role: Check::Skipped,
            schema: Check::Skipped,
        },
    };
    let ready = checks.ready();
    let status = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(Readiness {
            status: if ready { "ready" } else { "not_ready" },
            service: "aidrive-api",
            checks,
        }),
    )
}

fn app(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .merge(auth::routes())
        .merge(workspaces::routes())
        .merge(agents::routes())
        .merge(files::routes())
        .merge(handoffs::routes())
        .merge(activity::routes())
        .merge(pairing::routes())
        .merge(sessions::routes())
        .merge(knowledge::routes())
        .merge(precheck::routes())
        .merge(local::routes())
        .merge(messages::routes())
        .merge(artifacts::routes())
        .merge(review::routes())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_same_origin,
        ))
        .with_state(state)
}

fn pool(url: &str, acquire_timeout: Duration) -> Result<PgPool, &'static str> {
    let options = PgConnectOptions::from_str(url)
        .map_err(|_| "DATABASE_URL is not a valid PostgreSQL URL")?;
    Ok(PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(acquire_timeout)
        .connect_lazy_with(options))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "embedded-db")]
    if let Some(command) = env::args()
        .nth(1)
        .filter(|a| cli::COMMANDS.contains(&a.as_str()))
    {
        let rest: Vec<String> = env::args().skip(2).collect();
        if let Err(error) = cli::run(&command, &rest) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    // `aidrive up` starts the API with no console or inherited handles, so it
    // writes its own log; a start error would otherwise be lost.
    match env::var_os("AIDRIVE_LOG_FILE") {
        Some(path) => {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(file))
                .init();
        }
        None => tracing_subscriber::fmt().with_env_filter(filter).init(),
    }
    let result = serve().await;
    if let Err(error) = &result {
        tracing::error!("AIDrive API stopped: {error}");
    }
    result
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let address: SocketAddr = env::var("AIDRIVE_API_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_owned())
        .parse()?;
    let embedded_requested = env::args().skip(1).any(|a| a == "--embedded-db")
        || env::var("AIDRIVE_EMBEDDED_DB").is_ok_and(|v| v == "1");
    if let Some(other) = env::args()
        .skip(1)
        .enumerate()
        .find(|(i, a)| a != "--embedded-db" && !(*i == 0 && a == "serve"))
        .map(|(_, a)| a)
    {
        return Err(format!("unknown argument {other}; usage: aidrive-api [serve] [--embedded-db] | up | down | status").into());
    }
    #[cfg(feature = "embedded-db")]
    let mut embedded = None;
    let db = if embedded_requested {
        if env::var("DATABASE_URL").is_ok_and(|u| !u.is_empty()) {
            return Err("use either DATABASE_URL or --embedded-db, not both".into());
        }
        if !address.ip().is_loopback() {
            return Err("--embedded-db serves only this computer; AIDRIVE_API_ADDR must be a loopback address".into());
        }
        #[cfg(feature = "embedded-db")]
        {
            let server = embedded::start().await?;
            let options = server.app.clone();
            embedded = Some(server);
            Some(
                PgPoolOptions::new()
                    .max_connections(10)
                    .acquire_timeout(Duration::from_secs(2))
                    .connect_lazy_with(options),
            )
        }
        #[cfg(not(feature = "embedded-db"))]
        return Err("this build has no embedded database; rebuild with the embedded-db feature or set DATABASE_URL".into());
    } else {
        match env::var("DATABASE_URL") {
            Ok(url) if !url.is_empty() => Some(pool(&url, Duration::from_secs(2))?),
            _ => {
                tracing::warn!(
                    "DATABASE_URL not set; /readyz will report not_ready (or start with --embedded-db)"
                );
                None
            }
        }
    };
    let web_origin =
        env::var("AIDRIVE_WEB_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".to_owned());
    let allow_signup = env::var("AIDRIVE_ALLOW_SIGNUP").is_ok_and(|v| v == "1");
    let listener = TcpListener::bind(address).await?;
    tracing::info!(%address, database = db.is_some(), %web_origin, allow_signup, "AIDrive API listening");
    let state = AppState {
        db,
        web_origin,
        allow_signup,
    };
    let router = match web::dir() {
        Some(dir) => {
            tracing::info!(web = %dir.display(), "serving the web app");
            web::with_front(app(state), dir)
        }
        None => app(state),
    };
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown())
        .await?;
    #[cfg(feature = "embedded-db")]
    if let Some(server) = embedded {
        server.stop().await;
    }
    Ok(())
}

// Ctrl+C everywhere, and SIGTERM on Unix (systemd, kill), so the embedded
// PostgreSQL is stopped with the API.
async fn shutdown() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    include!("agent_tests.rs");
    include!("file_tests.rs");
    include!("agent_file_tests.rs");
    include!("handoff_tests.rs");
    include!("activity_tests.rs");
    include!("pairing_tests.rs");
    include!("session_tests.rs");
    include!("knowledge_tests.rs");
    include!("local_tests.rs");
    include!("eval_tests.rs");
    include!("messages_tests.rs");
    include!("artifacts_tests.rs");
    include!("roles_tests.rs");
    include!("review_tests.rs");
    include!("summary_tests.rs");
    use super::{AppState, app, pool};
    use sqlx::PgPool;

    fn state(db: Option<PgPool>) -> AppState {
        AppState {
            db,
            web_origin: "http://localhost:3000".into(),
            allow_signup: false,
        }
    }
    use axum::{body::Body, http::Request, http::StatusCode};
    use std::time::Duration;
    use tower::ServiceExt;

    async fn get(state: AppState, uri: &str) -> (StatusCode, String) {
        let response = app(state)
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn health_endpoint_returns_service_status() {
        let (status, body) = get(state(None), "/healthz").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, r#"{"status":"ok","service":"aidrive-api"}"#);
    }

    #[tokio::test]
    async fn readiness_fails_without_database_configuration() {
        let (status, body) = get(state(None), "/readyz").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            body,
            r#"{"status":"not_ready","service":"aidrive-api","checks":{"database":"not_configured","role":"skipped","schema":"skipped"}}"#
        );
    }

    #[tokio::test]
    async fn readiness_fails_when_database_is_unreachable_without_leaking_url() {
        let url = "postgres://aidrive_app:hunter2-secret@127.0.0.1:1/aidrive";
        let db = Some(pool(url, Duration::from_millis(300)).unwrap());
        let (status, body) = get(state(db.clone()), "/readyz").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(body.contains(r#""database":"failed""#), "{body}");
        assert!(
            !body.contains("hunter2") && !body.contains("127.0.0.1"),
            "{body}"
        );
        let (status, _) = get(state(db), "/healthz").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "liveness must not depend on the database"
        );
    }

    #[test]
    fn malformed_database_url_is_rejected_without_echoing_it() {
        let error = pool("not a url secret-token", Duration::from_secs(1)).unwrap_err();
        assert!(!error.contains("secret-token"));
    }

    async fn send(state: AppState, request: Request<Body>) -> (StatusCode, String) {
        let response = app(state).oneshot(request).await.unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    fn post(uri: &str, headers: &[(&str, &str)]) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json");
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder
            .body(Body::from(
                r#"{"email":"a@x.test","password":"correct horse battery"}"#,
            ))
            .unwrap()
    }

    #[tokio::test]
    async fn cross_site_and_headerless_posts_are_rejected() {
        for headers in [
            &[("sec-fetch-site", "cross-site")][..],
            &[("sec-fetch-site", "same-site")][..],
            &[("origin", "http://evil.test")][..],
            &[][..],
        ] {
            let (status, body) = send(state(None), post("/auth/login", headers)).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{headers:?}");
            assert_eq!(body, r#"{"error":"cross_origin_request"}"#);
        }
    }

    #[tokio::test]
    async fn same_origin_posts_reach_handlers() {
        for headers in [
            &[("sec-fetch-site", "same-origin")][..],
            &[("origin", "http://localhost:3000")][..],
        ] {
            let (status, body) = send(state(None), post("/auth/register", headers)).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{headers:?}");
            assert_eq!(body, r#"{"error":"signup_disabled"}"#);
        }
    }

    #[tokio::test]
    async fn session_requires_a_cookie() {
        let (status, body) = get(state(None), "/auth/session").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, r#"{"error":"unauthenticated"}"#);
    }

    async fn call(
        state: &AppState,
        method: &str,
        uri: &str,
        cookie: Option<&str>,
        body: Option<&str>,
    ) -> (StatusCode, String, Option<String>) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("sec-fetch-site", "same-origin")
            .header("content-type", "application/json");
        if let Some(cookie) = cookie {
            builder = builder.header("cookie", cookie);
        }
        let request = builder
            .body(
                body.map(|b| Body::from(b.to_owned()))
                    .unwrap_or_else(Body::empty),
            )
            .unwrap();
        let response = app(state.clone()).oneshot(request).await.unwrap();
        let status = response.status();
        let cookie = response
            .headers()
            .get("set-cookie")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(str::to_owned);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap(), cookie)
    }

    fn json_field(body: &str, key: &str) -> String {
        let marker = format!("\"{key}\":\"");
        let start = body.find(&marker).expect(body) + marker.len();
        body[start..start + body[start..].find('"').unwrap()].to_owned()
    }

    // Driven by scripts/api-check.sh: one pooled connection is reused across
    // users, so leaked transaction context would show up as wrong results.
    #[tokio::test]
    #[ignore = "needs a migrated PostgreSQL target via AIDRIVE_LIVE_TEST_URL"]
    async fn workspace_flow_against_live_database() {
        let url = std::env::var("AIDRIVE_LIVE_TEST_URL").expect("AIDRIVE_LIVE_TEST_URL");
        let db = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        let state = AppState {
            db: Some(db.clone()),
            web_origin: "http://localhost:3000".into(),
            allow_signup: true,
        };
        let creds = |email: &str| {
            format!(r#"{{"email":"{email}","password":"correct horse battery staple"}}"#)
        };
        let (status, _, alice) = call(
            &state,
            "POST",
            "/auth/register",
            None,
            Some(&creds("alice@live.test")),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let alice = alice.unwrap();
        let (status, _, bob) = call(
            &state,
            "POST",
            "/auth/register",
            None,
            Some(&creds("bob@live.test")),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let bob = bob.unwrap();

        let (status, body, _) = call(&state, "GET", "/workspaces", Some(&alice), None).await;
        assert_eq!(
            (status, body.as_str()),
            (StatusCode::OK, r#"{"workspaces":[]}"#)
        );
        let (status, body, _) = call(
            &state,
            "POST",
            "/workspaces",
            Some(&alice),
            Some(r#"{"name":"  Alice launch "}"#),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let id = json_field(&body, "id");
        assert_eq!(json_field(&body, "name"), "Alice launch");

        let (status, body, _) = call(&state, "GET", "/workspaces", Some(&alice), None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            body.contains(&id) && body.contains(r#""role":"owner""#),
            "{body}"
        );
        let (status, _, _) = call(
            &state,
            "GET",
            &format!("/workspaces/{id}"),
            Some(&alice),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, body, _) = call(&state, "GET", "/workspaces", Some(&bob), None).await;
        assert_eq!(
            (status, body.as_str()),
            (StatusCode::OK, r#"{"workspaces":[]}"#)
        );
        let (status, body, _) = call(
            &state,
            "GET",
            &format!("/workspaces/{id}"),
            Some(&bob),
            None,
        )
        .await;
        assert_eq!(
            (status, body.as_str()),
            (StatusCode::NOT_FOUND, r#"{"error":"not_found"}"#)
        );
        let (status, _, _) = call(
            &state,
            "GET",
            "/workspaces/00000000-0000-4000-8000-000000000000",
            Some(&alice),
            None,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "unknown id indistinguishable from foreign id"
        );

        let (status, _, _) = call(
            &state,
            "POST",
            "/workspaces",
            Some(&alice),
            Some(r#"{"name":"   "}"#),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        let forged = format!("__Host-aidrive_session={}", "f".repeat(64));
        let (status, _, _) = call(&state, "GET", "/workspaces", Some(&forged), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _, _) = call(&state, "GET", "/workspaces", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let leaked: String = sqlx::query_scalar(
            "SELECT coalesce(current_setting('app.user_id', true), '') || coalesce(current_setting('app.workspace_id', true), '')",
        )
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(
            leaked, "",
            "request context leaked onto the pooled connection"
        );
        println!("PASS workspace flow, cross-user denial, forged cookie, no pooled context leak");
    }

    // Driven by scripts/api-check.sh against a private PostgreSQL instance.
    #[tokio::test]
    #[ignore = "needs a live PostgreSQL target via AIDRIVE_READYZ_TEST_URL"]
    async fn readiness_against_live_database() {
        let url = std::env::var("AIDRIVE_READYZ_TEST_URL").expect("AIDRIVE_READYZ_TEST_URL");
        let expected = std::env::var("AIDRIVE_READYZ_EXPECT").expect("AIDRIVE_READYZ_EXPECT");
        let db = Some(pool(&url, Duration::from_secs(2)).unwrap());
        let (status, body) = get(state(db), "/readyz").await;
        println!("{status} {body}");
        assert!(body.contains(&expected), "expected {expected} in {body}");
        assert_eq!(
            status == StatusCode::OK,
            body.contains(r#""status":"ready""#)
        );
    }
    #[tokio::test]
    #[ignore = "needs a migrated PostgreSQL target via AIDRIVE_LIVE_TEST_URL"]
    async fn full_password_and_logout_against_live_database() {
        let url = std::env::var("AIDRIVE_LIVE_TEST_URL").expect("AIDRIVE_LIVE_TEST_URL");
        let db = pool(&url, Duration::from_secs(2)).unwrap();
        let state = AppState {
            db: Some(db.clone()),
            web_origin: "http://localhost:3000".into(),
            allow_signup: true,
        };
        for (index, character) in ['x', 'ա'].into_iter().enumerate() {
            let password = character.to_string().repeat(256);
            let credentials =
                format!(r#"{{"email":"length-{index}@regression.test","password":"{password}"}}"#);
            let (status, _, _) =
                call(&state, "POST", "/auth/register", None, Some(&credentials)).await;
            assert_eq!(status, StatusCode::CREATED);
            let wrong = format!(
                r#"{{"email":"length-{index}@regression.test","password":"{password}WRONG-SUFFIX"}}"#
            );
            let (status, body, cookie) =
                call(&state, "POST", "/auth/login", None, Some(&wrong)).await;
            assert_eq!(
                status,
                StatusCode::UNAUTHORIZED,
                "overlong incorrect password accepted"
            );
            assert_eq!(body, r#"{"error":"invalid_credentials"}"#);
            assert!(cookie.is_none(), "a rejected password issued a session");
            let (status, _, cookie) =
                call(&state, "POST", "/auth/login", None, Some(&credentials)).await;
            assert_eq!(
                status,
                StatusCode::OK,
                "valid boundary-length password rejected"
            );
            let cookie = cookie.unwrap();
            let (status, _, _) = call(&state, "GET", "/auth/session", Some(&cookie), None).await;
            assert_eq!(status, StatusCode::OK);
            let (status, _, _) = call(&state, "POST", "/auth/logout", Some(&cookie), None).await;
            assert_eq!(status, StatusCode::NO_CONTENT);
            let (status, _, _) = call(&state, "GET", "/auth/session", Some(&cookie), None).await;
            assert_eq!(
                status,
                StatusCode::UNAUTHORIZED,
                "logged-out session still resolves"
            );
            let (status, _, _) = call(&state, "POST", "/auth/logout", Some(&cookie), None).await;
            assert_eq!(
                status,
                StatusCode::NO_CONTENT,
                "retrying logout must acknowledge revocation"
            );
        }
        db.close().await;
        println!(
            "PASS ASCII/Armenian full-password boundary, suffix rejection, logout revocation and retry"
        );
    }
}
