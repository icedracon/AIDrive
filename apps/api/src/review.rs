// Approval by exception (ROADMAP "T1 approval by exception: decision").
use crate::{
    AppState,
    auth::ApiError,
    files::{Principal, access, db_error, ids},
    handoffs::{Handoff, recent_auto},
    knowledge::{Item, load},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};

pub const MODES: [&str; 2] = ["exceptions", "strict"];
pub const NEW_SPACE_MODE: &str = "exceptions";

// Why a proposal waits for the owner. Mirrors app.auto_accept_item.
pub fn item_reasons(
    mode: &str,
    kind: &str,
    correction: bool,
    conflict: bool,
    stale: bool,
    no_evidence: bool,
) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if mode == "strict" {
        reasons.push("strict_review");
    }
    if kind == "keystone" {
        reasons.push("keystone");
    }
    if correction {
        reasons.push("correction");
    }
    if conflict {
        reasons.push("conflict");
    }
    if stale {
        reasons.push("stale_evidence");
    }
    if no_evidence {
        reasons.push("no_evidence");
    }
    if reasons.is_empty() {
        // Proposed while strict, or the exception it had (e.g. a conflict) is gone.
        reasons.push("exception_cleared");
    }
    reasons
}

// Mirrors app.auto_accept_handoff.
pub fn handoff_reasons(mode: &str, stale: bool, no_sources: bool) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if mode == "strict" {
        reasons.push("strict_review");
    }
    if stale {
        reasons.push("stale_sources");
    }
    if no_sources {
        reasons.push("no_sources");
    }
    if reasons.is_empty() {
        reasons.push("exception_cleared");
    }
    reasons
}

pub fn review_error(error: sqlx::Error) -> ApiError {
    match error.as_database_error().and_then(|e| e.code()).as_deref() {
        Some("AD409") => ApiError(StatusCode::CONFLICT, "not_auto_accepted"),
        Some("42501") => ApiError(StatusCode::FORBIDDEN, "owner_required"),
        _ => db_error(error),
    }
}

// Owner-only (app.set_review_mode checks); selects the workspace first.
pub async fn set_mode(
    tx: &mut Transaction<'_, Postgres>,
    workspace: &str,
    mode: &str,
) -> Result<(), ApiError> {
    sqlx::query("SELECT set_config('app.workspace_id', $1, true)")
        .bind(workspace)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    sqlx::query("SELECT app.set_review_mode($1::uuid, $2)")
        .bind(workspace)
        .bind(mode)
        .execute(&mut **tx)
        .await
        .map_err(review_error)?;
    Ok(())
}

async fn mode_of(tx: &mut Transaction<'_, Postgres>, workspace: &str) -> Result<String, ApiError> {
    sqlx::query_scalar(
        "SELECT coalesce((SELECT review_mode FROM workspace_settings WHERE workspace_id = $1::uuid), 'strict')",
    )
    .bind(workspace)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)
}

#[derive(Serialize)]
struct Review {
    mode: String,
    // Auto-accepted and not yet kept or undone, newest first.
    recent_knowledge: Vec<Item>,
    recent_handoffs: Vec<Handoff>,
}

