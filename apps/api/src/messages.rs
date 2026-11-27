// Messages between the people and assistants of a space (migration 0016).
// Coordination, not approved project state. Sending needs write access
// (editors, owners, files:write keys); reading and read receipts need read
// access, for web sessions and assistant keys alike.
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
    ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_message")
}

fn text_ok(value: &str, max: usize) -> bool {
    (1..=max).contains(&value.trim().chars().count())
        && !value
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
}

#[derive(Serialize)]
struct Source {
    file_id: String,
    file_name: Option<String>,
    version_id: String,
    current_version_id: Option<String>,
    // True when the file moved past the cited version.
    stale: bool,
}

#[derive(Serialize)]
struct Message {
    id: String,
    thread_id: String,
    reply_to: Option<String>,
    from_email: Option<String>,
    from_agent_id: Option<String>,
    from_agent_name: Option<String>,
    // assistants | owner | agent
    to: String,
    to_agent_id: Option<String>,
    to_agent_name: Option<String>,
    subject: Option<String>,
    body: String,
    created_at: String,
    read_by_me: bool,
    sources: Vec<Source>,
    // The chat it was written in, and the one chat it is for (if any).
    from_chat: Option<String>,
    to_chat: Option<String>,
    // Which chat took it, per receiving key.
    deliveries: Vec<Delivery>,
    codex_queue: Option<crate::codex_queue::Receipt>,
}

#[derive(Serialize)]
struct Delivery {
    chat_id: String,
    client: String,
    cwd: Option<String>,
    // true: delivered by waking the chat (or continuing its turn) without the owner.
    wake: bool,
    delivered_at: String,
}

// message_id, chat_id, client, cwd, wake, delivered_at
type DeliveryRow = (String, String, String, Option<String>, bool, String);

// message_id, file_id, file name, cited version, current version
type SourceRow = (String, String, Option<String>, String, Option<String>);

#[derive(Serialize)]
struct MessageList {
    messages: Vec<Message>,
}

