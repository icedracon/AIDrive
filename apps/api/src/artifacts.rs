// File index (migration 0020): files assistants create outside AIDrive, with
// where they live, why, and until when they are needed. Registering, checking
// and deciding need write access; listing needs read access. AIDrive never
// touches the files themselves: the client checks existence on its machine.
use crate::{
    AppState,
    auth::ApiError,
    files::{Principal, access, db_error, ids},
    workspaces::is_uuid,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Row, Transaction};

const STAMP: &str = "'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'";

fn invalid() -> ApiError {
    ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_artifact")
}

fn text_ok(value: &str, max: usize) -> bool {
    (1..=max).contains(&value.trim().chars().count()) && !value.chars().any(char::is_control)
}

// RFC 3339 only. PostgreSQL alone would also accept words such as
// "tomorrow" or "epoch", which an assistant could send by mistake.
fn rfc3339_ok(value: &str) -> bool {
    let b = value.as_bytes();
    let digits = |r: std::ops::Range<usize>| {
        r.into_iter()
            .all(|i| b.get(i).is_some_and(u8::is_ascii_digit))
    };
    let mut rest = 19;
    if b.len() < 20
        || !(digits(0..4)
            && digits(5..7)
            && digits(8..10)
            && digits(11..13)
            && digits(14..16)
            && digits(17..19))
        || b[4] != b'-'
        || b[7] != b'-'
        || !matches!(b[10], b'T' | b't')
        || b[13] != b':'
        || b[16] != b':'
    {
        return false;
    }
    if b[rest] == b'.' {
        rest += 1;
        let start = rest;
        while b.get(rest).is_some_and(u8::is_ascii_digit) {
            rest += 1;
        }
        if rest == start {
            return false;
        }
    }
    match &b[rest..] {
        [b'Z' | b'z'] => true,
        [b'+' | b'-', h1, h2, b':', m1, m2] => [h1, h2, m1, m2].iter().all(|c| c.is_ascii_digit()),
        _ => false,
    }
}

#[derive(Serialize)]
struct Artifact {
    id: String,
    path: String,
    host: String,
    worktree: Option<String>,
    branch: Option<String>,
    purpose: String,
    keep_until: Option<String>,
    // active | kept | removed | missing
    status: String,
    // Active and past keep_until: the assistant should check and decide.
    due: bool,
    registered_by_email: Option<String>,
    registered_by_agent_id: Option<String>,
    registered_by_agent_name: Option<String>,
    created_at: String,
    updated_at: String,
    last_checked_at: Option<String>,
    last_seen_at: Option<String>,
    closed_at: Option<String>,
    note: Option<String>,
    // file | temp | build | package | artifact (0026)
    kind: String,
    // Measured by the client that registered or checked it.
    size_bytes: Option<i64>,
    // The session that created it, as the client named it.
    session_ref: Option<String>,
    size_checked_at: Option<String>,
}

#[derive(Serialize)]
struct ArtifactList {
    artifacts: Vec<Artifact>,
}

