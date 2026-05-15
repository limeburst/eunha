use axum::{
    extract::{Extension, Query, State},
    Json,
};
use serde::Deserialize;

use crate::{
    error::AppResult,
    middleware::{AuthenticatedUser, ResolvedInstance},
    state::AppState,
};
use super::{
    accounts::{build_status, fetch_reblog_data, fetch_status_media},
    convert::account_from_db,
    types::{SearchResults, Status, Tag},
};

// ── GET /api/v2/search ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(rename = "type")]
    pub search_type: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub resolve: Option<bool>,
    pub following: Option<bool>,
    pub account_id: Option<String>,
    pub exclude_unreviewed: Option<bool>,
}

pub async fn search(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Query(q): Query<SearchQuery>,
    auth: Option<Extension<AuthenticatedUser>>,
) -> AppResult<Json<SearchResults>> {
    let limit = q.limit.unwrap_or(20).min(40).max(1);
    let pattern = format!("%{}%", q.q.to_lowercase());
    let search_type = q.search_type.as_deref();

    let viewer_id = auth.as_ref().map(|Extension(a)| a.account_id);
    let accounts = if search_type.is_none() || search_type == Some("accounts") {
        let following_filter = q.following.unwrap_or(false);
        if following_filter {
            let vid = viewer_id.ok_or(crate::error::AppError::Unauthorized)?;
            sqlx::query_as!(
                crate::db::models::Account,
                r#"SELECT a.* FROM accounts a
                   JOIN follows f ON f.target_account_id = a.id AND f.state = 'accepted'
                   WHERE a.instance_id = $1
                     AND a.suspended_at IS NULL
                     AND f.account_id = $4
                     AND (lower(a.username) LIKE $2 OR lower(a.display_name) LIKE $2)
                   ORDER BY a.followers_count DESC LIMIT $3"#,
                instance.id, pattern, limit, vid
            )
            .fetch_all(&state.db)
            .await?
            .iter()
            .map(account_from_db)
            .collect()
        } else {
            sqlx::query_as!(
                crate::db::models::Account,
                r#"SELECT * FROM accounts
                   WHERE instance_id = $1
                     AND suspended_at IS NULL
                     AND (lower(username) LIKE $2 OR lower(display_name) LIKE $2)
                   ORDER BY followers_count DESC LIMIT $3"#,
                instance.id, pattern, limit
            )
            .fetch_all(&state.db)
            .await?
            .iter()
            .map(account_from_db)
            .collect()
        }
    } else {
        vec![]
    };

    let statuses = if (search_type.is_none() || search_type == Some("statuses")) && auth.is_some() {
        let fts_query = q.q.trim().to_string();
        let filter_account_id: Option<i64> = q.account_id.as_deref()
            .and_then(|s| s.parse().ok());
        let rows = sqlx::query_as!(
            crate::db::models::Status,
            r#"SELECT s.* FROM statuses s
               JOIN accounts a ON a.id = s.account_id
               WHERE s.instance_id = $1
                 AND s.deleted_at IS NULL
                 AND s.visibility IN ('public', 'unlisted')
                 AND a.suspended_at IS NULL
                 AND (a.domain IS NULL OR NOT EXISTS (
                     SELECT 1 FROM domain_blocks db WHERE db.domain = a.domain
                 ))
                 AND ($4::bigint IS NULL OR s.account_id = $4)
                 AND ($5::bigint IS NULL OR NOT EXISTS (
                     SELECT 1 FROM blocks b
                     WHERE (b.account_id = $5 AND b.target_account_id = s.account_id)
                        OR (b.account_id = s.account_id AND b.target_account_id = $5)
                 ))
                 AND to_tsvector('simple', coalesce(s.content, '') || ' ' || coalesce(s.text, ''))
                     @@ websearch_to_tsquery('simple', $2)
               ORDER BY s.id DESC LIMIT $3"#,
            instance.id, fts_query, limit, filter_account_id, viewer_id
        )
        .fetch_all(&state.db)
        .await?;

        let mut result: Vec<Status> = Vec::with_capacity(rows.len());
        for s in &rows {
            let account = sqlx::query_as!(
                crate::db::models::Account,
                "SELECT * FROM accounts WHERE id = $1",
                s.account_id
            )
            .fetch_one(&state.db)
            .await?;
            let media = fetch_status_media(&state, s.id).await?;
            let reblog = fetch_reblog_data(&state, s).await?;
            let ctx = if let Some(vid) = viewer_id {
                let favourited = sqlx::query!(
                    "SELECT 1 as e FROM favourites WHERE account_id = $1 AND status_id = $2",
                    vid, s.id
                )
                .fetch_optional(&state.db)
                .await?
                .is_some();
                let reblogged = sqlx::query!(
                    "SELECT 1 as e FROM statuses WHERE account_id = $1 AND reblog_of_id = $2 AND deleted_at IS NULL",
                    vid, s.id
                )
                .fetch_optional(&state.db)
                .await?
                .is_some();
                let bookmarked = sqlx::query!(
                    "SELECT 1 as e FROM bookmarks WHERE account_id = $1 AND status_id = $2",
                    vid, s.id
                )
                .fetch_optional(&state.db)
                .await?
                .is_some();
                Some(super::convert::StatusViewerContext { account_id: vid, favourited, reblogged, muted: false, bookmarked, pinned: false })
            } else {
                None
            };
            result.push(build_status(&state, s, &account, media, reblog, ctx).await?);
        }
        result
    } else {
        vec![]
    };

    let hashtags = if search_type.is_none() || search_type == Some("hashtags") {
        sqlx::query!(
            "SELECT id, name FROM tags WHERE lower(name) LIKE $1 ORDER BY name LIMIT $2",
            pattern, limit
        )
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|r| Tag {
            id: r.id.to_string(),
            name: r.name.clone(),
            url: format!("https://{}/tags/{}", instance.domain, r.name),
            history: vec![],
            following: None,
            featuring: None,
        })
        .collect()
    } else {
        vec![]
    };

    Ok(Json(SearchResults { accounts, statuses, hashtags }))
}
