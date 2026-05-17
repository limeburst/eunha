use axum::{extract::{Path, State}, response::Json, Extension};
use serde::{Deserialize, Serialize};
use crate::{
    error::{AppError, AppResult},
    middleware::AuthenticatedUser,
    state::AppState,
};

pub type ScheduledStatusResponse = ScheduledStatus;

#[derive(Debug, Serialize)]
pub struct ScheduledStatus {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_at: Option<String>,
    pub params: serde_json::Value,
    pub media_attachments: Vec<serde_json::Value>,
}

pub async fn list_scheduled_statuses(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<ScheduledStatus>>> {
    let rows = sqlx::query!(
        r#"SELECT id, scheduled_at, params
           FROM scheduled_statuses
           WHERE account_id = $1
           ORDER BY scheduled_at ASC NULLS LAST, created_at ASC"#,
        auth.account_id,
    )
    .fetch_all(&state.db)
    .await?;

    let statuses = rows
        .into_iter()
        .map(|r| ScheduledStatus {
            id: r.id.to_string(),
            scheduled_at: r.scheduled_at.map(super::convert::mastodon_date),
            params: r.params.unwrap_or(serde_json::Value::Null),
            media_attachments: vec![],
        })
        .collect();

    Ok(Json(statuses))
}

// ── GET /api/v1/scheduled_statuses/:id ────────────────────────────────────

pub async fn get_scheduled_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<ScheduledStatus>> {
    let row = sqlx::query!(
        "SELECT id, scheduled_at, params FROM scheduled_statuses WHERE id = $1 AND account_id = $2",
        id, auth.account_id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(Json(ScheduledStatus {
        id: row.id.to_string(),
        scheduled_at: row.scheduled_at.map(super::convert::mastodon_date),
        params: row.params.unwrap_or(serde_json::Value::Null),
        media_attachments: vec![],
    }))
}

// ── PUT /api/v1/scheduled_statuses/:id ────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UpdateScheduledStatusForm {
    pub scheduled_at: Option<String>,
}

pub async fn update_scheduled_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<UpdateScheduledStatusForm>,
) -> AppResult<Json<ScheduledStatus>> {
    let scheduled_at = form.scheduled_at.as_deref()
        .map(|s| chrono::DateTime::parse_from_rfc3339(s)
            .map(|dt| dt.with_timezone(&chrono::Utc)))
        .transpose()
        .map_err(|_| AppError::Unprocessable("Invalid scheduled_at format".into()))?;

    let row = sqlx::query!(
        r#"UPDATE scheduled_statuses SET scheduled_at = $1
           WHERE id = $2 AND account_id = $3
           RETURNING id, scheduled_at, params"#,
        scheduled_at,
        id,
        auth.account_id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(Json(ScheduledStatus {
        id: row.id.to_string(),
        scheduled_at: row.scheduled_at.map(super::convert::mastodon_date),
        params: row.params.unwrap_or(serde_json::Value::Null),
        media_attachments: vec![],
    }))
}

// ── DELETE /api/v1/scheduled_statuses/:id ─────────────────────────────────

pub async fn delete_scheduled_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<serde_json::Value>> {
    let deleted = sqlx::query_scalar!(
        "DELETE FROM scheduled_statuses WHERE id = $1 AND account_id = $2 RETURNING id",
        id, auth.account_id,
    )
    .fetch_optional(&state.db)
    .await?;

    if deleted.is_none() {
        return Err(AppError::NotFound);
    }

    Ok(Json(serde_json::json!({})))
}
