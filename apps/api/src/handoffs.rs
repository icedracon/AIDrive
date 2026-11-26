use crate::{
    AppState,
    auth::{ApiError, unavailable},
    files::{Principal, access, db_error, ids},
    workspaces::is_uuid,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Row as _, Transaction};

const MAX_SUMMARY: usize = 4000;
const MAX_NEXT: usize = 2000;
const MAX_ITEM: usize = 1000;
const MAX_ITEMS: usize = 20;

fn invalid() -> ApiError {
    ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_handoff")
}

fn bounded(text: &str, max: usize) -> bool {
    let n = text.trim().chars().count();
    (1..=max).contains(&n)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRef {
    file_id: String,
    version_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewHandoff {
    request_id: String,
    summary: String,
    #[serde(default)]
    decisions: Vec<String>,
    next_step: String,
    #[serde(default)]
    sources: Vec<SourceRef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    decision: String,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Serialize)]
struct Source {
    file_id: String,
    name: String,
    version_id: String,
    current_version_id: Option<String>,
    stale: bool,
}

#[derive(Serialize)]
pub(crate) struct Handoff {
    id: String,
    status: String,
    summary: String,
    decisions: Vec<String>,
    next_step: String,
    actor_id: String,
    agent_id: Option<String>,
    created_at: String,
    decided_at: Option<String>,
    decision_note: Option<String>,
    // True when any cited file has moved past the cited version.
    stale: bool,
    // Accepted without the owner (approval by exception); `owner_checked`
    // is true once the owner approved it or kept it.
    auto_accepted: bool,
    owner_checked: bool,
    confirmed_at: Option<String>,
    undone_at: Option<String>,
    // The handoff this one replaced as the current one.
    replaced: Option<String>,
    // Proposed handoffs: why this one waits for the owner.
    review_reasons: Vec<&'static str>,
    sources: Vec<Source>,
}

#[derive(Serialize)]
struct HandoffList {
    handoffs: Vec<Handoff>,
}

#[derive(Serialize)]
struct Latest {
    approved: Option<Handoff>,
    pending: Vec<Handoff>,
}

type Row = (
    (
        String,
        String,
        String,
        Vec<String>,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
    ),
    (bool, Option<String>, Option<String>, Option<String>, String),
);
type SourceRow = (String, String, String, String, Option<String>);

const SELECT: &str = "SELECT id::text, status, summary, decisions, next_step, actor_id::text, agent_id::text, \
    to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), \
    to_char(decided_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), decision_note, \
    auto_accepted, to_char(confirmed_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), \
    to_char(undone_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), replaced::text, \
    coalesce((SELECT s.review_mode FROM workspace_settings s WHERE s.workspace_id = handoffs.workspace_id), 'strict') \
    FROM handoffs WHERE workspace_id = $1::uuid";

// `filter` is always a fixed string from this module; the only variable part
// (a single handoff ID) is bound as $2.
async fn load(
    tx: &mut Transaction<'_, Postgres>,
    workspace: &str,
    filter: &str,
    id: Option<&str>,
) -> Result<Vec<Handoff>, ApiError> {
    let rows: Vec<Row> = sqlx::query(&format!("{SELECT} {filter}"))
        .bind(workspace)
        .bind(id)
        .fetch_all(&mut **tx)
        .await
        .map_err(db_error)?
        .into_iter()
        .map(|row| {
            Ok((
                (
                    row.try_get(0)?,
                    row.try_get(1)?,
                    row.try_get(2)?,
                    row.try_get(3)?,
                    row.try_get(4)?,
                    row.try_get(5)?,
                    row.try_get(6)?,
                    row.try_get(7)?,
                    row.try_get(8)?,
                    row.try_get(9)?,
                ),
                (
                    row.try_get(10)?,
                    row.try_get(11)?,
                    row.try_get(12)?,
                    row.try_get(13)?,
                    row.try_get(14)?,
                ),
            ))
        })
        .collect::<Result<_, sqlx::Error>>()
        .map_err(db_error)?;
    let ids: Vec<String> = rows.iter().map(|r| r.0.0.clone()).collect();
    let sources: Vec<SourceRow> = sqlx::query_as(
        "SELECT s.handoff_id::text, s.file_id::text, f.name, s.version_id::text, f.current_version_id::text \
         FROM handoff_sources s JOIN files f ON f.workspace_id = s.workspace_id AND f.id = s.file_id \
         WHERE s.workspace_id = $1::uuid AND s.handoff_id = ANY($2::uuid[]) ORDER BY f.name",
    )
    .bind(workspace)
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_error)?;
    Ok(rows
        .into_iter()
        .map(
            |(
                (
                    id,
                    status,
                    summary,
                    decisions,
                    next_step,
                    actor_id,
                    agent_id,
                    created_at,
                    decided_at,
                    decision_note,
                ),
                (auto_accepted, confirmed_at, undone_at, replaced, mode),
            )| {
                let sources: Vec<Source> = sources
                    .iter()
                    .filter(|s| s.0 == id)
                    .map(|(_, file_id, name, version_id, current)| Source {
                        stale: current.as_deref() != Some(version_id.as_str()),
                        file_id: file_id.clone(),
                        name: name.clone(),
                        version_id: version_id.clone(),
                        current_version_id: current.clone(),
                    })
                    .collect();
                let stale = sources.iter().any(|s| s.stale);
                let review_reasons = if status == "proposed" {
                    crate::review::handoff_reasons(&mode, stale, sources.is_empty())
                } else {
                    Vec::new()
                };
                Handoff {
                    owner_checked: decided_at.is_some()
                        && status != "rejected"
                        && (!auto_accepted || confirmed_at.is_some()),
                    auto_accepted,
                    confirmed_at,
                    undone_at,
                    replaced,
                    review_reasons,
                    stale,
                    id,
                    status,
                    summary,
                    decisions,
                    next_step,
                    actor_id,
                    agent_id,
                    created_at,
                    decided_at,
                    decision_note,
                    sources,
                }
            },
        )
        .collect())
}

