use crate::{
    AppState,
    agents::bearer_token,
    auth::{ApiError, CurrentUser, pool, token_hash, unavailable},
    workspaces::{is_uuid, scoped},
};
use axum::{
    Json, Router,
    extract::{FromRequestParts, Path, Query, State},
    http::{StatusCode, header, request::Parts},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};

const MAX_BYTES: usize = 262144;

pub fn db_error(error: sqlx::Error) -> ApiError {
    if let Some(db) = error.as_database_error() {
        match db.code().as_deref() {
            Some("42501") => return ApiError(StatusCode::FORBIDDEN, "read_only"),
            Some("P0002") => return missing(),
            Some("AD409") => return ApiError(StatusCode::CONFLICT, "request_conflict"),
            Some("AD412") => return ApiError(StatusCode::CONFLICT, "version_conflict"),
            Some("AD413") => return ApiError(StatusCode::CONFLICT, "name_taken"),
            Some("AD403") => return ApiError(StatusCode::FORBIDDEN, "insufficient_scope"),
            Some("23514") => return ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_file"),
            _ => {}
        }
    }
    tracing::warn!(kind = crate::error_kind(&error), "file query failed");
    ApiError(StatusCode::SERVICE_UNAVAILABLE, "unavailable")
}

fn missing() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "not_found")
}
pub fn ids(values: &[&str]) -> Result<(), ApiError> {
    if values.iter().all(|v| is_uuid(v)) {
        Ok(())
    } else {
        Err(missing())
    }
}
fn request_id(id: &str) -> Result<(), ApiError> {
    if is_uuid(id) {
        Ok(())
    } else {
        Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request_id",
        ))
    }
}
fn text(content: &str) -> Result<(), ApiError> {
    if content.len() > MAX_BYTES {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE, "file_too_large"));
    }
    if content.contains('\0') {
        return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_text"));
    }
    Ok(())
}
fn filename(raw: &str) -> Result<String, ApiError> {
    let name = raw.trim();
    let lower = name.to_ascii_lowercase();
    if name.is_empty()
        || name.chars().count() > 255
        || name
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
        || !(lower.ends_with(".txt") || lower.ends_with(".md"))
    {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_filename",
        ));
    }
    Ok(name.to_owned())
}
// The same handlers serve people (session cookie, /workspaces/...) and agents
// (bearer token, /agent/workspaces/...); only the request context differs.
pub enum Principal {
    User(String),
    Agent(String),
}

impl FromRequestParts<AppState> for Principal {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        if parts.uri.path().starts_with("/agent/") {
            bearer_token(&parts.headers).map(Principal::Agent)
        } else {
            CurrentUser::from_request_parts(parts, state)
                .await
                .map(|user| Principal::User(user.id))
        }
    }
}

pub async fn access(
    state: &AppState,
    principal: &Principal,
    workspace: &str,
    write: bool,
) -> Result<Transaction<'static, Postgres>, ApiError> {
    ids(&[workspace])?;
    let db = pool(state)?;
    let mut tx = match principal {
        Principal::User(id) => scoped(db, id, Some(workspace)).await?,
        Principal::Agent(token) => {
            let mut tx = db.begin().await.map_err(unavailable)?;
            let grant: Option<String> =
                sqlx::query_scalar("SELECT app.begin_agent_request($1::uuid, $2, $3)::text")
                    .bind(workspace)
                    .bind(token_hash(token))
                    .bind(if write { "files:write" } else { "files:read" })
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(db_error)?;
            if grant.is_none() {
                return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid_agent_token"));
            }
            tx
        }
    };
    sqlx::query("SELECT app.lock_file_access($1::uuid, false)")
        .bind(workspace)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    Ok(tx)
}

#[derive(Serialize)]
struct FileVersion {
    file_id: String,
    name: String,
    version_id: String,
    revision: i64,
    parent_version_id: Option<String>,
    restored_from: Option<String>,
    content: String,
    sha256: String,
    actor_id: String,
    agent_id: Option<String>,
    created_at: String,
    current_version_id: String,
    created_by: String,
    operation: String,
}
type VersionRow = (
    String,
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    String,
    String,
);
const VERSION_SQL: &str = "SELECT f.id::text, f.name, v.id::text, v.revision, v.parent_version_id::text, v.restored_from::text, v.content, encode(v.content_hash, 'hex'), v.actor_id::text, v.agent_id::text, to_char(v.created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), f.current_version_id::text, f.created_by::text, a.operation FROM files f JOIN file_versions v ON v.workspace_id=f.workspace_id AND v.file_id=f.id JOIN file_actions a ON a.version_id=v.id WHERE f.workspace_id=$1::uuid AND ($2::uuid IS NULL OR f.id=$2::uuid) AND v.id=coalesce($3::uuid,f.current_version_id)";
async fn version(
    tx: &mut Transaction<'_, Postgres>,
    workspace: &str,
    file: Option<&str>,
    id: Option<&str>,
) -> Result<FileVersion, ApiError> {
    let row: Option<VersionRow> = sqlx::query_as(VERSION_SQL)
        .bind(workspace)
        .bind(file)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_error)?;
    let (
        file_id,
        name,
        version_id,
        revision,
        parent_version_id,
        restored_from,
        content,
        sha256,
        actor_id,
        agent_id,
        created_at,
        current_version_id,
        created_by,
        operation,
    ) = row.ok_or_else(missing)?;
    Ok(FileVersion {
        file_id,
        name,
        version_id,
        revision,
        parent_version_id,
        restored_from,
        content,
        sha256,
        actor_id,
        agent_id,
        created_at,
        current_version_id,
        created_by,
        operation,
    })
}

