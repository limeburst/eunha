use axum::{
    extract::{Extension, Query, State},
    Json,
};

use crate::{
    error::AppResult,
    middleware::AuthenticatedUser,
    state::AppState,
};
use super::{
    accounts::{fetch_reblog_data, fetch_status_media},
    convert::status_from_db,
    types::{PaginationParams, Status},
};

// ── GET /api/v1/bookmarks ─────────────────────────────────────────────────

pub async fn get_bookmarks(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Query(q): Query<PaginationParams>,
) -> AppResult<Json<Vec<Status>>> {
    let limit = q.limit_clamped(20, 40);
    let max_id = q.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = q.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let rows = sqlx::query!(
        r#"SELECT s.id as status_id FROM statuses s
           JOIN bookmarks b ON b.status_id = s.id
           WHERE b.account_id = $1
             AND s.deleted_at IS NULL
             AND ($2::bigint IS NULL OR s.id < $2)
             AND ($3::bigint IS NULL OR s.id > $3)
           ORDER BY b.created_at DESC LIMIT $4"#,
        auth.account_id, max_id, since_id, limit
    )
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::with_capacity(rows.len());
    for row in &rows {
        let status = sqlx::query_as!(
            crate::db::models::Status,
            "SELECT * FROM statuses WHERE id = $1 AND deleted_at IS NULL",
            row.status_id
        )
        .fetch_optional(&state.db)
        .await?;

        let Some(s) = status else { continue };

        let account = sqlx::query_as!(
            crate::db::models::Account,
            "SELECT * FROM accounts WHERE id = $1",
            s.account_id
        )
        .fetch_one(&state.db)
        .await?;

        let media = fetch_status_media(&state, s.id).await?;

        let favourited = sqlx::query!(
            "SELECT 1 as e FROM favourites WHERE account_id = $1 AND status_id = $2",
            auth.account_id, s.id
        )
        .fetch_optional(&state.db)
        .await?
        .is_some();

        let reblogged = sqlx::query!(
            "SELECT 1 as e FROM statuses WHERE account_id = $1 AND reblog_of_id = $2 AND deleted_at IS NULL",
            auth.account_id, s.id
        )
        .fetch_optional(&state.db)
        .await?
        .is_some();

        let reblog = fetch_reblog_data(&state, &s).await?;
        let ctx = super::convert::StatusViewerContext {
            favourited,
            reblogged,
            muted: false,
            bookmarked: true,
            pinned: false,
        };
        result.push(status_from_db(&s, &account, media, reblog, Some(ctx)));
    }

    Ok(Json(result))
}