async fn show(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
) -> Result<Json<Review>, ApiError> {
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let mode = mode_of(&mut tx, &workspace).await?;
    let recent_knowledge = load(
        &mut tx,
        &workspace,
        "AND $2::text IS NULL AND k.status = 'approved' AND k.auto_accepted AND k.confirmed_at IS NULL \
         AND NOT app.is_derived_item(k.decision_note) ORDER BY k.decided_at DESC, k.id",
        None,
        None,
        20,
    )
    .await?;
    let recent_handoffs = recent_auto(&mut tx, &workspace).await?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(Review {
        mode,
        recent_knowledge,
        recent_handoffs,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModeChange {
    mode: String,
}

#[derive(Serialize)]
struct ModeBody {
    mode: String,
}

// Session-only: no /agent route, and the database refuses agent context.
async fn change(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
    Json(body): Json<ModeChange>,
) -> Result<Json<ModeBody>, ApiError> {
    ids(&[&workspace])?;
    if !MODES.contains(&body.mode.as_str()) {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_review_mode",
        ));
    }
    let mut tx = access(&state, &principal, &workspace, true).await?;
    set_mode(&mut tx, &workspace, &body.mode).await?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(ModeBody { mode: body.mode }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricsQuery {
    days: Option<i32>,
}

#[derive(Serialize, Default)]
struct Day {
    day: String,
    owner_approved: i64,
    auto_accepted: i64,
    kept: i64,
    undone: i64,
    owner_rejected: i64,
}

#[derive(Serialize)]
struct Metrics {
    days: Vec<Day>,
    totals: Day,
    // Owner decisions (approvals, rejections, keeps, undos) per day.
    owner_actions_per_day: f64,
    // Undone / auto-accepted; null while nothing was auto-accepted.
    undo_rate: Option<f64>,
}

type MetricRow = (String, i64, i64, i64, i64, i64);

async fn metrics(
    State(state): State<AppState>,
    principal: Principal,
    Path(workspace): Path<String>,
    Query(q): Query<MetricsQuery>,
) -> Result<Json<Metrics>, ApiError> {
    let days = q.days.unwrap_or(7);
    if !(1..=90).contains(&days) {
        return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_days"));
    }
    let mut tx = access(&state, &principal, &workspace, false).await?;
    let rows: Vec<MetricRow> = sqlx::query_as(
        "SELECT day::text, owner_approved, auto_accepted, kept, undone, owner_rejected \
         FROM app.review_metrics($1::uuid, $2)",
    )
    .bind(&workspace)
    .bind(days)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(Json(summarize(rows)))
}

fn summarize(rows: Vec<MetricRow>) -> Metrics {
    let mut totals = Day {
        day: "total".into(),
        ..Day::default()
    };
    let days: Vec<Day> = rows
        .into_iter()
        .map(
            |(day, owner_approved, auto_accepted, kept, undone, owner_rejected)| {
                totals.owner_approved += owner_approved;
                totals.auto_accepted += auto_accepted;
                totals.kept += kept;
                totals.undone += undone;
                totals.owner_rejected += owner_rejected;
                Day {
                    day,
                    owner_approved,
                    auto_accepted,
                    kept,
                    undone,
                    owner_rejected,
                }
            },
        )
        .collect();
    let owner_actions = totals.owner_approved + totals.owner_rejected + totals.kept + totals.undone;
    Metrics {
        owner_actions_per_day: owner_actions as f64 / days.len().max(1) as f64,
        undo_rate: (totals.auto_accepted > 0)
            .then(|| totals.undone as f64 / totals.auto_accepted as f64),
        days,
        totals,
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/workspaces/{id}/review", get(show).post(change))
        .route("/workspaces/{id}/review/metrics", get(metrics))
        .route("/agent/workspaces/{id}/review", get(show).post(change))
        .route("/agent/workspaces/{id}/review/metrics", get(metrics))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_name_every_exception_and_strict_mode() {
        assert_eq!(
            item_reasons("exceptions", "keystone", true, true, true, true),
            [
                "keystone",
                "correction",
                "conflict",
                "stale_evidence",
                "no_evidence"
            ]
        );
        assert_eq!(
            item_reasons("strict", "fact", false, false, false, false),
            ["strict_review"]
        );
        assert_eq!(
            item_reasons("exceptions", "fact", false, false, false, false),
            ["exception_cleared"]
        );
        assert_eq!(
            handoff_reasons("exceptions", true, false),
            ["stale_sources"]
        );
        assert_eq!(
            handoff_reasons("strict", false, true),
            ["strict_review", "no_sources"]
        );
    }

    #[test]
    fn metrics_total_days_and_undo_rate() {
        let m = summarize(vec![
            ("2026-11-20".into(), 1, 4, 1, 1, 0),
            ("2026-11-21".into(), 0, 4, 0, 1, 1),
        ]);
        assert_eq!((m.totals.auto_accepted, m.totals.undone), (8, 2));
        assert_eq!(m.undo_rate, Some(0.25));
        assert_eq!(m.owner_actions_per_day, 2.5);
        assert_eq!(
            summarize(vec![("2026-11-21".into(), 0, 0, 0, 0, 0)]).undo_rate,
            None
        );
    }
}