#[derive(Deserialize)]
struct Page {
    after: Option<String>,
}
#[derive(Serialize)]
struct FileSummary {
    id: String,
    name: String,
    current_version_id: String,
    byte_size: i64,
}
#[derive(Serialize)]
struct FileList {
    files: Vec<FileSummary>,
    next_after: Option<String>,
}
async fn list(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
    Query(page): Query<Page>,
) -> Result<Json<FileList>, ApiError> {
    if let Some(after) = &page.after {
        ids(&[after])?;
    }
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let mut rows: Vec<(String,String,String,i64)> = sqlx::query_as("SELECT id::text,name,current_version_id::text,byte_size FROM files WHERE workspace_id=$1::uuid AND current_version_id IS NOT NULL AND ($2::uuid IS NULL OR id>$2::uuid) ORDER BY id LIMIT 101")
        .bind(&workspace).bind(page.after).fetch_all(&mut *tx).await.map_err(db_error)?;
    let more = rows.len() > 100;
    rows.truncate(100);
    let next_after = if more {
        rows.last().map(|r| r.0.clone())
    } else {
        None
    };
    tx.commit().await.map_err(db_error)?;
    Ok(Json(FileList {
        files: rows
            .into_iter()
            .map(|(id, name, current_version_id, byte_size)| FileSummary {
                id,
                name,
                current_version_id,
                byte_size,
            })
            .collect(),
        next_after,
    }))
}
async fn show(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, file)): Path<(String, String)>,
) -> Result<Json<FileVersion>, ApiError> {
    ids(&[&file])?;
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let result = version(&mut tx, &workspace, Some(&file), None).await?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(result))
}
async fn show_version(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, file, id)): Path<(String, String, String)>,
) -> Result<Json<FileVersion>, ApiError> {
    ids(&[&file, &id])?;
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let result = version(&mut tx, &workspace, Some(&file), Some(&id)).await?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(result))
}