async fn load(
    tx: &mut Transaction<'_, Postgres>,
    workspace: &str,
    id: Option<&str>,
    thread: Option<&str>,
    for_me: bool,
    unread: bool,
    limit: i64,
) -> Result<Vec<Message>, ApiError> {
    let rows = sqlx::query(&format!(
        "SELECT id::text, thread_id::text, reply_to::text, actor_email, agent_id::text, agent_name, recipient, \
         recipient_agent::text, recipient_name, subject, body, to_char(created_at AT TIME ZONE 'UTC', {STAMP}), read_by_me \
         FROM app.workspace_messages($1::uuid, $2::uuid, $3::uuid, $4, $5, $6::int)"
    ))
    .bind(workspace)
    .bind(id)
    .bind(thread)
    .bind(for_me)
    .bind(unread)
    .bind(limit)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_error)?;
    let message_ids: Vec<String> = rows.iter().map(|r| r.get::<String, _>(0)).collect();
    let sources: Vec<SourceRow> = sqlx::query_as(
        "SELECT s.message_id::text, s.file_id::text, f.name, s.version_id::text, f.current_version_id::text \
         FROM message_sources s LEFT JOIN files f ON f.workspace_id = s.workspace_id AND f.id = s.file_id \
         WHERE s.workspace_id = $1::uuid AND s.message_id = ANY($2::uuid[]) ORDER BY s.file_id",
    )
    .bind(workspace)
    .bind(&message_ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_error)?;
    let chats: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT id::text, from_chat::text, to_chat::text FROM messages WHERE workspace_id = $1::uuid AND id = ANY($2::uuid[])",
    )
    .bind(workspace)
    .bind(&message_ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_error)?;
    let deliveries: Vec<DeliveryRow> = sqlx::query_as(&format!(
        "SELECT d.message_id::text, d.chat_id::text, c.client, c.cwd, d.wake, \
         to_char(d.delivered_at AT TIME ZONE 'UTC', {STAMP}) \
         FROM message_deliveries d JOIN chats c ON c.id = d.chat_id \
         WHERE d.workspace_id = $1::uuid AND d.message_id = ANY($2::uuid[]) ORDER BY d.delivered_at"
    ))
    .bind(workspace)
    .bind(&message_ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_error)?;
    rows.into_iter()
        .map(|row| {
            let id: String = row.try_get(0)?;
            let cited = sources
                .iter()
                .filter(|s| s.0 == id)
                .map(|(_, file_id, file_name, version_id, current)| Source {
                    stale: current.as_deref() != Some(version_id.as_str()),
                    file_id: file_id.clone(),
                    file_name: file_name.clone(),
                    version_id: version_id.clone(),
                    current_version_id: current.clone(),
                })
                .collect();
            let (from_chat, to_chat) = chats
                .iter()
                .find(|c| c.0 == id)
                .map(|c| (c.1.clone(), c.2.clone()))
                .unwrap_or_default();
            let delivered = deliveries
                .iter()
                .filter(|d| d.0 == id)
                .map(|d| Delivery {
                    chat_id: d.1.clone(),
                    client: d.2.clone(),
                    cwd: d.3.clone(),
                    wake: d.4,
                    delivered_at: d.5.clone(),
                })
                .collect();
            let codex_queue = crate::codex_queue::receipt(workspace, to_chat.as_deref(), &id);
            Ok(Message {
                id,
                thread_id: row.try_get(1)?,
                reply_to: row.try_get(2)?,
                from_email: row.try_get(3)?,
                from_agent_id: row.try_get(4)?,
                from_agent_name: row.try_get(5)?,
                to: row.try_get(6)?,
                to_agent_id: row.try_get(7)?,
                to_agent_name: row.try_get(8)?,
                subject: row.try_get(9)?,
                body: row.try_get(10)?,
                created_at: row.try_get(11)?,
                read_by_me: row.try_get(12)?,
                sources: cited,
                from_chat,
                to_chat,
                codex_queue,
                deliveries: delivered,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(db_error)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    // true: only messages addressed to the caller (its key, all assistants,
    // or for a person the owner) that it did not send.
    for_me: Option<bool>,
    unread: Option<bool>,
    thread: Option<String>,
    limit: Option<i64>,
}

async fn list(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Json<MessageList>, ApiError> {
    if q.thread.as_deref().is_some_and(|t| !is_uuid(t)) {
        return Err(invalid());
    }
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let messages = load(
        &mut tx,
        &workspace,
        None,
        q.thread.as_deref(),
        q.for_me.unwrap_or(false),
        q.unread.unwrap_or(false),
        q.limit.unwrap_or(50).clamp(1, 100),
    )
    .await?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(MessageList { messages }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceIn {
    file_id: String,
    version_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewMessage {
    request_id: String,
    // "assistants", "owner", or the name or id of an assistant key in this space.
    to: String,
    subject: Option<String>,
    body: String,
    reply_to: Option<String>,
    #[serde(default)]
    sources: Vec<SourceIn>,
    // The sender's own chat (a chat of the sending key), and the one chat
    // the message is for. A reply without to_chat goes to the parent's chat.
    from_chat: Option<String>,
    to_chat: Option<String>,
}

async fn send(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
    Json(body): Json<NewMessage>,
) -> Result<(StatusCode, Json<Message>), ApiError> {
    if !is_uuid(&body.request_id)
        || !text_ok(&body.to, 100)
        || !text_ok(&body.body, 8000)
        || body.subject.as_deref().is_some_and(|s| !text_ok(s, 200))
        || body.reply_to.as_deref().is_some_and(|r| !is_uuid(r))
        || body.from_chat.as_deref().is_some_and(|r| !is_uuid(r))
        || body.to_chat.as_deref().is_some_and(|r| !is_uuid(r))
        || body.sources.len() > 10
        || body
            .sources
            .iter()
            .any(|s| !is_uuid(&s.file_id) || !is_uuid(&s.version_id))
    {
        return Err(invalid());
    }
    let sources: Vec<serde_json::Value> = body
        .sources
        .iter()
        .map(|s| serde_json::json!({"file_id": s.file_id, "version_id": s.version_id}))
        .collect();
    let mut tx = access(&state, &principal, &workspace, true).await?;
    let id: String = sqlx::query_scalar(
        "SELECT app.send_message($1::uuid, $2::uuid, $3, $4, $5, $6::uuid, $7::jsonb, $8::uuid, $9::uuid)::text",
    )
    .bind(&workspace)
    .bind(&body.request_id)
    .bind(body.to.trim())
    .bind(&body.subject)
    .bind(&body.body)
    .bind(&body.reply_to)
    .bind(serde_json::Value::Array(sources).to_string())
    .bind(&body.from_chat)
    .bind(&body.to_chat)
    .fetch_one(&mut *tx)
    .await
    .map_err(
        |error| match error.as_database_error().and_then(|e| e.code()).as_deref() {
            Some("AD421") => ApiError(StatusCode::UNPROCESSABLE_ENTITY, "unknown_recipient"),
            Some("AD422") => unknown_chat(),
            Some("AD423") => ApiError(StatusCode::CONFLICT, "ambiguous_chat"),
            Some("23503") => ApiError(StatusCode::UNPROCESSABLE_ENTITY, "unknown_reference"),
            Some("23514") => invalid(),
            _ => db_error(error),
        },
    )?;
    let mut message = load(&mut tx, &workspace, Some(&id), None, false, false, 1)
        .await?
        .pop()
        .ok_or(ApiError(StatusCode::NOT_FOUND, "not_found"))?;
    let job = if message.deliveries.is_empty() {
        crate::codex_queue::prepare(
            &mut tx,
            &workspace,
            message.to_chat.as_deref(),
            &id,
            &message.thread_id,
            message.from_agent_id.is_none(),
        )
        .await
    } else {
        None
    };
    if let Err(error) = tx.commit().await {
        if let Some(job) = job {
            let _ = tokio::task::spawn_blocking(move || crate::codex_queue::abandon(job)).await;
        }
        return Err(db_error(error));
    }
    if let Some(job) = job {
        // Usually done in well under a second; a slow queue call keeps
        // running and the receipt shows "prepared" until it finishes.
        let dispatch = tokio::task::spawn_blocking(move || crate::codex_queue::run(job));
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), dispatch).await;
    }
    message.codex_queue = crate::codex_queue::receipt(&workspace, message.to_chat.as_deref(), &id);
    Ok((StatusCode::CREATED, Json(message)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadIds {
    ids: Vec<String>,
}

#[derive(Serialize)]
struct Marked {
    marked: i32,
}

async fn mark_read(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
    Json(body): Json<ReadIds>,
) -> Result<Json<Marked>, ApiError> {
    if body.ids.is_empty() || body.ids.len() > 100 || body.ids.iter().any(|i| !is_uuid(i)) {
        return Err(invalid());
    }
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let marked: i32 = sqlx::query_scalar("SELECT app.mark_messages_read($1::uuid, $2::uuid[])")
        .bind(&workspace)
        .bind(&body.ids)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(Marked { marked }))
}

async fn one(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, message)): Path<(String, String)>,
) -> Result<Json<Message>, ApiError> {
    ids(&[&workspace, &message])?;
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let found = load(&mut tx, &workspace, Some(&message), None, false, false, 1)
        .await?
        .pop()
        .ok_or(ApiError(StatusCode::NOT_FOUND, "not_found"))?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(found))
}

fn unknown_chat() -> ApiError {
    ApiError(StatusCode::UNPROCESSABLE_ENTITY, "unknown_chat")
}

fn chat_error(error: sqlx::Error) -> ApiError {
    match error.as_database_error().and_then(|e| e.code()).as_deref() {
        Some("AD422") => unknown_chat(),
        Some("23514") => ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_chat"),
        _ => db_error(error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewChat {
    // claude | codex | opencode | other
    client: String,
    // The client's own session (Claude Code) or thread (Codex) id.
    session_id: String,
    cwd: Option<String>,
}

#[derive(Serialize)]
struct ChatId {
    chat_id: String,
}

// Hooks register the chat they run in; assistant keys only.
async fn register_chat(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
    Json(body): Json<NewChat>,
) -> Result<Json<ChatId>, ApiError> {
    if body
        .cwd
        .as_deref()
        .is_some_and(|c| c.chars().count() > 1000)
    {
        return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_chat"));
    }
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let chat_id: String = sqlx::query_scalar("SELECT app.upsert_chat($1::uuid, $2, $3, $4)::text")
        .bind(&workspace)
        .bind(&body.client)
        .bind(&body.session_id)
        .bind(&body.cwd)
        .fetch_one(&mut *tx)
        .await
        .map_err(chat_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(ChatId { chat_id }))
}

// Whole captured assistant notices are evidence, not authenticated provider telemetry.
const CLAUDE_LIMIT_NOTICE: &str = r"\AYou've hit your 5-hour limit · resets (1[0-2]|[1-9])(?::([0-5][0-9]))?(am|pm) \(([A-Za-z_]+(?:/[A-Za-z0-9_+\-]+)*)\)\Z";

#[derive(Serialize)]
struct Chat {
    id: String,
    agent_id: String,
    agent_name: String,
    client: String,
    session_id: String,
    cwd: Option<String>,
    first_seen: String,
    last_seen: String,
    title: Option<String>,
    capacity: serde_json::Value,
}

#[derive(Serialize)]
struct ChatList {
    chats: Vec<Chat>,
}

type ChatRow = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    Option<String>,
    serde_json::Value,
);

async fn list_chats(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
) -> Result<Json<ChatList>, ApiError> {
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let rows: Vec<ChatRow> = sqlx::query_as(&format!(
        "SELECT c.id::text, c.agent_id::text, c.agent_name, c.client, c.session_id, c.cwd, \
         to_char(c.first_seen AT TIME ZONE 'UTC', {STAMP}), to_char(c.last_seen AT TIME ZONE 'UTC', {STAMP}), \
         s.label, jsonb_build_object( \
             'state', CASE WHEN n.event_key IS NULL THEN 'unknown' \
                 WHEN n.at < statement_timestamp() - interval '5 hours' \
                     OR reset.at <= statement_timestamp() THEN 'reset_unverified' ELSE 'limited' END, \
             'evidence_kind', CASE WHEN n.event_key IS NOT NULL THEN 'captured_notice' END, \
             'observed_at', to_char(n.at AT TIME ZONE 'UTC', {STAMP}), \
             'reset_at', to_char(reset.at AT TIME ZONE 'UTC', {STAMP}), \
             'notice', n.body, \
             'source', CASE WHEN n.event_key IS NOT NULL THEN jsonb_build_object( \
                 'session_id', s.id::text, 'event_key', n.event_key, 'ordinal', n.ordinal, 'client', s.client) END, \
             'requires_recheck', n.event_key IS NULL OR n.at < statement_timestamp() - interval '5 hours' \
                 OR COALESCE(reset.at <= statement_timestamp(), false)) \
         FROM app.workspace_chats($1::uuid, 50) c \
         LEFT JOIN sessions s ON s.workspace_id = $1::uuid AND s.external_id = c.session_id \
             AND s.client = CASE c.client WHEN 'claude' THEN 'claude-code' ELSE c.client END \
         LEFT JOIN LATERAL ( \
             SELECT e.event_key, e.ordinal, e.at, e.body, m.parts \
             FROM session_events e CROSS JOIN LATERAL regexp_match(e.body, $2) m(parts) \
             WHERE e.workspace_id = $1::uuid AND e.session_id = s.id \
                 AND s.client = 'claude-code' AND c.client = 'claude' \
                 AND e.role = 'assistant' AND e.at <= statement_timestamp() AND m.parts IS NOT NULL \
             ORDER BY e.at DESC, e.ordinal DESC, e.event_key DESC LIMIT 1 \
         ) n ON true \
         LEFT JOIN LATERAL ( \
             SELECT CASE WHEN n.parts[4] IN ('UTC', 'Asia/Yerevan') THEN \
                 ((n.at AT TIME ZONE n.parts[4])::date + make_time( \
                     (n.parts[1]::int % 12) + CASE n.parts[3] WHEN 'pm' THEN 12 ELSE 0 END, \
                     COALESCE(NULLIF(n.parts[2], ''), '0')::int, 0)) END AS local_at \
         ) clock ON true \
         LEFT JOIN LATERAL ( \
             SELECT CASE WHEN clock.local_at <= n.at AT TIME ZONE CASE WHEN n.parts[4] IN ('UTC', 'Asia/Yerevan') THEN n.parts[4] ELSE 'UTC' END \
                 THEN clock.local_at + interval '1 day' ELSE clock.local_at END AT TIME ZONE \
                 CASE WHEN n.parts[4] IN ('UTC', 'Asia/Yerevan') THEN n.parts[4] ELSE 'UTC' END AS at \
         ) candidate ON true \
         LEFT JOIN LATERAL ( \
             SELECT candidate.at WHERE candidate.at > n.at AND candidate.at <= n.at + interval '5 hours' \
         ) reset ON true \
         ORDER BY c.last_seen DESC, c.id"
    ))
    .bind(&workspace)
    .bind(CLAUDE_LIMIT_NOTICE)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(ChatList {
        chats: rows
            .into_iter()
            .map(|r| Chat {
                id: r.0,
                agent_id: r.1,
                agent_name: r.2,
                client: r.3,
                session_id: r.4,
                cwd: r.5,
                first_seen: r.6,
                last_seen: r.7,
                title: r.8,
                capacity: r.9,
            })
            .collect(),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimRequest {
    // true when the claim wakes the chat (or continues its turn) by itself.
    #[serde(default)]
    wake: bool,
}

// A chat takes the messages that are for it (atomically: one chat per key).
async fn claim(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, chat)): Path<(String, String)>,
    Json(body): Json<ClaimRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ids(&[&workspace, &chat])?;
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let claimed: serde_json::Value =
        sqlx::query_scalar("SELECT app.claim_chat_messages($1::uuid, $2::uuid, $3)")
            .bind(&workspace)
            .bind(&chat)
            .bind(body.wake)
            .fetch_one(&mut *tx)
            .await
            .map_err(chat_error)?;
    tx.commit().await.map_err(db_error)?;
    let ids: Vec<String> = claimed["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["id"].as_str().map(str::to_owned))
        .collect();
    if !ids.is_empty() {
        tokio::task::spawn_blocking(move || {
            crate::codex_queue::cancel_retrieved(&workspace, &chat, &ids)
        });
    }
    Ok(Json(claimed))
}

pub fn routes() -> Router<AppState> {
    let shared = |prefix: &str| {
        Router::new()
            .route(&format!("{prefix}/chats"), get(list_chats))
            .route(&format!("{prefix}/messages"), get(list).post(send))
            .route(&format!("{prefix}/messages/read"), post(mark_read))
            .route(&format!("{prefix}/messages/{{message}}"), get(one))
    };
    shared("/workspaces/{id}")
        .merge(shared("/agent/workspaces/{id}"))
        .route("/agent/workspaces/{id}/chats", post(register_chat))
        .route("/agent/workspaces/{id}/chats/{chat}/claim", post(claim))
}
