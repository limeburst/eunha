use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use serde::Deserialize;

use crate::{
    db::models::{Account, Notification as DbNotification},
    error::{AppError, AppResult},
    middleware::AuthenticatedUser,
    state::AppState,
};
use super::{
    accounts::fetch_status_media,
    convert::{account_from_db, status_from_db},
    types::{Notification, PaginationParams},
};

#[derive(Debug, Deserialize)]
pub struct NotificationsQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    pub types: Option<Vec<String>>,
    pub exclude_types: Option<Vec<String>>,
    pub account_id: Option<uuid::Uuid>,
}

// ── GET /api/v1/notifications ─────────────────────────────────────────────

pub async fn get_notifications(
    State(state): State<AppState>,
    Query(q): Query<NotificationsQuery>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<Notification>>> {
    let limit = q.pagination.limit_clamped(15, 30);
    let max_id = q.pagination.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = q.pagination.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let notifications = sqlx::query_as!(
        DbNotification,
        r#"SELECT * FROM notifications
           WHERE account_id = $1
             AND ($2::bigint IS NULL OR id < $2)
             AND ($3::bigint IS NULL OR id > $3)
           ORDER BY id DESC
           LIMIT $4"#,
        auth.account_id,
        max_id,
        since_id,
        limit,
    )
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::with_capacity(notifications.len());
    for n in &notifications {
        result.push(build_notification(&state, n).await?);
    }
    Ok(Json(result))
}

// ── GET /api/v1/notifications/:id ─────────────────────────────────────────

pub async fn get_notification(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Notification>> {
    let n = sqlx::query_as!(
        DbNotification,
        "SELECT * FROM notifications WHERE id = $1 AND account_id = $2",
        id,
        auth.account_id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    build_notification(&state, &n).await.map(Json)
}

// ── POST /api/v1/notifications/clear ──────────────────────────────────────

pub async fn clear_notifications(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<serde_json::Value>> {
    sqlx::query!(
        "DELETE FROM notifications WHERE account_id = $1",
        auth.account_id
    )
    .execute(&state.db)
    .await?;
    Ok(Json(serde_json::json!({})))
}

// ── POST /api/v1/notifications/:id/dismiss ────────────────────────────────

pub async fn dismiss_notification(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<serde_json::Value>> {
    sqlx::query!(
        "DELETE FROM notifications WHERE id = $1 AND account_id = $2",
        id, auth.account_id,
    )
    .execute(&state.db)
    .await?;
    Ok(Json(serde_json::json!({})))
}

// ── Helpers ────────────────────────────────────────────────────────────────

async fn build_notification(state: &AppState, n: &DbNotification) -> AppResult<Notification> {
    let from_account = sqlx::query_as!(
        Account,
        "SELECT * FROM accounts WHERE id = $1",
        n.from_account_id
    )
    .fetch_one(&state.db)
    .await?;

    let status = if let Some(status_id) = n.status_id {
        let s = sqlx::query_as!(
            crate::db::models::Status,
            "SELECT * FROM statuses WHERE id = $1 AND deleted_at IS NULL",
            status_id
        )
        .fetch_optional(&state.db)
        .await?;

        if let Some(s) = s {
            let account = sqlx::query_as!(
                Account,
                "SELECT * FROM accounts WHERE id = $1",
                s.account_id
            )
            .fetch_one(&state.db)
            .await?;
            let media = fetch_status_media(state, s.id).await?;
            Some(status_from_db(&s, &account, media, None, None))
        } else {
            None
        }
    } else {
        None
    };

    Ok(Notification {
        id: n.id.to_string(),
        notification_type: n.notification_type.clone(),
        created_at: n.created_at.to_rfc3339(),
        account: account_from_db(&from_account),
        status,
    })
}