#[derive(Deserialize)]
struct HistoryPage {
    before: Option<i64>,
}
#[derive(Serialize)]
struct VersionSummary {
    id: String,
    revision: i64,
    actor_id: String,
    agent_id: Option<String>,
    created_at: String,
    sha256: String,
    operation: String,
    restored_from: Option<String>,
}
#[derive(Serialize)]
struct History {
    versions: Vec<VersionSummary>,
    next_before: Option<i64>,
}
type HistoryRow = (
    String,
    i64,
    String,
    Option<String>,
    String,
    String,
    String,
    Option<String>,
);
async fn history(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, file)): Path<(String, String)>,
    Query(page): Query<HistoryPage>,
) -> Result<Json<History>, ApiError> {
    ids(&[&file])?;
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM files WHERE workspace_id=$1::uuid AND id=$2::uuid AND current_version_id IS NOT NULL)")
        .bind(&workspace).bind(&file).fetch_one(&mut *tx).await.map_err(db_error)?;
    if !exists {
        return Err(missing());
    }
    let mut rows: Vec<HistoryRow> = sqlx::query_as("SELECT v.id::text,v.revision,v.actor_id::text,v.agent_id::text,to_char(v.created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),encode(v.content_hash,'hex'),a.operation,v.restored_from::text FROM file_versions v JOIN file_actions a ON a.version_id=v.id WHERE v.workspace_id=$1::uuid AND v.file_id=$2::uuid AND ($3::bigint IS NULL OR v.revision<$3) ORDER BY v.revision DESC LIMIT 51")
        .bind(&workspace).bind(&file).bind(page.before).fetch_all(&mut *tx).await.map_err(db_error)?;
    let more = rows.len() > 50;
    rows.truncate(50);
    let next_before = if more { rows.last().map(|r| r.1) } else { None };
    tx.commit().await.map_err(db_error)?;
    Ok(Json(History {
        versions: rows
            .into_iter()
            .map(
                |(
                    id,
                    revision,
                    actor_id,
                    agent_id,
                    created_at,
                    sha256,
                    operation,
                    restored_from,
                )| {
                    VersionSummary {
                        id,
                        revision,
                        actor_id,
                        agent_id,
                        created_at,
                        sha256,
                        operation,
                        restored_from,
                    }
                },
            )
            .collect(),
        next_before,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    request_id: String,
    name: String,
    content: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edit {
    request_id: String,
    base_version_id: String,
    content: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Restore {
    request_id: String,
    base_version_id: String,
    source_version_id: String,
}

struct Change<'a> {
    request: &'a str,
    operation: &'a str,
    file: Option<&'a str>,
    base: Option<&'a str>,
    name: Option<&'a str>,
    content: Option<&'a str>,
    source: Option<&'a str>,
}
async fn write(
    state: &AppState,
    principal: &Principal,
    workspace: &str,
    change: Change<'_>,
) -> Result<FileVersion, ApiError> {
    request_id(change.request)?;
    let mut tx = access(state, principal, workspace, true).await?;
    let id: String = sqlx::query_scalar(
        "SELECT app.write_text_file($1::uuid,$2::uuid,$3,$4::uuid,$5::uuid,$6,$7,$8::uuid)::text",
    )
    .bind(workspace)
    .bind(change.request)
    .bind(change.operation)
    .bind(change.file)
    .bind(change.base)
    .bind(change.name)
    .bind(change.content)
    .bind(change.source)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_error)?;
    let result = version(&mut tx, workspace, change.file, Some(&id)).await?;
    tx.commit().await.map_err(db_error)?;
    Ok(result)
}
async fn create(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
    Json(body): Json<Create>,
) -> Result<(StatusCode, Json<FileVersion>), ApiError> {
    let name = filename(&body.name)?;
    text(&body.content)?;
    let result = write(
        &state,
        &principal,
        &workspace,
        Change {
            request: &body.request_id,
            operation: "create",
            file: None,
            base: None,
            name: Some(&name),
            content: Some(&body.content),
            source: None,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(result)))
}
async fn edit(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, file)): Path<(String, String)>,
    Json(body): Json<Edit>,
) -> Result<Json<FileVersion>, ApiError> {
    ids(&[&file, &body.base_version_id])?;
    text(&body.content)?;
    Ok(Json(
        write(
            &state,
            &principal,
            &workspace,
            Change {
                request: &body.request_id,
                operation: "update",
                file: Some(&file),
                base: Some(&body.base_version_id),
                name: None,
                content: Some(&body.content),
                source: None,
            },
        )
        .await?,
    ))
}
async fn restore(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, file)): Path<(String, String)>,
    Json(body): Json<Restore>,
) -> Result<Json<FileVersion>, ApiError> {
    ids(&[&file, &body.base_version_id, &body.source_version_id])?;
    Ok(Json(
        write(
            &state,
            &principal,
            &workspace,
            Change {
                request: &body.request_id,
                operation: "restore",
                file: Some(&file),
                base: Some(&body.base_version_id),
                name: None,
                content: None,
                source: Some(&body.source_version_id),
            },
        )
        .await?,
    ))
}
async fn download(
    State(state): State<AppState>,
    principal: Principal,
    Path((workspace, file, id)): Path<(String, String, String)>,
) -> Result<Response, ApiError> {
    ids(&[&file, &id])?;
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let result = version(&mut tx, &workspace, Some(&file), Some(&id)).await?;
    tx.commit().await.map_err(db_error)?;
    let encoded: String = result
        .name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b".-_".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    Ok((
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"aidrive.txt\"; filename*=UTF-8''{encoded}"),
            ),
        ],
        result.content,
    )
        .into_response())
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/workspaces/{id}/files", get(list).post(create))
        .route("/workspaces/{id}/files/{file}", get(show).put(edit))
        .route("/workspaces/{id}/files/{file}/versions", get(history))
        .route(
            "/workspaces/{id}/files/{file}/versions/{version}",
            get(show_version),
        )
        .route(
            "/workspaces/{id}/files/{file}/versions/{version}/download",
            get(download),
        )
        .route("/workspaces/{id}/files/{file}/restore", post(restore))
        .route("/agent/workspaces/{id}/files", get(list).post(create))
        .route("/agent/workspaces/{id}/files/{file}", get(show).put(edit))
        .route("/agent/workspaces/{id}/files/{file}/versions", get(history))
        .route(
            "/agent/workspaces/{id}/files/{file}/versions/{version}",
            get(show_version),
        )
        .layer(middleware::map_response(
            |mut response: Response| async move {
                response
                    .headers_mut()
                    .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
                response
                    .headers_mut()
                    .insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
                response
            },
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_limits_count_bytes_and_reject_nul() {
        assert!(text(&"ա".repeat(MAX_BYTES / 2)).is_ok());
        assert!(text(&"ա".repeat(MAX_BYTES / 2 + 1)).is_err());
        assert!(text("bad\0text").is_err());
        assert!(text("").is_ok());
    }
    #[test]
    fn names_are_labels_for_supported_text_files() {
        assert_eq!(filename("  նշում.MD ").unwrap(), "նշում.MD");
        for name in [
            "../secret.txt",
            "C:\\secret.txt",
            "payload.html",
            "bad\n.txt",
            "",
        ] {
            assert!(filename(name).is_err());
        }
    }
}
