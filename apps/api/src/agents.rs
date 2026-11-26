use crate::{
    AppState,
    auth::{ApiError, CurrentUser, new_token, pool, token_hash},
    workspaces::{is_uuid, normalize_name, scoped},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    middleware,
    routing::{delete, get},
};
use serde::{Deserialize, Serialize};

const PREFIX: &str = "aidrive_agent_";
// Roles (migration 0021): files:write = low, review:accept = medium, grand = grand.
// Only a person who owns the space issues them, from the web app or by
// approving a pairing request.
pub const SCOPES: [&str; 5] = [
    "workspace:read",
    "files:read",
    "files:write",
    "review:accept",
    "grand",
];
// Local trust connects without anyone approving, so it never issues more than low.
pub const LOCAL_SCOPES: [&str; 3] = ["workspace:read", "files:read", "files:write"];

// Returns the full presented token (prefix included), which is what is hashed.
pub fn bearer_token(headers: &HeaderMap) -> Result<String, ApiError> {
    let denied = || ApiError(StatusCode::UNAUTHORIZED, "invalid_agent_token");
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = values
        .next()
        .and_then(|v| v.to_str().ok())
        .ok_or_else(denied)?;
    if values.next().is_some() {
        return Err(denied());
    }
    let (scheme, token) = value.split_once(' ').ok_or_else(denied)?;
    let secret = token.strip_prefix(PREFIX).ok_or_else(denied)?;
    if !scheme.eq_ignore_ascii_case("Bearer")
        || secret.len() != 64
        || !secret.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(denied());
    }
    Ok(token.to_owned())
}

fn database_error(error: sqlx::Error) -> ApiError {
    if let Some(error) = error.as_database_error() {
        match error.code().as_deref() {
            Some("42501") => return ApiError(StatusCode::NOT_FOUND, "not_found"),
            Some("23505") => return ApiError(StatusCode::CONFLICT, "request_already_used"),
            _ => {}
        }
    }
    tracing::warn!(kind = crate::error_kind(&error), "agent grant query failed");
    ApiError(StatusCode::SERVICE_UNAVAILABLE, "unavailable")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewGrant {
    request_id: String,
    name: String,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Serialize)]
struct Grant {
    id: String,
    name: String,
    scope: String,
    created_at: String,
    expires_at: String,
    status: String,
}

#[derive(Serialize)]
struct GrantList {
    grants: Vec<Grant>,
}

#[derive(Serialize)]
struct IssuedGrant {
    grant: Grant,
    token: String,
}

type GrantRow = (String, String, String, String, String, String);

const LIST_SQL: &str = "SELECT id::text, name, scope, \
    to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
    to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), status \
    FROM app.list_agent_grants($1::uuid)";

fn grant((id, name, scope, created_at, expires_at, status): GrantRow) -> Grant {
    Grant {
        id,
        name,
        scope,
        created_at,
        expires_at,
        status,
    }
}

fn valid_workspace(id: &str) -> Result<(), ApiError> {
    if is_uuid(id) {
        Ok(())
    } else {
        Err(ApiError(StatusCode::NOT_FOUND, "not_found"))
    }
}

async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<GrantList>, ApiError> {
    valid_workspace(&id)?;
    let mut tx = scoped(pool(&state)?, &user.id, Some(&id)).await?;
    let rows: Vec<GrantRow> = sqlx::query_as(LIST_SQL)
        .bind(&id)
        .fetch_all(&mut *tx)
        .await
        .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    Ok(Json(GrantList {
        grants: rows.into_iter().map(grant).collect(),
    }))
}

async fn issue(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(body): Json<NewGrant>,
) -> Result<(StatusCode, Json<IssuedGrant>), ApiError> {
    valid_workspace(&id)?;
    if !is_uuid(&body.request_id) {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request_id",
        ));
    }
    let name = normalize_name(&body.name)
        .filter(|s| !s.chars().any(char::is_control))
        .ok_or(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_name"))?;
    let scope = body.scope.as_deref().unwrap_or("workspace:read");
    if !SCOPES.contains(&scope) {
        return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_scope"));
    }
    let token = format!("{PREFIX}{}", new_token().0);
    let mut tx = scoped(pool(&state)?, &user.id, Some(&id)).await?;
    sqlx::query("SELECT app.issue_agent_grant($1::uuid, $2::uuid, $3, $4, $5)")
        .bind(&id)
        .bind(&body.request_id)
        .bind(&name)
        .bind(token_hash(&token))
        .bind(scope)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    let row: GrantRow = sqlx::query_as(&format!("{LIST_SQL} WHERE id = $2::uuid"))
        .bind(&id)
        .bind(&body.request_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    Ok((
        StatusCode::CREATED,
        Json(IssuedGrant {
            grant: grant(row),
            token,
        }),
    ))
}

async fn revoke(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, agent)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    valid_workspace(&id)?;
    valid_workspace(&agent)?;
    let mut tx = scoped(pool(&state)?, &user.id, Some(&id)).await?;
    let found: bool = sqlx::query_scalar("SELECT app.revoke_agent_grant($1::uuid, $2::uuid)")
        .bind(&id)
        .bind(&agent)
        .fetch_one(&mut *tx)
        .await
        .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    if found {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError(StatusCode::NOT_FOUND, "not_found"))
    }
}

#[derive(Serialize)]
struct AgentWorkspace {
    id: String,
    name: String,
    agent_id: String,
    scope: String,
}

async fn read_workspace(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<AgentWorkspace>, ApiError> {
    if !is_uuid(&id) {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid_agent_token"));
    }
    let token = bearer_token(&headers)?;
    let row: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT id::text, name, agent_id::text, scope FROM app.read_agent_workspace($1::uuid, $2)",
    )
    .bind(&id)
    .bind(token_hash(&token))
    .fetch_optional(pool(&state)?)
    .await
    .map_err(database_error)?;
    let (id, name, agent_id, scope) =
        row.ok_or(ApiError(StatusCode::UNAUTHORIZED, "invalid_agent_token"))?;
    Ok(Json(AgentWorkspace {
        id,
        name,
        agent_id,
        scope,
    }))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/workspaces/{id}/agents", get(list).post(issue))
        .route("/workspaces/{id}/agents/{agent}", delete(revoke))
        .route("/agent/workspaces/{id}", get(read_workspace))
        .layer(middleware::map_response(
            |mut response: axum::response::Response| async move {
                response
                    .headers_mut()
                    .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
                response
            },
        ))
}