const BY_ID: &str = "AND id = $2::uuid";

async fn list(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
) -> Result<Json<HandoffList>, ApiError> {
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let handoffs = load(
        &mut tx,
        &workspace,
        "AND $2::uuid IS NULL ORDER BY created_at DESC, id LIMIT 50",
        None,
    )
    .await?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(HandoffList { handoffs }))
}

// What the next assistant should start from: the approved handoff (flagged
// if its evidence moved on) and anything proposed but not yet approved.
async fn latest(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
) -> Result<Json<Latest>, ApiError> {
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let approved = load(
        &mut tx,
        &workspace,
        "AND $2::uuid IS NULL AND status = 'approved' LIMIT 1",
        None,
    )
    .await?
    .pop();
    let pending = load(
        &mut tx,
        &workspace,
        "AND $2::uuid IS NULL AND status = 'proposed' ORDER BY created_at DESC, id LIMIT 10",
        None,
    )
    .await?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(Latest { approved, pending }))
}

async fn create(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
    Json(body): Json<NewHandoff>,
) -> Result<(StatusCode, Json<Handoff>), ApiError> {
    if !is_uuid(&body.request_id)
        || !bounded(&body.summary, MAX_SUMMARY)
        || !bounded(&body.next_step, MAX_NEXT)
        || body.decisions.len() > MAX_ITEMS
        || body.sources.len() > MAX_ITEMS
        || !body.decisions.iter().all(|d| bounded(d, MAX_ITEM))
    {
        return Err(invalid());
    }
    for source in &body.sources {
        ids(&[&source.file_id, &source.version_id]).map_err(|_| invalid())?;
    }
    let decisions: Vec<String> = body.decisions.iter().map(|d| d.trim().to_owned()).collect();
    // IDs were validated as UUIDs above, so this JSON cannot be malformed.
    let sources = format!(
        "[{}]",
        body.sources
            .iter()
            .map(|s| format!(
                r#"{{"file_id":"{}","version_id":"{}"}}"#,
                s.file_id, s.version_id
            ))
            .collect::<Vec<_>>()
            .join(",")
    );
    let mut tx = access(&state, &principal, &workspace, true).await?;
    let id: String = sqlx::query_scalar(
        "SELECT app.record_handoff($1::uuid, $2::uuid, $3, $4, $5, $6::jsonb)::text",
    )
    .bind(&workspace)
    .bind(&body.request_id)
    .bind(&body.summary)
    .bind(&decisions)
    .bind(&body.next_step)
    .bind(&sources)
    .fetch_one(&mut *tx)
    .await
    .map_err(
        |error| match error.as_database_error().and_then(|e| e.code()).as_deref() {
            Some("23503") => ApiError(StatusCode::UNPROCESSABLE_ENTITY, "unknown_source"),
            Some("23505") => ApiError(StatusCode::UNPROCESSABLE_ENTITY, "duplicate_source"),
            _ => db_error(error),
        },
    )?;
    let handoff = load(&mut tx, &workspace, BY_ID, Some(&id))
        .await?
        .pop()
        .ok_or_else(|| unavailable("handoff vanished"))?;
    tx.commit().await.map_err(db_error)?;
    Ok((StatusCode::CREATED, Json(handoff)))
}

