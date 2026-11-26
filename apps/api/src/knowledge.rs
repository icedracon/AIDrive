use crate::{
    AppState,
    auth::{ApiError, unavailable},
    files::{Principal, access, db_error, ids},
    sessions::MATCH_QUERY,
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

const KINDS: [&str; 7] = [
    "decision",
    "fact",
    "constraint",
    "preference",
    "gotcha",
    "attempt",
    "keystone",
];
const STAMP: &str = "'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'";
// Validity bounds keep microseconds so a returned bound can be passed back as `as_of`.
const VALID_STAMP: &str = "'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'";

fn invalid() -> ApiError {
    ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_knowledge")
}

// Shape check only; PostgreSQL parses the value and rejects impossible dates.
fn timestamp_like(value: &str) -> bool {
    (10..=40).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || b"-:.+TZ ".contains(&b))
}

fn bounded(value: &str, max: usize) -> bool {
    (1..=max).contains(&value.trim().chars().count())
        && !value
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceIn {
    file_id: Option<String>,
    version_id: Option<String>,
    session_id: Option<String>,
    ordinal: Option<i32>,
    handoff_id: Option<String>,
    quote: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewItem {
    request_id: String,
    kind: String,
    topic: Option<String>,
    statement: String,
    outcome: Option<String>,
    supersedes: Option<String>,
    #[serde(default)]
    evidence: Vec<EvidenceIn>,
    // team (default) or private (only its creator and their assistants).
    visibility: Option<String>,
}

#[derive(Serialize)]
struct Evidence {
    file_id: Option<String>,
    file_name: Option<String>,
    version_id: Option<String>,
    current_version_id: Option<String>,
    session_id: Option<String>,
    event_key: Option<String>,
    handoff_id: Option<String>,
    quote: Option<String>,
    stale: bool,
}

#[derive(Serialize)]
pub(crate) struct Item {
    id: String,
    kind: String,
    topic: Option<String>,
    statement: String,
    outcome: Option<String>,
    status: String,
    supersedes: Option<String>,
    actor_id: String,
    agent_id: Option<String>,
    decided_at: Option<String>,
    decision_note: Option<String>,
    created_at: String,
    // Valid time: when the item became current knowledge and when it ended
    // (superseded or retired). Recorded time is created_at/decided_at.
    valid_from: Option<String>,
    invalid_at: Option<String>,
    superseded_by: Option<String>,
    invalidation_note: Option<String>,
    visibility: String,
    // True when the caller (or the person an assistant acts for) created it.
    mine: bool,
    // Proposals: approved items that approving this one would contradict.
    conflicts: Vec<String>,
    // Approved items: pending proposals that challenge this one.
    disputed_by: Vec<String>,
    // True when a cited file has moved past the cited version.
    stale: bool,
    // Accepted without the owner (approval by exception); `owner_checked`
    // is true once the owner approved it or kept an auto-accepted item.
    auto_accepted: bool,
    owner_checked: bool,
    confirmed_at: Option<String>,
    // Proposals: why this one waits for the owner instead of being accepted.
    review_reasons: Vec<&'static str>,
    evidence: Vec<Evidence>,
}

type EvidenceRow = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

// `filter` is a fixed fragment chosen by the caller (this module or
// precheck.rs); user input is always bound as $2 or $4.
pub(crate) async fn load(
    tx: &mut Transaction<'_, Postgres>,
    workspace: &str,
    filter: &str,
    arg: Option<&str>,
    as_of: Option<&str>,
    limit: i64,
) -> Result<Vec<Item>, ApiError> {
    // $4 is the optional as-of time; it is always bound so the statement shape
    // stays fixed. Disputes mirror app.item_conflicts from the other side.
    let rows = sqlx::query(&format!(
        "SELECT k.id::text, k.kind, k.topic, k.statement, k.outcome, k.status, k.supersedes::text, k.actor_id::text, \
         k.agent_id::text, to_char(k.decided_at AT TIME ZONE 'UTC', {STAMP}), k.decision_note, \
         to_char(k.created_at AT TIME ZONE 'UTC', {STAMP}), to_char(k.valid_from AT TIME ZONE 'UTC', {VALID_STAMP}), \
         to_char(k.invalid_at AT TIME ZONE 'UTC', {VALID_STAMP}), k.superseded_by::text, k.invalidation_note, \
         CASE WHEN k.status = 'proposed' THEN app.item_conflicts(k.workspace_id, k.kind, k.topic, k.supersedes, k.id)::text[] \
              ELSE '{{}}'::text[] END, \
         CASE WHEN k.status = 'approved' THEN ARRAY(SELECT p.id::text FROM knowledge_items p \
              WHERE p.workspace_id = k.workspace_id AND p.status = 'proposed' \
                AND p.visibility = k.visibility AND (k.visibility = 'team' OR p.actor_id = k.actor_id) \
                AND (p.supersedes = k.id OR (p.kind <> 'attempt' AND p.kind = k.kind AND p.topic = k.topic)) \
              ORDER BY p.created_at, p.id) ELSE '{{}}'::text[] END, \
         k.visibility, k.actor_id = app.current_user_id(), k.auto_accepted, \
         to_char(k.confirmed_at AT TIME ZONE 'UTC', {STAMP}), \
         coalesce((SELECT s.review_mode FROM workspace_settings s WHERE s.workspace_id = k.workspace_id), 'strict') \
         FROM knowledge_items k WHERE k.workspace_id = $1::uuid \
         AND ($4::timestamptz IS NULL OR (k.valid_from <= $4::timestamptz AND (k.invalid_at IS NULL OR k.invalid_at > $4::timestamptz))) \
         {filter} LIMIT $3"
    ))
    .bind(workspace)
    .bind(arg)
    .bind(limit)
    .bind(as_of)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| match error.as_database_error().and_then(|e| e.code()).as_deref() {
        Some("22007" | "22008") => invalid(),
        _ => db_error(error),
    })?;
    let item_ids: Vec<String> = rows.iter().map(|r| r.get::<String, _>(0)).collect();
    let evidence: Vec<EvidenceRow> = sqlx::query_as(
        "SELECT e.item_id::text, e.file_id::text, f.name, e.version_id::text, f.current_version_id::text, \
         e.session_id::text, e.event_key, e.handoff_id::text, e.quote \
         FROM knowledge_evidence e LEFT JOIN files f ON f.workspace_id = e.workspace_id AND f.id = e.file_id \
         WHERE e.workspace_id = $1::uuid AND e.item_id = ANY($2::uuid[]) ORDER BY e.id",
    )
    .bind(workspace)
    .bind(&item_ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_error)?;
    rows.into_iter()
        .map(|row| {
            let id: String = row.try_get(0)?;
            let evidence: Vec<Evidence> = evidence
                .iter()
                .filter(|e| e.0 == id)
                .map(
                    |(
                        _,
                        file_id,
                        file_name,
                        version_id,
                        current,
                        session_id,
                        event_key,
                        handoff_id,
                        quote,
                    )| Evidence {
                        stale: version_id.is_some() && current.as_deref() != version_id.as_deref(),
                        file_id: file_id.clone(),
                        file_name: file_name.clone(),
                        version_id: version_id.clone(),
                        current_version_id: current.clone(),
                        session_id: session_id.clone(),
                        event_key: event_key.clone(),
                        handoff_id: handoff_id.clone(),
                        quote: quote.clone(),
                    },
                )
                .collect();
            let stale = evidence.iter().any(|e| e.stale);
            let status: String = row.try_get(5)?;
            let kind: String = row.try_get(1)?;
            let supersedes: Option<String> = row.try_get(6)?;
            let conflicts: Vec<String> = row.try_get(16)?;
            let valid_from: Option<String> = row.try_get(12)?;
            let auto_accepted: bool = row.try_get(20)?;
            let confirmed_at: Option<String> = row.try_get(21)?;
            let mode: String = row.try_get(22)?;
            let review_reasons = if status == "proposed" {
                crate::review::item_reasons(
                    &mode,
                    &kind,
                    supersedes.is_some(),
                    !conflicts.is_empty(),
                    stale,
                    evidence.is_empty(),
                )
            } else {
                Vec::new()
            };
            Ok(Item {
                stale,
                owner_checked: valid_from.is_some() && (!auto_accepted || confirmed_at.is_some()),
                auto_accepted,
                confirmed_at,
                review_reasons,
                id,
                kind,
                topic: row.try_get(2)?,
                statement: row.try_get(3)?,
                outcome: row.try_get(4)?,
                status,
                supersedes,
                actor_id: row.try_get(7)?,
                agent_id: row.try_get(8)?,
                decided_at: row.try_get(9)?,
                decision_note: row.try_get(10)?,
                created_at: row.try_get(11)?,
                valid_from,
                invalid_at: row.try_get(13)?,
                superseded_by: row.try_get(14)?,
                invalidation_note: row.try_get(15)?,
                conflicts,
                disputed_by: row.try_get(17)?,
                visibility: row.try_get(18)?,
                mine: row.try_get(19)?,
                evidence,
            })
        })
        .collect::<Result<Vec<Item>, sqlx::Error>>()
        .map_err(db_error)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    status: Option<String>,
    kind: Option<String>,
    q: Option<String>,
    // Historical recall: what was approved knowledge at this moment.
    as_of: Option<String>,
    limit: Option<i64>,
    // Paging for the plain (unsearched) list, which also reports `total`.
    offset: Option<i64>,
}

#[derive(Serialize)]
struct ItemList {
    items: Vec<Item>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total: Option<i64>,
}

// Default is current knowledge only (approved); `status=all` adds history
// (proposed, rejected, superseded) for review and audit.
async fn list(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Json<ItemList>, ApiError> {
    let status = q.status.as_deref().unwrap_or("approved");
    let as_of = q.as_of.as_deref().map(str::trim).filter(|t| !t.is_empty());
    if !matches!(status, "approved" | "proposed" | "all")
        || q.kind.as_deref().is_some_and(|k| !KINDS.contains(&k))
        || as_of.is_some_and(|t| q.status.is_some() || !timestamp_like(t))
    {
        return Err(invalid());
    }
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let offset = q.offset.unwrap_or(0);
    if !(0..=100_000).contains(&offset) || (offset > 0 && q.q.is_some()) {
        return Err(invalid());
    }
    let status_clause = match status {
        _ if as_of.is_some() => "",
        "approved" => "AND k.status = 'approved'",
        "proposed" => "AND k.status = 'proposed'",
        _ => "",
    };
    let kind_clause = match q.kind.as_deref() {
        Some(k) => format!("AND k.kind = '{k}'"), // validated against KINDS above
        None => String::new(),
    };
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let items = match q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(text) if text.chars().count() <= 200 => match crate::search::planned(text) {
            // Plain question: half of the content terms must match; more matches rank first.
            Some(planned) => {
                let coverage = crate::search::coverage("k.search");
                let need = crate::search::NEED;
                let filter = format!(
                    "{status_clause} {kind_clause} AND k.search @@ to_tsquery('simple', $2) AND {coverage} >= {need} \
                     ORDER BY {coverage} DESC, ts_rank_cd(k.search, to_tsquery('simple', $2)) DESC, k.created_at DESC"
                );
                load(&mut tx, &workspace, &filter, Some(&planned), as_of, limit).await?
            }
            None => {
                let filter = format!(
                    "{status_clause} {kind_clause} AND k.search @@ {MATCH_QUERY} \
                     ORDER BY ts_rank_cd(k.search, {MATCH_QUERY}) DESC, k.created_at DESC"
                );
                load(&mut tx, &workspace, &filter, Some(text), as_of, limit).await?
            }
        },
        Some(_) => return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_query")),
        None => {
            // k.id breaks created_at ties so pages never repeat or skip an item.
            let filter = format!(
                "{status_clause} {kind_clause} AND $2::text IS NULL \
                 ORDER BY CASE k.kind WHEN 'keystone' THEN 0 WHEN 'constraint' THEN 1 WHEN 'decision' THEN 2 ELSE 3 END, k.created_at DESC, k.id \
                 OFFSET {offset}"
            );
            let items = load(&mut tx, &workspace, &filter, None, as_of, limit).await?;
            let total: i64 = sqlx::query_scalar(&format!(
                "SELECT count(*) FROM knowledge_items k WHERE k.workspace_id = $1::uuid \
                 AND ($2::timestamptz IS NULL OR (k.valid_from <= $2::timestamptz AND (k.invalid_at IS NULL OR k.invalid_at > $2::timestamptz))) \
                 {status_clause} {kind_clause}"
            ))
            .bind(&workspace)
            .bind(as_of)
            .fetch_one(&mut *tx)
            .await
            .map_err(|error| match error.as_database_error().and_then(|e| e.code()).as_deref() {
                Some("22007" | "22008") => invalid(),
                _ => db_error(error),
            })?;
            tx.commit().await.map_err(db_error)?;
            return Ok(Json(ItemList {
                items,
                total: Some(total),
            }));
        }
    };
    tx.commit().await.map_err(db_error)?;
    Ok(Json(ItemList { items, total: None }))
}

async fn propose(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
    Json(body): Json<NewItem>,
) -> Result<(StatusCode, Json<Item>), ApiError> {
    let outcome_ok = match (body.kind.as_str(), body.outcome.as_deref()) {
        ("attempt", Some(o)) => matches!(o, "failed" | "partial" | "worked"),
        ("attempt", None) => false,
        (_, None) => true,
        (_, Some(_)) => false,
    };
    if !is_uuid(&body.request_id)
        || !KINDS.contains(&body.kind.as_str())
        || !outcome_ok
        || !bounded(&body.statement, 2000)
        || body.topic.as_deref().is_some_and(|t| !bounded(t, 120))
        || body.supersedes.as_deref().is_some_and(|s| !is_uuid(s))
        || body
            .visibility
            .as_deref()
            .is_some_and(|v| !matches!(v, "private" | "team"))
        || body.evidence.len() > 10
        || body
            .evidence
            .iter()
            .any(|e| e.quote.as_deref().is_some_and(|q| !bounded(q, 500)))
    {
        return Err(invalid());
    }
    let mut tx = access(&state, &principal, &workspace, true).await?;
    let mut evidence = Vec::new();
    for e in &body.evidence {
        let entry = match (
            &e.file_id,
            &e.version_id,
            &e.session_id,
            e.ordinal,
            &e.handoff_id,
        ) {
            (Some(f), Some(v), None, None, None) if is_uuid(f) && is_uuid(v) => {
                serde_json::json!({"file_id": f, "version_id": v, "quote": e.quote})
            }
            // Search hits identify events by ordinal; resolve the stable key here.
            (None, None, Some(s), Some(ordinal), None) if is_uuid(s) => {
                let key: Option<String> = sqlx::query_scalar(
                    "SELECT event_key FROM session_events WHERE workspace_id = $1::uuid AND session_id = $2::uuid AND ordinal = $3 ORDER BY event_key LIMIT 1",
                )
                .bind(&workspace)
                .bind(s)
                .bind(ordinal)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db_error)?;
                let key = key.ok_or(ApiError(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "unknown_evidence",
                ))?;
                serde_json::json!({"session_id": s, "event_key": key, "quote": e.quote})
            }
            (None, None, None, None, Some(h)) if is_uuid(h) => {
                serde_json::json!({"handoff_id": h, "quote": e.quote})
            }
            _ => return Err(invalid()),
        };
        evidence.push(entry);
    }
    let id: String = sqlx::query_scalar(
        "SELECT app.propose_item($1::uuid, $2::uuid, $3, $4, $5, $6, $7::uuid, $8::jsonb, $9)::text",
    )
    .bind(&workspace)
    .bind(&body.request_id)
    .bind(&body.kind)
    .bind(&body.topic)
    .bind(&body.statement)
    .bind(&body.outcome)
    .bind(&body.supersedes)
    .bind(serde_json::Value::Array(evidence).to_string())
    .bind(body.visibility.as_deref().unwrap_or("team"))
    .fetch_one(&mut *tx)
    .await
    .map_err(
        |error| match error.as_database_error().and_then(|e| e.code()).as_deref() {
            Some("23503") => ApiError(StatusCode::UNPROCESSABLE_ENTITY, "unknown_evidence"),
            Some("23514") => invalid(),
            _ => db_error(error),
        },
    )?;
    let item = load(
        &mut tx,
        &workspace,
        "AND k.id = $2::uuid",
        Some(&id),
        None,
        1,
    )
    .await?
    .pop()
    .ok_or_else(|| unavailable("item vanished"))?;
    tx.commit().await.map_err(db_error)?;
    Ok((StatusCode::CREATED, Json(item)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    decision: String,
    note: Option<String>,
    statement: Option<String>,
    // Required when the proposal conflicts with approved items.
    resolution: Option<String>,
}

// Session-only owner decision; `statement` approves an edited wording.
async fn decide(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, item)): Path<(String, String)>,
    Json(body): Json<Decision>,
) -> Result<Json<Item>, ApiError> {
    ids(&[&workspace, &item])?;
    if !matches!(body.decision.as_str(), "approved" | "rejected")
        || body
            .note
            .as_deref()
            .is_some_and(|n| n.chars().count() > 2000)
        || body.statement.as_deref().is_some_and(|s| !bounded(s, 2000))
        || body
            .resolution
            .as_deref()
            .is_some_and(|r| !matches!(r, "replace" | "keep_both"))
    {
        return Err(invalid());
    }
    let mut tx = access(&state, &principal, &workspace, true).await?;
    let result: String =
        sqlx::query_scalar("SELECT app.decide_item($1::uuid, $2::uuid, $3, $4, $5, $6)::text")
            .bind(&workspace)
            .bind(&item)
            .bind(&body.decision)
            .bind(&body.note)
            .bind(&body.statement)
            .bind(&body.resolution)
            .fetch_one(&mut *tx)
            .await
            .map_err(
                |error| match error.as_database_error().and_then(|e| e.code()).as_deref() {
                    Some("AD412") => ApiError(StatusCode::CONFLICT, "stale_evidence"),
                    Some("AD410") => ApiError(StatusCode::CONFLICT, "conflict_unresolved"),
                    Some("AD409") => ApiError(StatusCode::CONFLICT, "already_decided"),
                    Some("42501") => ApiError(StatusCode::FORBIDDEN, "owner_required"),
                    _ => db_error(error),
                },
            )?;
    let decided = load(
        &mut tx,
        &workspace,
        "AND k.id = $2::uuid",
        Some(&result),
        None,
        1,
    )
    .await?
    .pop()
    .ok_or(ApiError(StatusCode::NOT_FOUND, "not_found"))?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(decided))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Retirement {
    note: Option<String>,
}

