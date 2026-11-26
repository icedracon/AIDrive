use crate::{
    AppState,
    auth::{ApiError, CurrentUser, pool, unavailable},
    files::{Principal, access, db_error, ids},
    workspaces::scoped,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};

const STAMP: &str = "'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'";

// Search query used by session and knowledge search ($2 is the user's text):
// any term may match, 3+ character terms match as prefixes, phrases stay
// phrases and "-term" exclusions stay ANDed. Every quoted lexeme is taken in
// order first, so a short word can never pair quotes across its neighbours
// (the bug the memory evaluation found: "PayLink-ի միջնորդավճար" matched nothing).
pub(crate) const MATCH_QUERY: &str = "to_tsquery('simple', regexp_replace(regexp_replace(regexp_replace(\
    websearch_to_tsquery('simple', $2)::text, '''((?:[^'']|'''')+)''', '''\\1'':*', 'g'), \
    '''([^'']{1,2})'':\\*', '''\\1''', 'g'), ' & (?!!)', ' | ', 'g'))";

fn invalid() -> ApiError {
    ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_session")
}

fn map(error: sqlx::Error) -> ApiError {
    match error.as_database_error().and_then(|e| e.code()).as_deref() {
        // Malformed timestamps or values rejected by a check constraint.
        Some("22007") | Some("22008") | Some("22P02") | Some("23514") | Some("23502") => invalid(),
        // Another person already captured a session with this external id.
        Some("AD409") => ApiError(StatusCode::CONFLICT, "session_belongs_to_another_person"),
        _ => db_error(error),
    }
}

fn bounded(value: &str, max: usize) -> bool {
    let n = value.chars().count();
    (1..=max).contains(&n)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewSession {
    client: String,
    external_id: String,
    label: String,
    origin: String,
    started_at: Option<String>,
    ended_at: Option<String>,
}

#[derive(Serialize)]
struct SessionId {
    session_id: String,
}

async fn upsert(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
    Json(body): Json<NewSession>,
) -> Result<Json<SessionId>, ApiError> {
    if !matches!(body.client.as_str(), "claude-code" | "codex")
        || !matches!(body.origin.as_str(), "import" | "hook")
        || !bounded(&body.external_id, 200)
        || !bounded(body.label.trim(), 200)
    {
        return Err(invalid());
    }
    let mut tx = access(&state, &principal, &workspace, true).await?;
    let id: String = sqlx::query_scalar(
        "SELECT app.upsert_session($1::uuid, $2, $3, $4, $5, $6::timestamptz, $7::timestamptz)::text",
    )
    .bind(&workspace)
    .bind(&body.client)
    .bind(&body.external_id)
    .bind(body.label.trim())
    .bind(&body.origin)
    .bind(&body.started_at)
    .bind(&body.ended_at)
    .fetch_one(&mut *tx)
    .await
    .map_err(map)?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(SessionId { session_id: id }))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EventIn {
    key: String,
    ordinal: i32,
    role: String,
    at: Option<String>,
    text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chunk_index: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chunk_count: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy_sha256: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventBatch {
    events: Vec<EventIn>,
    #[serde(default)]
    complete: bool,
}

#[derive(Serialize)]
struct Inserted {
    inserted: i32,
}

async fn add_events(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, session)): Path<(String, String)>,
    Json(body): Json<EventBatch>,
) -> Result<Json<Inserted>, ApiError> {
    ids(&[&session])?;
    if body.events.len() > 500
        || !body.events.iter().all(|e| {
            bounded(&e.key, 200)
                && e.ordinal >= 0
                && matches!(e.role.as_str(), "user" | "assistant" | "tool")
                && bounded(&e.text, 4000)
                && !e.text.contains('\0')
                && e.legacy_sha256.as_deref().is_none_or(|hash| {
                    hash.len() == 64
                        && hash
                            .bytes()
                            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                })
                && match (
                    e.chunk_index,
                    e.chunk_count,
                    e.content_sha256.as_deref(),
                    e.legacy_sha256.as_deref(),
                ) {
                    (None, None, None, None) => true,
                    (Some(index), Some(count), Some(hash), Some(_)) => {
                        matches!(e.role.as_str(), "user" | "assistant")
                            && index >= 0
                            && index < count
                            && hash.len() == 64
                            && hash
                                .bytes()
                                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                    }
                    _ => false,
                }
        })
    {
        return Err(invalid());
    }
    let events = serde_json::to_string(&body.events).map_err(|_| invalid())?;
    let mut tx = access(&state, &principal, &workspace, true).await?;
    let inserted: i32 =
        sqlx::query_scalar("SELECT app.add_session_events($1::uuid, $2::uuid, $3::jsonb, $4)")
            .bind(&workspace)
            .bind(&session)
            .bind(&events)
            .bind(body.complete)
            .fetch_one(&mut *tx)
            .await
            .map_err(map)?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(Inserted { inserted }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    limit: Option<i64>,
}

#[derive(Serialize)]
struct SessionSummary {
    id: String,
    client: String,
    label: String,
    capture_origin: String,
    coverage: String,
    agent_id: Option<String>,
    started_at: Option<String>,
    ended_at: Option<String>,
    event_count: i32,
    // private: only its creator and their assistants see it; team: every reader.
    visibility: String,
    // True when the caller (or the person an assistant acts for) captured it.
    mine: bool,
}

type SummaryRow = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    i32,
    String,
    bool,
);

