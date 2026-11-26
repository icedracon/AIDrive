use crate::{
    AppState,
    auth::{ApiError, CurrentUser, pool, unavailable},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};

const MAX_NAME_CHARS: usize = 120;

// Request scope for RLS. Both settings are transaction-local, so a pooled
// connection never carries one request's identity into the next.
pub async fn scoped(
    db: &PgPool,
    user: &str,
    workspace: Option<&str>,
) -> Result<Transaction<'static, Postgres>, ApiError> {
    let mut tx = db.begin().await.map_err(unavailable)?;
    sqlx::query(
        "SELECT set_config('app.user_id', $1, true), set_config('app.workspace_id', coalesce($2, ''), true)",
    )
    .bind(user)
    .bind(workspace)
    .execute(&mut *tx)
    .await
    .map_err(unavailable)?;
    Ok(tx)
}

pub fn is_uuid(value: &str) -> bool {
    value.len() == 36
        && value.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

pub fn normalize_name(raw: &str) -> Option<String> {
    let name = raw.trim();
    let length = name.chars().count();
    (1..=MAX_NAME_CHARS)
        .contains(&length)
        .then(|| name.to_owned())
}

#[derive(Serialize)]
pub struct Workspace {
    id: String,
    name: String,
    role: String,
}

#[derive(Serialize)]
struct WorkspaceList {
    workspaces: Vec<Workspace>,
}

#[derive(Serialize)]
struct WorkspaceBody {
    workspace: Workspace,
}

#[derive(Deserialize)]
struct NewWorkspace {
    name: String,
    // "exceptions" (default for new spaces) or "strict".
    review: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<WorkspaceList>, ApiError> {
    let mut tx = scoped(pool(&state)?, &user.id, None).await?;
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT id::text, name, role FROM app.my_workspaces()")
            .fetch_all(&mut *tx)
            .await
            .map_err(unavailable)?;
    tx.commit().await.map_err(unavailable)?;
    Ok(Json(WorkspaceList {
        workspaces: rows
            .into_iter()
            .map(|(id, name, role)| Workspace { id, name, role })
            .collect(),
    }))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<NewWorkspace>,
) -> Result<(StatusCode, Json<WorkspaceBody>), ApiError> {
    let name = normalize_name(&body.name)
        .ok_or(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_name"))?;
    let review = body
        .review
        .as_deref()
        .unwrap_or(crate::review::NEW_SPACE_MODE);
    if !crate::review::MODES.contains(&review) {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_review_mode",
        ));
    }
    let mut tx = scoped(pool(&state)?, &user.id, None).await?;
    let id: String = sqlx::query_scalar("SELECT app.create_workspace($1)::text")
        .bind(&name)
        .fetch_one(&mut *tx)
        .await
        .map_err(unavailable)?;
    crate::review::set_mode(&mut tx, &id, review).await?;
    tx.commit().await.map_err(unavailable)?;
    Ok((
        StatusCode::CREATED,
        Json(WorkspaceBody {
            workspace: Workspace {
                id,
                name,
                role: "owner".into(),
            },
        }),
    ))
}

// Non-members and unknown IDs get the same 404, so existence is not disclosed.
async fn show(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<WorkspaceBody>, ApiError> {
    let not_found = ApiError(StatusCode::NOT_FOUND, "not_found");
    if !is_uuid(&id) {
        return Err(not_found);
    }
    let mut tx = scoped(pool(&state)?, &user.id, Some(&id)).await?;
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT w.id::text, w.name, m.role FROM workspaces w \
         JOIN memberships m ON m.workspace_id = w.id AND m.user_id = app.current_user_id()",
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(unavailable)?;
    tx.commit().await.map_err(unavailable)?;
    let (id, name, role) = row.ok_or(not_found)?;
    Ok(Json(WorkspaceBody {
        workspace: Workspace { id, name, role },
    }))
}

#[derive(Serialize)]
struct Summary {
    id: String,
    name: String,
    role: String,
    file_count: i64,
    registered_chat_count: i64,
    last_visible_activity_at: Option<String>,
}

#[derive(Serialize)]
struct SummaryBody {
    summary: Summary,
}

#[derive(Serialize)]
struct SummaryList {
    summaries: Vec<Summary>,
}

// Runs under the transaction's current app.workspace_id, so RLS on files and
// chats and the visibility rules of app.workspace_activity decide what counts.
// Sessions are not part of the activity feed, so they never move the time.
async fn summarize(
    tx: &mut Transaction<'static, Postgres>,
    id: String,
    name: String,
    role: String,
) -> Result<Summary, ApiError> {
    let (file_count, registered_chat_count, last_visible_activity_at): (i64, i64, Option<String>) =
        sqlx::query_as(
            "SELECT              (SELECT count(*) FROM files WHERE workspace_id = $1::uuid AND current_version_id IS NOT NULL),              (SELECT count(*) FROM chats WHERE workspace_id = $1::uuid),              (SELECT to_char(at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')               FROM app.workspace_activity($1::uuid, NULL, 1))",
        )
        .bind(&id)
        .fetch_one(&mut **tx)
        .await
        .map_err(unavailable)?;
    Ok(Summary {
        id,
        name,
        role,
        file_count,
        registered_chat_count,
        last_visible_activity_at,
    })
}

async fn summary(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<SummaryBody>, ApiError> {
    let not_found = ApiError(StatusCode::NOT_FOUND, "not_found");
    if !is_uuid(&id) {
        return Err(not_found);
    }
    let mut tx = scoped(pool(&state)?, &user.id, Some(&id)).await?;
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT w.id::text, w.name, m.role FROM workspaces w          JOIN memberships m ON m.workspace_id = w.id AND m.user_id = app.current_user_id()",
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(unavailable)?;
    let (id, name, role) = row.ok_or(not_found)?;
    let summary = summarize(&mut tx, id, name, role).await?;
    tx.commit().await.map_err(unavailable)?;
    Ok(Json(SummaryBody { summary }))
}

// One transaction; app.workspace_id is switched to each of the caller's
// workspaces in turn so every count is taken under that workspace's RLS scope.
async fn summaries(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<SummaryList>, ApiError> {
    let mut tx = scoped(pool(&state)?, &user.id, None).await?;
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT id::text, name, role FROM app.my_workspaces()")
            .fetch_all(&mut *tx)
            .await
            .map_err(unavailable)?;
    let mut summaries = Vec::with_capacity(rows.len());
    for (id, name, role) in rows {
        sqlx::query("SELECT set_config('app.workspace_id', $1, true)")
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        summaries.push(summarize(&mut tx, id, name, role).await?);
    }
    tx.commit().await.map_err(unavailable)?;
    Ok(Json(SummaryList { summaries }))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/workspaces", get(list).post(create))
        .route("/workspaces/summaries", get(summaries))
        .route("/workspaces/{id}", get(show))
        .route("/workspaces/{id}/summary", get(summary))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_shape_is_checked_before_reaching_the_database() {
        assert!(is_uuid("00000000-0000-4000-8000-00000000000a"));
        assert!(!is_uuid("00000000-0000-4000-8000-00000000000"));
        assert!(!is_uuid("00000000x0000-4000-8000-00000000000a"));
        assert!(!is_uuid("'; DROP TABLE workspaces; --aaaaaaaaa"));
    }

    #[test]
    fn workspace_names_are_trimmed_and_bounded() {
        assert_eq!(normalize_name("  Launch  ").as_deref(), Some("Launch"));
        assert_eq!(normalize_name("   "), None);
        assert_eq!(
            normalize_name(&"ա".repeat(120)).map(|n| n.chars().count()),
            Some(120)
        );
        assert_eq!(normalize_name(&"x".repeat(121)), None);
    }
}
