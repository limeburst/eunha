use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};

use crate::{
    db::models::Account,
    error::{AppError, AppResult},
    middleware::AuthenticatedUser,
    state::AppState,
};
use super::{
    accounts::{build_status, fetch_account, fetch_reblog_data, fetch_status_media},
    convert::account_from_db,
    statuses::build_viewer_context,
    types::{Conversation, PaginationParams},
};

// ── GET /api/v1/conversations ─────────────────────────────────────────────

pub async fn get_conversations(
    State(state): State<AppState>,
    Query(pagination): Query<PaginationParams>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<Conversation>>> {
    let limit = pagination.limit_clamped(20, 40);
    let max_id = pagination.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = pagination.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let rows = sqlx::query!(
        r#"SELECT c.id, cp.unread
           FROM conversations c
           JOIN conversation_participants cp ON cp.conversation_id = c.id
           WHERE cp.account_id = $1
             AND ($2::bigint IS NULL OR c.id < $2)
             AND ($4::bigint IS NULL OR c.id > $4)
           ORDER BY c.updated_at DESC
           LIMIT $3"#,
        auth.account_id,
        max_id,
        limit,
        since_id,
    )
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::with_capacity(rows.len());
    for row in &rows {
        let participants = sqlx::query_as!(
            Account,
            r#"SELECT a.* FROM accounts a
               JOIN conversation_participants cp ON cp.account_id = a.id
               WHERE cp.conversation_id = $1 AND a.id != $2"#,
            row.id,
            auth.account_id,
        )
        .fetch_all(&state.db)
        .await?;

        let last = sqlx::query_as!(
            crate::db::models::Status,
            "SELECT * FROM statuses WHERE conversation_id = $1 AND deleted_at IS NULL ORDER BY id DESC LIMIT 1",
            row.id,
        )
        .fetch_optional(&state.db)
        .await?;

        let last_status = if let Some(s) = last {
            let saccount = fetch_account(&state, s.account_id).await?;
            let media = fetch_status_media(&state, s.id).await?;
            let reblog = fetch_reblog_data(&state, &s).await?;
            let ctx = build_viewer_context(&state, auth.account_id, s.id).await?;
            Some(build_status(&state, &s, &saccount, media, reblog, Some(ctx)).await?)
        } else {
            None
        };

        result.push(Conversation {
            id: row.id.to_string(),
            unread: row.unread,
            accounts: participants.iter().map(account_from_db).collect(),
            last_status,
        });
    }

    Ok(Json(result))
}

// ── DELETE /api/v1/conversations/:id ─────────────────────────────────────

pub async fn delete_conversation(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<serde_json::Value>> {
    let deleted = sqlx::query!(
        "DELETE FROM conversation_participants WHERE conversation_id = $1 AND account_id = $2 RETURNING conversation_id",
        id,
        auth.account_id,
    )
    .fetch_optional(&state.db)
    .await?;

    if deleted.is_none() {
        return Err(AppError::NotFound);
    }

    Ok(Json(serde_json::json!({})))
}

// ── POST /api/v1/conversations/:id/read ──────────────────────────────────

pub async fn mark_conversation_read(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Conversation>> {
    let updated = sqlx::query!(
        "UPDATE conversation_participants SET unread = false WHERE conversation_id = $1 AND account_id = $2 RETURNING conversation_id",
        id,
        auth.account_id,
    )
    .fetch_optional(&state.db)
    .await?;

    if updated.is_none() {
        return Err(AppError::NotFound);
    }

    let participants = sqlx::query_as!(
        Account,
        r#"SELECT a.* FROM accounts a
           JOIN conversation_participants cp ON cp.account_id = a.id
           WHERE cp.conversation_id = $1 AND a.id != $2"#,
        id,
        auth.account_id,
    )
    .fetch_all(&state.db)
    .await?;

    let last = sqlx::query_as!(
        crate::db::models::Status,
        "SELECT * FROM statuses WHERE conversation_id = $1 AND deleted_at IS NULL ORDER BY id DESC LIMIT 1",
        id,
    )
    .fetch_optional(&state.db)
    .await?;

    let last_status = if let Some(s) = last {
        let saccount = fetch_account(&state, s.account_id).await?;
        let media = fetch_status_media(&state, s.id).await?;
        let reblog = fetch_reblog_data(&state, &s).await?;
        let ctx = build_viewer_context(&state, auth.account_id, s.id).await?;
        Some(build_status(&state, &s, &saccount, media, reblog, Some(ctx)).await?)
    } else {
        None
    };

    Ok(Json(Conversation {
        id: id.to_string(),
        unread: false,
        accounts: participants.iter().map(account_from_db).collect(),
        last_status,
    }))
}
