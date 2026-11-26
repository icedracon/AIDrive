// Everything an assistant should know before editing one file: rules and
// earlier attempts that cite it, decisions that rely on it (editing it marks
// them stale), handoffs built on it, and captured sessions that discussed it.
// Read-only and evaluated under the caller's row-level security.
use crate::{
    AppState,
    auth::ApiError,
    files::{Principal, access, db_error, ids},
    knowledge::{Item, load},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use serde::Serialize;

const STAMP: &str = "'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'";

#[derive(Serialize)]
struct FileState {
    id: String,
    name: String,
    current_version_id: String,
    revision: i64,
    changed_at: String,
    changed_by_agent_id: Option<String>,
}

#[derive(Serialize)]
struct HandoffRef {
    id: String,
    status: String,
    summary: String,
    next_step: String,
    cited_version_id: String,
    // True when the file moved past the version the handoff relied on.
    stale: bool,
}

#[derive(Serialize)]
struct Mention {
    session_id: String,
    client: String,
    label: Option<String>,
    ordinal: i32,
    at: Option<String>,
    snippet: String,
    // True when the file changed after this session last mentioned it;
    // null when the session has no timestamps.
    changed_since: Option<bool>,
}

#[derive(Serialize)]
struct Precheck {
    file: FileState,
    knowledge: Vec<Item>,
    handoffs: Vec<HandoffRef>,
    sessions: Vec<Mention>,
}

type FileRow = (String, String, String, i64, String, Option<String>);
type HandoffRow = (String, String, String, String, String, bool);
type MentionRow = (
    String,
    String,
    Option<String>,
    i32,
    Option<String>,
    String,
    Option<bool>,
);

async fn precheck(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, file)): Path<(String, String)>,
) -> Result<Json<Precheck>, ApiError> {
    ids(&[&workspace, &file])?;
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let row: Option<FileRow> = sqlx::query_as(&format!(
        "SELECT f.id::text, f.name, v.id::text, v.revision, to_char(v.created_at AT TIME ZONE 'UTC', {STAMP}), v.agent_id::text \
         FROM files f JOIN file_versions v ON v.workspace_id = f.workspace_id AND v.id = f.current_version_id \
         WHERE f.workspace_id = $1::uuid AND f.id = $2::uuid"
    ))
    .bind(&workspace)
    .bind(&file)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_error)?;
    let (id, name, current_version_id, revision, changed_at, changed_by_agent_id) =
        row.ok_or(ApiError(StatusCode::NOT_FOUND, "not_found"))?;

    // Rules first, then attempts that did not work, then what relies on it.
    let knowledge = load(
        &mut tx,
        &workspace,
        "AND k.status IN ('approved', 'proposed') \
         AND k.id IN (SELECT e.item_id FROM knowledge_evidence e WHERE e.workspace_id = k.workspace_id AND e.file_id = $2::uuid) \
         ORDER BY CASE WHEN k.kind = 'keystone' THEN 0 WHEN k.kind = 'constraint' THEN 1 \
                       WHEN k.kind = 'attempt' AND k.outcome <> 'worked' THEN 2 WHEN k.kind = 'decision' THEN 3 ELSE 4 END, \
                  k.status, k.created_at DESC",
        Some(&file),
        None,
        30,
    )
    .await?;

    let handoffs: Vec<HandoffRow> = sqlx::query_as(
        "SELECT h.id::text, h.status, h.summary, h.next_step, s.version_id::text, f.current_version_id IS DISTINCT FROM s.version_id \
         FROM handoff_sources s \
         JOIN handoffs h ON h.workspace_id = s.workspace_id AND h.id = s.handoff_id \
         JOIN files f ON f.workspace_id = s.workspace_id AND f.id = s.file_id \
         WHERE s.workspace_id = $1::uuid AND s.file_id = $2::uuid AND h.status IN ('approved', 'proposed') \
         ORDER BY h.created_at DESC LIMIT 5",
    )
    .bind(&workspace)
    .bind(&file)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_error)?;

    // A file name is one token in the 'simple' configuration, so a phrase
    // query matches the name itself, not words that merely resemble it.
    let mentions: Vec<MentionRow> = sqlx::query_as(&format!(
        "WITH hits AS ( \
             SELECT DISTINCT ON (e.session_id) e.session_id, e.ordinal, coalesce(e.at, s.ended_at) AS at, e.body, s.client, s.label \
             FROM session_events e JOIN sessions s ON s.workspace_id = e.workspace_id AND s.id = e.session_id \
             WHERE e.workspace_id = $1::uuid AND e.search @@ phraseto_tsquery('simple', $2) \
             ORDER BY e.session_id, e.ordinal DESC \
         ), changed AS ( \
             SELECT v.created_at FROM files f JOIN file_versions v ON v.workspace_id = f.workspace_id AND v.id = f.current_version_id \
             WHERE f.workspace_id = $1::uuid AND f.id = $3::uuid \
         ) \
         SELECT hits.session_id::text, hits.client, hits.label, hits.ordinal, to_char(hits.at AT TIME ZONE 'UTC', {STAMP}), \
                ts_headline('simple', hits.body, phraseto_tsquery('simple', $2), 'MaxWords=30, MinWords=10, StartSel=«, StopSel=»'), \
                CASE WHEN hits.at IS NULL THEN NULL ELSE changed.created_at > hits.at END \
         FROM hits, changed ORDER BY hits.at DESC NULLS LAST LIMIT 5"
    ))
    .bind(&workspace)
    .bind(&name)
    .bind(&file)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;

    Ok(Json(Precheck {
        file: FileState {
            id,
            name,
            current_version_id,
            revision,
            changed_at,
            changed_by_agent_id,
        },
        knowledge,
        handoffs: handoffs
            .into_iter()
            .map(
                |(id, status, summary, next_step, cited_version_id, stale)| HandoffRef {
                    id,
                    status,
                    summary,
                    next_step,
                    cited_version_id,
                    stale,
                },
            )
            .collect(),
        sessions: mentions
            .into_iter()
            .map(
                |(session_id, client, label, ordinal, at, snippet, changed_since)| Mention {
                    session_id,
                    client,
                    label,
                    ordinal,
                    at,
                    snippet,
                    changed_since,
                },
            )
            .collect(),
    }))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/workspaces/{id}/files/{file}/precheck", get(precheck))
        .route(
            "/agent/workspaces/{id}/files/{file}/precheck",
            get(precheck),
        )
}
