use axum::{
    extract::{Extension, State},
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    middleware::AuthenticatedUser,
    state::AppState,
};
use super::{
    convert::account_from_db,
    types::Report,
};

// ── POST /api/v1/reports ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ReportForm {
    pub account_id: Uuid,
    pub status_ids: Option<Vec<String>>,
    pub comment: Option<String>,
    pub forward: Option<bool>,
    pub category: Option<String>,
}

pub async fn file_report(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<ReportForm>,
) -> AppResult<Json<Report>> {
    let target_account = sqlx::query_as!(
        crate::db::models::Account,
        "SELECT * FROM accounts WHERE id = $1",
        form.account_id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let status_ids: Vec<i64> = form.status_ids
        .unwrap_or_default()
        .iter()
        .filter_map(|s| s.parse::<i64>().ok())
        .collect();

    let comment = form.comment.unwrap_or_default();
    let forwarded = form.forward.unwrap_or(false);
    let category = form.category.unwrap_or_else(|| "other".into());

    let report = sqlx::query!(
        r#"INSERT INTO reports (account_id, target_account_id, status_ids, comment, forwarded, category)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id, created_at"#,
        auth.account_id,
        form.account_id,
        &status_ids,
        comment,
        forwarded,
        category,
    )
    .fetch_one(&state.db)
    .await?;

    let status_id_strings: Vec<String> = status_ids.iter().map(|id| id.to_string()).collect();

    Ok(Json(Report {
        id: report.id.to_string(),
        action_taken: false,
        action_taken_at: None,
        category,
        comment,
        forwarded,
        created_at: report.created_at.to_rfc3339(),
        status_ids: if status_id_strings.is_empty() { None } else { Some(status_id_strings) },
        rule_ids: None,
        target_account: account_from_db(&target_account),
    }))
}