// Session-only: the owner ends an approved item without a replacement.
async fn retire(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, item)): Path<(String, String)>,
    Json(body): Json<Retirement>,
) -> Result<Json<Item>, ApiError> {
    ids(&[&workspace, &item])?;
    if body.note.as_deref().is_some_and(|n| !bounded(n, 2000)) {
        return Err(invalid());
    }
    let mut tx = access(&state, &principal, &workspace, true).await?;
    sqlx::query_scalar::<_, String>("SELECT app.retire_item($1::uuid, $2::uuid, $3)::text")
        .bind(&workspace)
        .bind(&item)
        .bind(&body.note)
        .fetch_one(&mut *tx)
        .await
        .map_err(
            |error| match error.as_database_error().and_then(|e| e.code()).as_deref() {
                Some("AD409") => ApiError(StatusCode::CONFLICT, "not_current"),
                Some("42501") => ApiError(StatusCode::FORBIDDEN, "owner_required"),
                _ => db_error(error),
            },
        )?;
    let retired = load(
        &mut tx,
        &workspace,
        "AND k.id = $2::uuid",
        Some(&item),
        None,
        1,
    )
    .await?
    .pop()
    .ok_or(ApiError(StatusCode::NOT_FOUND, "not_found"))?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(retired))
}

// Session-only: keep (owner checked it) or undo an auto-accepted item.
async fn keep(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, item)): Path<(String, String)>,
) -> Result<Json<Item>, ApiError> {
    after_review(state, principal, workspace, item, "SELECT app.confirm_item($1::uuid, $2::uuid) IS NOT NULL FROM (SELECT $3::text) unused_note", None).await
}