// The owner, or a medium/grand assistant key (the database checks the role).
async fn decide(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, handoff)): Path<(String, String)>,
    Json(body): Json<Decision>,
) -> Result<Json<Handoff>, ApiError> {
    ids(&[&workspace, &handoff])?;
    if !matches!(body.decision.as_str(), "approved" | "rejected")
        || body
            .note
            .as_deref()
            .is_some_and(|n| n.chars().count() > 2000)
    {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_decision",
        ));
    }
    let mut tx = access(&state, &principal, &workspace, true).await?;
    sqlx::query("SELECT app.decide_handoff($1::uuid, $2::uuid, $3, $4)")
        .bind(&workspace)
        .bind(&handoff)
        .bind(&body.decision)
        .bind(&body.note)
        .execute(&mut *tx)
        .await
        .map_err(
            |error| match error.as_database_error().and_then(|e| e.code()).as_deref() {
                Some("AD412") => ApiError(StatusCode::CONFLICT, "stale_sources"),
                Some("AD409") => ApiError(StatusCode::CONFLICT, "already_decided"),
                Some("42501") => ApiError(StatusCode::FORBIDDEN, "owner_required"),
                _ => db_error(error),
            },
        )?;
    let result = load(&mut tx, &workspace, BY_ID, Some(&handoff))
        .await?
        .pop()
        .ok_or(ApiError(StatusCode::NOT_FOUND, "not_found"))?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(result))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Undo {
    note: Option<String>,
}

// Session-only: keep (owner checked it) or undo an auto-accepted handoff.
async fn keep(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, handoff)): Path<(String, String)>,
) -> Result<Json<Handoff>, ApiError> {
    after_review(
        state,
        principal,
        workspace,
        handoff,
        "SELECT app.confirm_handoff($1::uuid, $2::uuid) IS NOT NULL FROM (SELECT $3::text) unused_note",
        None,
    )
    .await
}

async fn undo(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, handoff)): Path<(String, String)>,
    Json(body): Json<Undo>,
) -> Result<Json<Handoff>, ApiError> {
    if body
        .note
        .as_deref()
        .is_some_and(|n| n.chars().count() > 2000)
    {
        return Err(invalid());
    }
    after_review(
        state,
        principal,
        workspace,
        handoff,
        "SELECT app.undo_handoff($1::uuid, $2::uuid, $3) IS NOT NULL",
        body.note,
    )
    .await
}

// `call` is one of the two fixed statements above.
async fn after_review(
    state: AppState,
    principal: Principal,
    workspace: String,
    handoff: String,
    call: &str,
    note: Option<String>,
) -> Result<Json<Handoff>, ApiError> {
    ids(&[&workspace, &handoff])?;
    let mut tx = access(&state, &principal, &workspace, true).await?;
    sqlx::query_scalar::<_, bool>(call)
        .bind(&workspace)
        .bind(&handoff)
        .bind(&note)
        .fetch_one(&mut *tx)
        .await
        .map_err(crate::review::review_error)?;
    let result = load(&mut tx, &workspace, BY_ID, Some(&handoff))
        .await?
        .pop()
        .ok_or(ApiError(StatusCode::NOT_FOUND, "not_found"))?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(result))
}

// Auto-accepted handoffs the owner has neither kept nor undone yet.
pub(crate) async fn recent_auto(
    tx: &mut Transaction<'_, Postgres>,
    workspace: &str,
) -> Result<Vec<Handoff>, ApiError> {
    load(
        tx,
        workspace,
        "AND $2::uuid IS NULL AND auto_accepted AND confirmed_at IS NULL AND status IN ('approved', 'superseded') \
         ORDER BY decided_at DESC, id LIMIT 20",
        None,
    )
    .await
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/workspaces/{id}/handoffs/{handoff}/keep", post(keep))
        .route("/workspaces/{id}/handoffs/{handoff}/undo", post(undo))
        .route("/workspaces/{id}/handoffs", get(list).post(create))
        .route("/workspaces/{id}/handoffs/latest", get(latest))
        .route("/workspaces/{id}/handoffs/{handoff}/decision", post(decide))
        .route("/agent/workspaces/{id}/handoffs", get(list).post(create))
        .route("/agent/workspaces/{id}/handoffs/latest", get(latest))
        .route(
            "/agent/workspaces/{id}/handoffs/{handoff}/decision",
            post(decide),
        )
        .route("/agent/workspaces/{id}/handoffs/{handoff}/keep", post(keep))
        .route("/agent/workspaces/{id}/handoffs/{handoff}/undo", post(undo))
}