async fn list(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<SessionSummary>>, ApiError> {
    let limit = q.limit.unwrap_or(20).clamp(1, 100);
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let rows: Vec<SummaryRow> = sqlx::query_as(&format!(
        "SELECT id::text, client, label, capture_origin, coverage, agent_id::text, \
         to_char(started_at AT TIME ZONE 'UTC', {STAMP}), to_char(ended_at AT TIME ZONE 'UTC', {STAMP}), event_count, \
         visibility, actor_id = app.current_user_id() \
         FROM sessions WHERE workspace_id = $1::uuid ORDER BY ended_at DESC NULLS LAST, id LIMIT $2"
    ))
    .bind(&workspace)
    .bind(limit)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(
        rows.into_iter()
            .map(
                |(
                    id,
                    client,
                    label,
                    capture_origin,
                    coverage,
                    agent_id,
                    started_at,
                    ended_at,
                    event_count,
                    visibility,
                    mine,
                )| {
                    SessionSummary {
                        id,
                        client,
                        label,
                        capture_origin,
                        coverage,
                        agent_id,
                        started_at,
                        ended_at,
                        event_count,
                        visibility,
                        mine,
                    }
                },
            )
            .collect(),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchQuery {
    q: String,
    limit: Option<i64>,
}

#[derive(Serialize)]
struct Hit {
    session_id: String,
    client: String,
    label: String,
    ordinal: i32,
    role: String,
    at: Option<String>,
    snippet: String,
    rank: f32,
}

type HitRow = (
    String,
    String,
    String,
    i32,
    String,
    Option<String>,
    String,
    f32,
);

// Lexical first (research/session-memory.md §8): websearch syntax over the
// 'simple' configuration so Armenian and English both match. Loose recall:
// any word may match (top-level AND becomes OR) and events matching more
// words rank higher; quoted phrases and -exclusions keep their meaning.
// Terms of 3+ characters match as prefixes, because 'simple' does no
// stemming and Armenian inflects with suffixes (ֆայլ → ֆայլը, ֆայլերը).
// Ties go to the more recent event.
async fn search(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Vec<Hit>>, ApiError> {
    if !bounded(q.q.trim(), 200) {
        return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_query"));
    }
    let limit = q.limit.unwrap_or(20).clamp(1, 50);
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let planned = crate::search::planned(q.q.trim());
    let sql = match &planned {
        // A source must match half of the content terms; a matched question
        // brings its answer (the next assistant event) right after it.
        Some(_) => {
            let coverage = crate::search::coverage("e.search");
            let need = crate::search::NEED;
            format!(
                "WITH query AS (SELECT to_tsquery('simple', $2) AS tsq), \
                 hits AS (SELECT e.session_id, e.ordinal, e.role, e.at, e.body, {coverage} AS coverage, \
                                 ts_rank_cd(e.search, query.tsq) AS rank \
                          FROM session_events e, query WHERE e.workspace_id = $1::uuid AND e.search @@ query.tsq), \
                 kept AS (SELECT * FROM hits WHERE coverage >= {need}), \
                 answers AS (SELECT a.session_id, a.ordinal, a.role, a.at, a.body, k.coverage, (k.rank * 0.999)::real AS rank \
                             FROM kept k JOIN session_events a ON a.workspace_id = $1::uuid AND a.session_id = k.session_id \
                                  AND a.ordinal = k.ordinal + 1 \
                             WHERE k.role = 'user' AND a.role = 'assistant'), \
                 best AS (SELECT DISTINCT ON (session_id, ordinal) * FROM (SELECT * FROM kept UNION ALL SELECT * FROM answers) u \
                          ORDER BY session_id, ordinal, coverage DESC, rank DESC) \
                 SELECT b.session_id::text, s.client, s.label, b.ordinal, b.role, \
                        to_char(b.at AT TIME ZONE 'UTC', {STAMP}), \
                        ts_headline('simple', b.body, query.tsq, 'MaxWords=40, MinWords=12, StartSel=«, StopSel=»'), b.rank \
                 FROM best b JOIN sessions s ON s.workspace_id = $1::uuid AND s.id = b.session_id, query \
                 ORDER BY b.coverage DESC, b.rank DESC, b.at DESC NULLS LAST LIMIT $3"
            )
        }
        None => format!(
            "WITH query AS (SELECT {MATCH_QUERY} AS tsq) \
         SELECT e.session_id::text, s.client, s.label, e.ordinal, e.role, \
                to_char(e.at AT TIME ZONE 'UTC', {STAMP}), \
                ts_headline('simple', e.body, query.tsq, 'MaxWords=40, MinWords=12, StartSel=«, StopSel=»'), \
                ts_rank_cd(e.search, query.tsq) \
         FROM session_events e JOIN sessions s ON s.workspace_id = e.workspace_id AND s.id = e.session_id, query \
         WHERE e.workspace_id = $1::uuid AND e.search @@ query.tsq \
         ORDER BY ts_rank_cd(e.search, query.tsq) DESC, e.at DESC NULLS LAST LIMIT $3"
        ),
    };
    let rows: Vec<HitRow> = sqlx::query_as(&sql)
        .bind(&workspace)
        .bind(planned.as_deref().unwrap_or(q.q.trim()))
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(
        rows.into_iter()
            .map(
                |(session_id, client, label, ordinal, role, at, snippet, rank)| Hit {
                    session_id,
                    client,
                    label,
                    ordinal,
                    role,
                    at,
                    snippet,
                    rank,
                },
            )
            .collect(),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExcerptQuery {
    around: Option<i32>,
    radius: Option<i32>,
}

#[derive(Serialize)]
struct EventOut {
    ordinal: i32,
    role: String,
    at: Option<String>,
    text: String,
}

async fn excerpt(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, session)): Path<(String, String)>,
    Query(q): Query<ExcerptQuery>,
) -> Result<Json<Vec<EventOut>>, ApiError> {
    ids(&[&session])?;
    let around = q.around.unwrap_or(0).max(0);
    let radius = q.radius.unwrap_or(5).clamp(0, 20);
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let rows: Vec<(i32, String, Option<String>, String)> = sqlx::query_as(&format!(
        "SELECT ordinal, role, to_char(at AT TIME ZONE 'UTC', {STAMP}), body FROM session_events \
         WHERE workspace_id = $1::uuid AND session_id = $2::uuid AND ordinal BETWEEN $3 AND $4 ORDER BY ordinal, event_key"
    ))
    .bind(&workspace)
    .bind(&session)
    .bind(around.saturating_sub(radius))
    .bind(around.saturating_add(radius))
    .fetch_all(&mut *tx)
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    if rows.is_empty() {
        return Err(ApiError(StatusCode::NOT_FOUND, "not_found"));
    }
    Ok(Json(
        rows.into_iter()
            .map(|(ordinal, role, at, text)| EventOut {
                ordinal,
                role,
                at,
                text,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TranscriptQuery {
    after_ordinal: Option<i32>,
    after_key: Option<String>,
    limit: Option<i64>,
}

#[derive(Serialize)]
struct TranscriptSession {
    id: String,
    client: String,
    label: String,
    coverage: String,
    event_count: i32,
    started_at: Option<String>,
    ended_at: Option<String>,
}

#[derive(Serialize)]
struct TranscriptEvent {
    key: String,
    ordinal: i32,
    role: String,
    at: Option<String>,
    text: String,
}

#[derive(Serialize)]
struct TranscriptCursor {
    ordinal: i32,
    key: String,
}

#[derive(Serialize)]
struct Transcript {
    session: TranscriptSession,
    events: Vec<TranscriptEvent>,
    next_cursor: Option<TranscriptCursor>,
}

async fn transcript(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, chat)): Path<(String, String)>,
    Query(q): Query<TranscriptQuery>,
) -> Result<Json<Transcript>, ApiError> {
    ids(&[&chat])?;
    if q.after_ordinal.is_some() != q.after_key.is_some()
        || q.after_ordinal.is_some_and(|n| n < 0)
        || q.after_key.as_ref().is_some_and(|key| !bounded(key, 200))
    {
        return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_cursor"));
    }
    let limit = q.limit.unwrap_or(100).clamp(1, 200);
    let mut tx = access(&state, &principal, &workspace, false).await?;
    type SessionRow = (
        String,
        String,
        String,
        String,
        i32,
        Option<String>,
        Option<String>,
    );
    let row: Option<SessionRow> = sqlx::query_as(&format!(
        "SELECT s.id::text, s.client, s.label, s.coverage, s.event_count, \
         to_char(s.started_at AT TIME ZONE 'UTC', {STAMP}), \
         to_char(s.ended_at AT TIME ZONE 'UTC', {STAMP}) \
         FROM sessions s JOIN chats c ON c.workspace_id = s.workspace_id \
             AND s.external_id = c.session_id \
             AND s.client = CASE c.client WHEN 'claude' THEN 'claude-code' ELSE c.client END \
         WHERE c.workspace_id = $1::uuid AND c.id = $2::uuid"
    ))
    .bind(&workspace)
    .bind(&chat)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_error)?;
    let Some((id, client, label, coverage, event_count, started_at, ended_at)) = row else {
        return Err(ApiError(StatusCode::NOT_FOUND, "capture_not_available"));
    };
    let rows: Vec<(String, i32, String, Option<String>, String)> = sqlx::query_as(&format!(
        "WITH ordered_events AS ( \
             SELECT event_key, (row_number() OVER (ORDER BY at NULLS LAST, ordinal, event_key) - 1)::integer AS ordinal, role, at, body \
             FROM session_events WHERE workspace_id = $1::uuid AND session_id = $2::uuid \
         ) \
         SELECT event_key, ordinal, role, to_char(at AT TIME ZONE 'UTC', {STAMP}), body \
         FROM ordered_events \
         WHERE ($3::integer IS NULL OR (ordinal, event_key) > ($3, $4::text)) \
         ORDER BY ordinal, event_key LIMIT $5"
    ))
    .bind(&workspace)
    .bind(&id)
    .bind(q.after_ordinal)
    .bind(&q.after_key)
    .bind(limit + 1)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    let more = rows.len() > limit as usize;
    let events: Vec<TranscriptEvent> = rows
        .into_iter()
        .take(limit as usize)
        .map(|(key, ordinal, role, at, text)| TranscriptEvent {
            key,
            ordinal,
            role,
            at,
            text,
        })
        .collect();
    let next_cursor = if more {
        events.last().map(|e| TranscriptCursor {
            ordinal: e.ordinal,
            key: e.key.clone(),
        })
    } else {
        None
    };
    Ok(Json(Transcript {
        session: TranscriptSession {
            id,
            client,
            label,
            coverage,
            event_count,
            started_at,
            ended_at,
        },
        events,
        next_cursor,
    }))
}

// Session-only; the database also refuses any agent context.
async fn remove(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((workspace, session)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    ids(&[&workspace, &session])?;
    let mut tx = scoped(pool(&state)?, &user.id, Some(&workspace)).await?;
    let found: bool = sqlx::query_scalar("SELECT app.delete_session($1::uuid, $2::uuid)")
        .bind(&workspace)
        .bind(&session)
        .fetch_one(&mut *tx)
        .await
        .map_err(
            |error| match error.as_database_error().and_then(|e| e.code()).as_deref() {
                Some("42501") => ApiError(StatusCode::FORBIDDEN, "owner_required"),
                _ => unavailable(error),
            },
        )?;
    tx.commit().await.map_err(db_error)?;
    if found {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError(StatusCode::NOT_FOUND, "not_found"))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VisibilityChange {
    visibility: String,
}

// Session-only: the person who captured a session shares or hides it.
async fn set_visibility(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((workspace, session)): Path<(String, String)>,
    Json(body): Json<VisibilityChange>,
) -> Result<StatusCode, ApiError> {
    ids(&[&workspace, &session])?;
    if !matches!(body.visibility.as_str(), "private" | "team") {
        return Err(invalid());
    }
    let mut tx = scoped(pool(&state)?, &user.id, Some(&workspace)).await?;
    sqlx::query("SELECT app.set_session_visibility($1::uuid, $2::uuid, $3)")
        .bind(&workspace)
        .bind(&session)
        .bind(&body.visibility)
        .execute(&mut *tx)
        .await
        .map_err(map)?;
    tx.commit().await.map_err(db_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecaptureAuthorization {
    agent_id: Option<String>,
}

async fn authorize_recapture(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((workspace, session)): Path<(String, String)>,
    Json(body): Json<RecaptureAuthorization>,
) -> Result<StatusCode, ApiError> {
    ids(&[&workspace, &session])?;
    if let Some(agent) = &body.agent_id {
        ids(&[agent])?;
    }
    let mut tx = scoped(pool(&state)?, &user.id, Some(&workspace)).await?;
    sqlx::query("SELECT app.authorize_session_recapture($1::uuid, $2::uuid, $3::uuid)")
        .bind(&workspace)
        .bind(&session)
        .bind(&body.agent_id)
        .execute(&mut *tx)
        .await
        .map_err(map)?;
    tx.commit().await.map_err(db_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn routes() -> Router<AppState> {
    let shared = |prefix: &str| {
        Router::new()
            .route(&format!("{prefix}/sessions"), get(list).post(upsert))
            .route(&format!("{prefix}/sessions/search"), get(search))
            .route(
                &format!("{prefix}/chats/{{chat}}/transcript"),
                get(transcript),
            )
            .route(
                &format!("{prefix}/sessions/{{session}}/events"),
                get(excerpt).post(add_events),
            )
    };
    shared("/workspaces/{id}")
        .merge(shared("/agent/workspaces/{id}"))
        .route("/workspaces/{id}/sessions/{session}", delete(remove))
        .route(
            "/workspaces/{id}/sessions/{session}/recapture-authorization",
            post(authorize_recapture),
        )
        .route(
            "/workspaces/{id}/sessions/{session}/visibility",
            post(set_visibility),
        )
}

#[cfg(test)]
mod tests {
    use super::bounded;

    #[test]
    fn bounds_count_characters_not_bytes() {
        assert!(bounded(&"ա".repeat(4000), 4000));
        assert!(!bounded(&"ա".repeat(4001), 4000));
        assert!(!bounded("", 10));
    }
    #[tokio::test]
    #[ignore = "requires isolated AIDRIVE_CHUNKS_TEST_DB"]
    async fn lossless_chunks_through_http_router() {
        use axum::{
            body::Body,
            http::{Request, StatusCode},
        };
        use tower::ServiceExt;
        let url = std::env::var("AIDRIVE_CHUNKS_TEST_DB").unwrap();
        let private_cluster = std::env::var("AIDRIVE_TEST_CLUSTER").ok();
        assert!(
            url.contains("55449/aidrive_chunks")
                || private_cluster
                    .as_deref()
                    .is_some_and(|p| p.starts_with("/tmp/aidrive-pg.") && url.contains(p)),
            "isolated synthetic fixture only"
        );
        let db = sqlx::postgres::PgPoolOptions::new()
            .connect(&url)
            .await
            .unwrap();
        let app = crate::app(crate::AppState {
            db: Some(db),
            web_origin: "http://localhost:3000".into(),
            allow_signup: true,
        });
        async fn send(
            app: &axum::Router,
            method: &str,
            path: &str,
            cookie: &str,
            body: serde_json::Value,
        ) -> (StatusCode, serde_json::Value, String) {
            let bytes = body.to_string();
            assert!(bytes.len() < 1024 * 1024);
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header("content-type", "application/json")
                        .header("cookie", cookie)
                        .header("origin", "http://localhost:3000")
                        .body(Body::from(bytes))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let cookie = response
                .headers()
                .get("set-cookie")
                .and_then(|c| c.to_str().ok())
                .unwrap_or("")
                .split(';')
                .next()
                .unwrap_or("")
                .to_owned();
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            (
                status,
                serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
                cookie,
            )
        }
        let (status, _, cookie) = send(&app, "POST", "/auth/register", "",
            serde_json::json!({"email":format!("chunks-http-{}@example.test", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),"password":"synthetic fixture password only"})).await;
        assert!(status.is_success());
        let (status, ws, _) = send(
            &app,
            "POST",
            "/workspaces",
            &cookie,
            serde_json::json!({"name":"Chunks HTTP"}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let ws = ws["workspace"]["id"].as_str().unwrap();
        let base = format!("/workspaces/{ws}/sessions");
        let (status, sid, _) = send(&app, "POST", &base, &cookie,
            serde_json::json!({"client":"codex","external_id":"synthetic-http","label":"HTTP","origin":"import"})).await;
        assert!(status.is_success());
        let path = format!("{base}/{}/events", sid["session_id"].as_str().unwrap());
        let authorization = format!(
            "{base}/{}/recapture-authorization",
            sid["session_id"].as_str().unwrap()
        );
        let (status, _, _) = send(
            &app,
            "POST",
            &authorization,
            &cookie,
            serde_json::json!({"agent_id":null}),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _, _) = send(
            &app,
            "POST",
            &authorization,
            "",
            serde_json::json!({"agent_id":null}),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let text = format!(
            "\u{a0}\u{2003}{}[REDACTED] final\n",
            "Հայերեն English ".repeat(160_000)
        );
        assert!(text.len() > 2 * 1024 * 1024);
        let hash = crate::auth::token_hash(&text);
        // token_hash uses SHA-256; preserve every byte, including Unicode whitespace.
        let hash: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        let chunks: Vec<String> = text
            .chars()
            .collect::<Vec<_>>()
            .chunks(4000)
            .map(|c| c.iter().collect())
            .collect();
        let mut events = Vec::new();
        for (index, chunk) in chunks.iter().enumerate() {
            events.push(serde_json::json!({"key":"u","ordinal":0,"role":"user","text":chunk,
                "chunk_index":index,"chunk_count":chunks.len(),"content_sha256":hash,"legacy_sha256":hash}));
        }
        for replay in 0..2 {
            for (index, batch) in events.chunks(40).enumerate() {
                let complete = (index + 1) * 40 >= events.len();
                let (status, result, _) = send(
                    &app,
                    "POST",
                    &path,
                    &cookie,
                    serde_json::json!({"events":batch,"complete":complete}),
                )
                .await;
                assert_eq!(status, StatusCode::OK, "{result}");
                if replay == 0 && index == 0 {
                    let (status, _, _) =
                        send(&app, "GET", &path, &cookie, serde_json::Value::Null).await;
                    assert_eq!(
                        status,
                        StatusCode::NOT_FOUND,
                        "partial fragments became visible"
                    );
                }
            }
        }
        let (status, result, _) = send(&app, "GET", &path, &cookie, serde_json::Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        let events = result.as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0]["text"].as_str().unwrap() == text);
        println!(
            "PASS >2MiB text through bounded HTTP JSON requests, hidden partial upload, canonical response, replay"
        );
    }
}
