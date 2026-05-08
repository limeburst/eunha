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
    accounts::fetch_status_media,
    convert::{account_from_db, status_from_db},
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

    let accounts = if search_type.is_none() || search_type == Some("accounts") {
        sqlx::query_as!(
            crate::db::models::Account,
            r#"SELECT * FROM accounts
               WHERE instance_id = $1
                 AND (lower(username) LIKE $2 OR lower(display_name) LIKE $2)
               ORDER BY followers_count DESC LIMIT $3"#,
            instance.id, pattern, limit
        )
        .fetch_all(&state.db)
        .await?
        .iter()
        .map(account_from_db)
        .collect()
    } else {
        vec![]
    };

    let statuses = if (search_type.is_none() || search_type == Some("statuses")) && auth.is_some() {
        let viewer_id = auth.as_ref().map(|Extension(a)| a.account_id);
        let rows = sqlx::query_as!(
            crate::db::models::Status,
            r#"SELECT * FROM statuses
               WHERE instance_id = $1
                 AND deleted_at IS NULL
                 AND visibility IN ('public', 'unlisted')
                 AND lower(content) LIKE $2
               ORDER BY id DESC LIMIT $3"#,
            instance.id, pattern, limit
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
                Some(super::convert::StatusViewerContext { favourited, reblogged, muted: false, bookmarked })
            } else {
                None
            };
            result.push(status_from_db(s, &account, media, None, ctx));
        }
        result
    } else {
        vec![]
    };

    let hashtags = if search_type.is_none() || search_type == Some("hashtags") {
        sqlx::query!(
            "SELECT name FROM tags WHERE lower(name) LIKE $1 ORDER BY name LIMIT $2",
            pattern, limit
        )
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|r| Tag {
            name: r.name.clone(),
            url: format!("https://{}/tags/{}", instance.domain, r.name),
            history: vec![],
            following: None,
        })
        .collect()
    } else {
        vec![]
    };

    Ok(Json(SearchResults { accounts, statuses, hashtags }))
}
