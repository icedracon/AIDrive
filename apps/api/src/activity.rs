use crate::{
    AppState,
    auth::ApiError,
    files::{Principal, access, db_error},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    before: Option<String>,
    limit: Option<i32>,
}

#[derive(Serialize)]
struct Event {
    at: String,
    kind: String,
    actor_email: Option<String>,
    agent_id: Option<String>,
    agent_name: Option<String>,
    subject: Option<String>,
    file_id: Option<String>,
    version_id: Option<String>,
    revision: Option<i64>,
    handoff_id: Option<String>,
}

#[derive(Serialize)]
struct Timeline {
    events: Vec<Event>,
    next_before: Option<String>,
}

type Row = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
);

// RFC 3339 with microseconds, as produced below; anything else is rejected
// before it reaches the database.
fn valid_cursor(value: &str) -> bool {
    value.len() == 27
        && value.ends_with('Z')
        && value.bytes().enumerate().all(|(i, b)| match i {
            4 | 7 => b == b'-',
            10 => b == b'T',
            13 | 16 => b == b':',
            19 => b == b'.',
            26 => b == b'Z',
            _ => b.is_ascii_digit(),
        })
}

async fn timeline(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
    Query(page): Query<Page>,
) -> Result<Json<Timeline>, ApiError> {
    if page.before.as_deref().is_some_and(|b| !valid_cursor(b)) {
        return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_cursor"));
    }
    let limit = page.limit.unwrap_or(50).clamp(1, 100);
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT to_char(at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), kind, actor_email, \
         agent_id::text, agent_name, subject, file_id::text, version_id::text, revision, handoff_id::text \
         FROM app.workspace_activity($1::uuid, $2::timestamptz, $3)",
    )
    .bind(&workspace)
    .bind(&page.before)
    .bind(limit)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    let next_before = (rows.len() == limit as usize)
        .then(|| rows.last().map(|r| r.0.clone()))
        .flatten();
    Ok(Json(Timeline {
        events: rows
            .into_iter()
            .map(
                |(
                    at,
                    kind,
                    actor_email,
                    agent_id,
                    agent_name,
                    subject,
                    file_id,
                    version_id,
                    revision,
                    handoff_id,
                )| Event {
                    at,
                    kind,
                    actor_email,
                    agent_id,
                    agent_name,
                    subject,
                    file_id,
                    version_id,
                    revision,
                    handoff_id,
                },
            )
            .collect(),
        next_before,
    }))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/workspaces/{id}/activity", get(timeline))
        .route("/agent/workspaces/{id}/activity", get(timeline))
}

#[cfg(test)]
mod tests {
    use super::valid_cursor;

    #[test]
    fn cursor_shape_is_checked() {
        assert!(valid_cursor("2026-11-19T16:21:03.123456Z"));
        assert!(!valid_cursor("2026-11-19 16:21:03"));
        assert!(!valid_cursor("2026-11-19T16:21:03.123456Z'; --"));
        assert!(!valid_cursor("yesterday"));
    }
}