async fn undo(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, item)): Path<(String, String)>,
    Json(body): Json<Retirement>,
) -> Result<Json<Item>, ApiError> {
    if body.note.as_deref().is_some_and(|n| !bounded(n, 2000)) {
        return Err(invalid());
    }
    after_review(
        state,
        principal,
        workspace,
        item,
        "SELECT app.undo_item($1::uuid, $2::uuid, $3) IS NOT NULL",
        body.note,
    )
    .await
}

// `call` is one of the two fixed statements above.
async fn after_review(
    state: AppState,
    principal: Principal,
    workspace: String,
    item: String,
    call: &str,
    note: Option<String>,
) -> Result<Json<Item>, ApiError> {
    ids(&[&workspace, &item])?;
    let mut tx = access(&state, &principal, &workspace, true).await?;
    sqlx::query_scalar::<_, bool>(call)
        .bind(&workspace)
        .bind(&item)
        .bind(&note)
        .fetch_one(&mut *tx)
        .await
        .map_err(crate::review::review_error)?;
    let result = load(
        &mut tx,
        &workspace,
        "AND k.id = $2::uuid",
        Some(&item),
        None,
        1,
    )
    .await?
    .pop()
    .ok_or(ApiError(StatusCode::NOT_FOUND, "not_found"))?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(result))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/workspaces/{id}/knowledge/{item}/keep", post(keep))
        .route("/workspaces/{id}/knowledge/{item}/undo", post(undo))
        .route("/workspaces/{id}/knowledge", get(list).post(propose))
        .route("/workspaces/{id}/knowledge/{item}/decision", post(decide))
        .route("/workspaces/{id}/knowledge/{item}/retire", post(retire))
        .route("/agent/workspaces/{id}/knowledge", get(list).post(propose))
        .route(
            "/agent/workspaces/{id}/knowledge/{item}/decision",
            post(decide),
        )
        .route(
            "/agent/workspaces/{id}/knowledge/{item}/retire",
            post(retire),
        )
        .route("/agent/workspaces/{id}/knowledge/{item}/keep", post(keep))
        .route("/agent/workspaces/{id}/knowledge/{item}/undo", post(undo))
}