async fn load(
    tx: &mut Transaction<'_, Postgres>,
    workspace: &str,
    status: Option<&str>,
    due: bool,
    host: Option<&str>,
    limit: i64,
) -> Result<Vec<Artifact>, ApiError> {
    let ts = |c: &str| format!("to_char({c} AT TIME ZONE 'UTC', {STAMP})");
    let rows = sqlx::query(&format!(
        "SELECT id::text, path, host, worktree, branch, purpose, {}, status, due, actor_email, agent_id::text, agent_name, \
         {}, {}, {}, {}, {}, note, kind, size_bytes, session_ref, {} FROM app.workspace_artifacts($1::uuid, $2, $3, $4, $5::int)",
        ts("keep_until"),
        ts("created_at"),
        ts("updated_at"),
        ts("last_checked_at"),
        ts("last_seen_at"),
        ts("closed_at"),
        ts("size_checked_at"),
    ))
    .bind(workspace)
    .bind(status)
    .bind(due)
    .bind(host)
    .bind(limit)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_error)?;
    rows.into_iter()
        .map(|r| {
            Ok(Artifact {
                id: r.try_get(0)?,
                path: r.try_get(1)?,
                host: r.try_get(2)?,
                worktree: r.try_get(3)?,
                branch: r.try_get(4)?,
                purpose: r.try_get(5)?,
                keep_until: r.try_get(6)?,
                status: r.try_get(7)?,
                due: r.try_get(8)?,
                registered_by_email: r.try_get(9)?,
                registered_by_agent_id: r.try_get(10)?,
                registered_by_agent_name: r.try_get(11)?,
                created_at: r.try_get(12)?,
                updated_at: r.try_get(13)?,
                last_checked_at: r.try_get(14)?,
                last_seen_at: r.try_get(15)?,
                closed_at: r.try_get(16)?,
                note: r.try_get(17)?,
                kind: r.try_get(18)?,
                size_bytes: r.try_get(19)?,
                session_ref: r.try_get(20)?,
                size_checked_at: r.try_get(21)?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(db_error)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    // all (default) | open | active | kept | removed | missing
    status: Option<String>,
    due: Option<bool>,
    host: Option<String>,
    limit: Option<i64>,
}

async fn list(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Json<ArtifactList>, ApiError> {
    if q.status.as_deref().is_some_and(|s| {
        !matches!(
            s,
            "all" | "open" | "active" | "kept" | "removed" | "missing"
        )
    }) || q.host.as_deref().is_some_and(|h| h.chars().count() > 200)
    {
        return Err(invalid());
    }
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let artifacts = load(
        &mut tx,
        &workspace,
        q.status.as_deref(),
        q.due.unwrap_or(false),
        q.host.as_deref(),
        q.limit.unwrap_or(100).clamp(1, 500),
    )
    .await?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(ArtifactList { artifacts }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewArtifact {
    path: String,
    #[serde(default)]
    host: String,
    worktree: Option<String>,
    branch: Option<String>,
    purpose: String,
    // RFC 3339 time, or keep_days from now; neither = until someone decides.
    keep_until: Option<String>,
    keep_days: Option<i32>,
    // file (default) | temp | build | package | artifact
    kind: Option<String>,
    size_bytes: Option<i64>,
    session: Option<String>,
    // An automatic re-index: a known open path only updates its size.
    #[serde(default)]
    refresh: bool,
}

const KINDS: [&str; 5] = ["file", "temp", "build", "package", "artifact"];

fn db_refusal(error: sqlx::Error) -> ApiError {
    match error.as_database_error().and_then(|e| e.code()).as_deref() {
        Some("23514" | "22007" | "22008") => invalid(),
        _ => db_error(error),
    }
}

async fn register(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
    Json(body): Json<NewArtifact>,
) -> Result<(StatusCode, Json<Artifact>), ApiError> {
    if !text_ok(&body.path, 1000)
        || body.host.chars().count() > 200
        || body.host.chars().any(char::is_control)
        || body.worktree.as_deref().is_some_and(|w| !text_ok(w, 1000))
        || body.branch.as_deref().is_some_and(|b| !text_ok(b, 200))
        || !text_ok(&body.purpose, 1000)
        || (body.keep_until.is_some() && body.keep_days.is_some())
        || body.keep_days.is_some_and(|d| !(1..=400).contains(&d))
        || body.keep_until.as_deref().is_some_and(|k| !rfc3339_ok(k))
        || body.kind.as_deref().is_some_and(|k| !KINDS.contains(&k))
        || body.size_bytes.is_some_and(|b| b < 0)
        || body.session.as_deref().is_some_and(|x| !text_ok(x, 200))
    {
        return Err(invalid());
    }
    let mut tx = access(&state, &principal, &workspace, true).await?;
    let id: String = sqlx::query_scalar(
        "SELECT app.register_artifact($1::uuid, $2, $3, $4, $5, $6, \
         coalesce($7::timestamptz, clock_timestamp() + make_interval(days => $8::int)), \
         coalesce($9, 'file'), $10, $11, $12)::text",
    )
    .bind(&workspace)
    .bind(body.path.trim())
    .bind(body.host.trim())
    .bind(&body.worktree)
    .bind(&body.branch)
    .bind(body.purpose.trim())
    .bind(&body.keep_until)
    .bind(body.keep_days)
    .bind(&body.kind)
    .bind(body.size_bytes)
    .bind(&body.session)
    .bind(body.refresh)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_refusal)?;
    let found = load(
        &mut tx,
        &workspace,
        Some("all"),
        false,
        Some(body.host.trim()),
        500,
    )
    .await?
    .into_iter()
    .find(|a| a.id == id)
    .ok_or(ApiError(StatusCode::NOT_FOUND, "not_found"))?;
    tx.commit().await.map_err(db_error)?;
    Ok((StatusCode::CREATED, Json(found)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    // keep | extend | removed
    action: String,
    keep_until: Option<String>,
    keep_days: Option<i32>,
    note: Option<String>,
}

#[derive(Serialize)]
struct Resolved {
    status: String,
}

async fn resolve(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, artifact)): Path<(String, String)>,
    Json(body): Json<Decision>,
) -> Result<Json<Resolved>, ApiError> {
    ids(&[&workspace, &artifact])?;
    if !matches!(body.action.as_str(), "keep" | "extend" | "removed")
        || (body.keep_until.is_some() && body.keep_days.is_some())
        || body.keep_days.is_some_and(|d| !(1..=400).contains(&d))
        || body.keep_until.as_deref().is_some_and(|k| !rfc3339_ok(k))
        || body
            .note
            .as_deref()
            .is_some_and(|n| n.chars().count() > 1000)
    {
        return Err(invalid());
    }
    let mut tx = access(&state, &principal, &workspace, true).await?;
    let status: String = sqlx::query_scalar(
        "SELECT app.resolve_artifact($1::uuid, $2::uuid, $3, \
         coalesce($4::timestamptz, clock_timestamp() + make_interval(days => $5::int)), $6)",
    )
    .bind(&workspace)
    .bind(&artifact)
    .bind(&body.action)
    .bind(&body.keep_until)
    .bind(body.keep_days)
    .bind(&body.note)
    .fetch_one(&mut *tx)
    .await
    .map_err(
        |e| match e.as_database_error().and_then(|d| d.code()).as_deref() {
            Some("P0002") => ApiError(StatusCode::NOT_FOUND, "not_found"),
            _ => db_refusal(e),
        },
    )?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(Resolved { status }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Check {
    id: String,
    exists: bool,
    size_bytes: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Checks {
    checks: Vec<Check>,
}

#[derive(Serialize)]
struct Checked {
    updated: i32,
}

async fn check(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
    Json(body): Json<Checks>,
) -> Result<Json<Checked>, ApiError> {
    if body.checks.is_empty()
        || body.checks.len() > 100
        || body
            .checks
            .iter()
            .any(|c| !is_uuid(&c.id) || c.size_bytes.is_some_and(|b| b < 0))
    {
        return Err(invalid());
    }
    let checks: Vec<serde_json::Value> = body
        .checks
        .iter()
        .map(|c| serde_json::json!({"id": c.id, "exists": c.exists, "size_bytes": c.size_bytes}))
        .collect();
    let mut tx = access(&state, &principal, &workspace, true).await?;
    let updated: i32 = sqlx::query_scalar("SELECT app.record_artifact_checks($1::uuid, $2::jsonb)")
        .bind(&workspace)
        .bind(serde_json::Value::Array(checks).to_string())
        .fetch_one(&mut *tx)
        .await
        .map_err(db_refusal)?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(Checked { updated }))
}

pub fn routes() -> Router<AppState> {
    let shared = |prefix: &str| {
        Router::new()
            .route(&format!("{prefix}/artifacts"), get(list).post(register))
            .route(&format!("{prefix}/artifacts/check"), post(check))
            .route(
                &format!("{prefix}/artifacts/{{artifact}}/resolve"),
                post(resolve),
            )
    };
    shared("/workspaces/{id}").merge(shared("/agent/workspaces/{id}"))
}

#[cfg(test)]
mod rfc3339_tests {
    use super::rfc3339_ok;

    #[test]
    fn only_rfc3339_times_are_accepted() {
        for ok in [
            "2026-11-27T10:00:00Z",
            "2026-11-27t10:00:00.123z",
            "2026-11-27T10:00:00+04:00",
        ] {
            assert!(rfc3339_ok(ok), "{ok}");
        }
        for bad in [
            "tomorrow",
            "epoch",
            "2026-11-27",
            "2026-11-27 10:00:00Z",
            "2026-11-27T10:00:00",
            "2026-11-27T10:00:00.Z",
            "2026-11-27T10:00:00+0400",
        ] {
            assert!(!rfc3339_ok(bad), "{bad}");
        }
    }
}
