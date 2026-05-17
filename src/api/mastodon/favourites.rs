use axum::{
    extract::{Extension, Query, State},
    http::{header, HeaderMap, Uri},
    response::IntoResponse,
    Json,
};

use crate::{
    error::AppResult,
    middleware::AuthenticatedUser,
    state::AppState,
};
use super::{
    accounts::{build_status, fetch_reblog_data, fetch_status_media},
    types::PaginationParams,
};

// ── GET /api/v1/favourites ────────────────────────────────────────────────

pub async fn get_favourites(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    uri: Uri,
    req_headers: HeaderMap,
    Query(q): Query<PaginationParams>,
) -> AppResult<impl IntoResponse> {
    auth.require_scope("read:favourites")?;
    let limit = q.limit_clamped(20, 40);
    let max_id = q.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = q.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = q.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let status_ids: Vec<i64> = if min_id.is_some() {
        sqlx::query_scalar!(
            r#"SELECT s.id FROM statuses s
               JOIN favourites f ON f.status_id = s.id
               WHERE f.account_id = $1
                 AND s.deleted_at IS NULL
                 AND ($2::bigint IS NULL OR s.id > $2)
               ORDER BY s.id ASC LIMIT $3"#,
            auth.account_id, min_id, limit
        )
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_scalar!(
            r#"SELECT s.id FROM statuses s
               JOIN favourites f ON f.status_id = s.id
               WHERE f.account_id = $1
                 AND s.deleted_at IS NULL
                 AND ($2::bigint IS NULL OR s.id < $2)
                 AND ($3::bigint IS NULL OR s.id > $3)
               ORDER BY f.created_at DESC LIMIT $4"#,
            auth.account_id, max_id, since_id, limit
        )
        .fetch_all(&state.db)
        .await?
    };

    let mut result = Vec::with_capacity(status_ids.len());
    for sid in &status_ids {
        let status = sqlx::query_as!(
            crate::db::models::Status,
            "SELECT * FROM statuses WHERE id = $1 AND deleted_at IS NULL",
            sid
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

        let reblogged = sqlx::query!(
            "SELECT 1 as e FROM statuses WHERE account_id = $1 AND reblog_of_id = $2 AND deleted_at IS NULL",
            auth.account_id, s.id
        )
        .fetch_optional(&state.db)
        .await?
        .is_some();

        let bookmarked = sqlx::query!(
            "SELECT 1 as e FROM bookmarks WHERE account_id = $1 AND status_id = $2",
            auth.account_id, s.id
        )
        .fetch_optional(&state.db)
        .await?
        .is_some();

        let reblog = fetch_reblog_data(&state, &s).await?;
        let ctx = super::convert::StatusViewerContext {
            account_id: auth.account_id,
            favourited: true,
            reblogged,
            muted: false,
            bookmarked,
            pinned: false,
        };
        result.push(build_status(&state, &s, &account, media, reblog, Some(ctx)).await?);
    }

    let link = result.first().zip(result.last()).map(|(newest, oldest)| {
        let extra = super::non_pagination_query(uri.query());
        super::link_header(&req_headers, uri.path(), &extra, &newest.id, &oldest.id)
    });
    let mut resp_headers = HeaderMap::new();
    if let Some(v) = link {
        if let Ok(val) = v.parse() {
            resp_headers.insert(header::LINK, val);
        }
    }
    Ok((resp_headers, Json(result)))
}
