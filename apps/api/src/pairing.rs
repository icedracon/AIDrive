use crate::{
    AppState,
    agents::{SCOPES, bearer_token},
    auth::{ApiError, CurrentUser, pool, token_hash, unavailable},
    workspaces::{is_uuid, scoped},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

fn map_error(error: sqlx::Error) -> ApiError {
    match error.as_database_error().and_then(|e| e.code()).as_deref() {
        Some("AD429") => ApiError(StatusCode::TOO_MANY_REQUESTS, "too_many_attempts"),
        Some("AD409") => ApiError(StatusCode::CONFLICT, "already_used"),
        Some("42501") => ApiError(StatusCode::FORBIDDEN, "owner_required"),
        Some("23514") => ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request"),
        _ => unavailable(error),
    }
}

fn hex32(value: &str) -> Option<Vec<u8>> {
    (value.len() == 64)
        .then(|| {
            (0..32)
                .map(|i| u8::from_str_radix(&value[2 * i..2 * i + 2], 16).ok())
                .collect()
        })
        .flatten()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    pairing_id: String,
    client_name: String,
    token_sha256: String,
}

#[derive(Serialize)]
struct Started {
    user_code: String,
    verification_url: String,
    expires_in: u32,
}

// Unauthenticated: the client has no credential yet and sends only the
// SHA-256 of the secret it generated.
async fn start(
    State(state): State<AppState>,
    Json(body): Json<Start>,
) -> Result<(StatusCode, Json<Started>), ApiError> {
    let name = body.client_name.trim();
    let hash = hex32(&body.token_sha256);
    if !is_uuid(&body.pairing_id)
        || name.is_empty()
        || name.chars().count() > 80
        || name.chars().any(char::is_control)
        || hash.is_none()
    {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
        ));
    }
    let code: String = sqlx::query_scalar("SELECT app.start_pairing($1::uuid, $2, $3)")
        .bind(&body.pairing_id)
        .bind(name)
        .bind(hash)
        .fetch_one(pool(&state)?)
        .await
        .map_err(map_error)?;
    Ok((
        StatusCode::CREATED,
        Json(Started {
            verification_url: format!("{}/pair?code={code}", state.web_origin),
            user_code: code,
            expires_in: 600,
        }),
    ))
}

#[derive(Serialize)]
struct Status {
    status: String,
    workspace_id: Option<String>,
    workspace_name: Option<String>,
    scope: Option<String>,
    approver_email: Option<String>,
}

type StatusRow = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    bool,
);

// Polled by the client with the secret it generated; without it the
// pairing is indistinguishable from one that does not exist.
async fn status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Status>, ApiError> {
    let not_found = ApiError(StatusCode::NOT_FOUND, "not_found");
    if !is_uuid(&id) {
        return Err(not_found);
    }
    let token = bearer_token(&headers)?;
    let row: Option<StatusRow> = sqlx::query_as(
        "SELECT status, workspace_id::text, workspace_name, scope, approver_email, expires_at <= now() \
         FROM app.pairing_status($1::uuid, $2)",
    )
    .bind(&id)
    .bind(token_hash(&token))
    .fetch_optional(pool(&state)?)
    .await
    .map_err(map_error)?;
    let (status, workspace_id, workspace_name, scope, approver_email, expired) =
        row.ok_or(not_found)?;
    Ok(Json(Status {
        status: if status == "pending" && expired {
            "expired".into()
        } else {
            status
        },
        workspace_id,
        workspace_name,
        scope,
        approver_email,
    }))
}

#[derive(Serialize)]
struct Pending {
    client_name: String,
    status: String,
    expires_at: String,
}

async fn lookup(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(code): Path<String>,
) -> Result<Json<Pending>, ApiError> {
    if code.len() > 32 {
        return Err(ApiError(StatusCode::NOT_FOUND, "not_found"));
    }
    let mut tx = scoped(pool(&state)?, &user.id, None).await?;
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT client_name, status, to_char(expires_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') \
         FROM app.lookup_pairing($1)",
    )
    .bind(&code)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_error)?;
    // Commit even when nothing matched, so the failed attempt is counted.
    tx.commit().await.map_err(map_error)?;
    let (client_name, status, expires_at) =
        row.ok_or(ApiError(StatusCode::NOT_FOUND, "not_found"))?;
    Ok(Json(Pending {
        client_name,
        status,
        expires_at,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    decision: String,
    workspace_id: Option<String>,
    scope: Option<String>,
}

#[derive(Serialize)]
struct Decided {
    status: &'static str,
    agent_id: Option<String>,
}

async fn decide(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(code): Path<String>,
    Json(body): Json<Decision>,
) -> Result<Json<Decided>, ApiError> {
    let approving = match body.decision.as_str() {
        "approved" => true,
        "denied" => false,
        _ => {
            return Err(ApiError(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_request",
            ));
        }
    };
    let scope = body.scope.as_deref().unwrap_or("files:write");
    let workspace = body.workspace_id.as_deref().filter(|w| is_uuid(w));
    if code.len() > 32 || !SCOPES.contains(&scope) || (approving && workspace.is_none()) {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
        ));
    }
    let mut tx = scoped(pool(&state)?, &user.id, workspace).await?;
    let result: String = sqlx::query_scalar("SELECT app.decide_pairing($1, $2::uuid, $3, $4)")
        .bind(&code)
        .bind(workspace)
        .bind(scope)
        .bind(&body.decision)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_error)?;
    tx.commit().await.map_err(map_error)?;
    match result.as_str() {
        "not_found" => Err(ApiError(StatusCode::NOT_FOUND, "not_found")),
        "denied" => Ok(Json(Decided {
            status: "denied",
            agent_id: None,
        })),
        grant => Ok(Json(Decided {
            status: "approved",
            agent_id: Some(grant.to_owned()),
        })),
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/pairing", post(start))
        .route("/agent/pairing/{id}/status", post(status))
        .route("/pairing/{code}", get(lookup))
        .route("/pairing/{code}/decision", post(decide))
}

#[cfg(test)]
mod tests {
    use super::hex32;

    #[test]
    fn token_hashes_must_be_exactly_32_hex_bytes() {
        assert_eq!(hex32(&"ab".repeat(32)).map(|v| v.len()), Some(32));
        assert!(hex32(&"ab".repeat(31)).is_none());
        assert!(hex32(&"zz".repeat(32)).is_none());
    }
}
