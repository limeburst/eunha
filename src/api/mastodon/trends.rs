use axum::{
    extract::{Extension, Query, State},
    Json,
};
use serde::Deserialize;

use crate::{
    error::AppResult,
    middleware::ResolvedInstance,
    state::AppState,
};
use super::{
    accounts::{build_status, fetch_reblog_data, fetch_status_media},
    types::{Status, Tag, TagHistory},
};

#[derive(Debug, Deserialize)]
pub struct TrendParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// ── GET /api/v1/trends/tags  &  GET /api/v1/trends ────────────────────────

pub async fn trending_tags(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Query(params): Query<TrendParams>,
) -> AppResult<Json<Vec<Tag>>> {
    let limit = params.limit.unwrap_or(10).min(20).max(1);
    let domain = instance.custom_domain.as_deref().unwrap_or(&instance.domain).to_string();

    let rows = sqlx::query!(
        r#"SELECT t.id, t.name,
                  COUNT(DISTINCT st.status_id) FILTER (
                      WHERE s.created_at > now() - interval '1 day'
                  ) AS day_uses,
                  COUNT(DISTINCT st.status_id) FILTER (
                      WHERE s.created_at > now() - interval '7 days'
                  ) AS week_uses
           FROM tags t
           JOIN status_tags st ON st.tag_id = t.id
           JOIN statuses s ON s.id = st.status_id
           WHERE s.instance_id = $1
             AND s.deleted_at IS NULL
             AND s.visibility = 'public'
             AND s.created_at > now() - interval '7 days'
           GROUP BY t.id, t.name
           HAVING COUNT(DISTINCT st.status_id) > 1
           ORDER BY day_uses DESC, week_uses DESC
           LIMIT $2"#,
        instance.id,
        limit,
    )
    .fetch_all(&state.db)
    .await?;

    let tags = rows
        .into_iter()
        .map(|r| Tag {
            id: r.id.to_string(),
            name: r.name.clone(),
            url: format!("https://{}/tags/{}", domain, r.name),
            history: vec![
                TagHistory {
                    day: chrono::Utc::now().timestamp().to_string(),
                    uses: r.day_uses.unwrap_or(0).to_string(),
                    accounts: r.day_uses.unwrap_or(0).to_string(),
                },
                TagHistory {
                    day: (chrono::Utc::now() - chrono::Duration::days(7)).timestamp().to_string(),
                    uses: r.week_uses.unwrap_or(0).to_string(),
                    accounts: r.week_uses.unwrap_or(0).to_string(),
                },
            ],
            following: None,
            featuring: None,
        })
        .collect();

    Ok(Json(tags))
}

// ── GET /api/v1/trends/statuses ───────────────────────────────────────────

pub async fn trending_statuses(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Query(params): Query<TrendParams>,
    auth: Option<Extension<crate::middleware::AuthenticatedUser>>,
) -> AppResult<Json<Vec<Status>>> {
    let limit = params.limit.unwrap_or(20).min(40).max(1);
    let viewer_id = auth.map(|Extension(a)| a.account_id);

    let rows = sqlx::query_as!(
        crate::db::models::Status,
        r#"SELECT * FROM statuses
           WHERE instance_id = $1
             AND deleted_at IS NULL
             AND visibility = 'public'
             AND reblog_of_id IS NULL
             AND created_at > now() - interval '2 days'
           ORDER BY (favourites_count + reblogs_count * 2) DESC, created_at DESC
           LIMIT $2"#,
        instance.id,
        limit,
    )
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::with_capacity(rows.len());
    for s in &rows {
        let account = sqlx::query_as!(
            crate::db::models::Account,
            "SELECT * FROM accounts WHERE id = $1",
            s.account_id,
        )
        .fetch_one(&state.db)
        .await?;
        let media = fetch_status_media(&state, s.id).await?;
        let reblog = fetch_reblog_data(&state, s).await?;
        let ctx = if let Some(vid) = viewer_id {
            let favourited = sqlx::query_scalar!(
                "SELECT 1 AS e FROM favourites WHERE account_id = $1 AND status_id = $2",
                vid, s.id
            ).fetch_optional(&state.db).await?.is_some();
            let reblogged = sqlx::query_scalar!(
                "SELECT 1 AS e FROM statuses WHERE account_id = $1 AND reblog_of_id = $2 AND deleted_at IS NULL",
                vid, s.id
            ).fetch_optional(&state.db).await?.is_some();
            let bookmarked = sqlx::query_scalar!(
                "SELECT 1 AS e FROM bookmarks WHERE account_id = $1 AND status_id = $2",
                vid, s.id
            ).fetch_optional(&state.db).await?.is_some();
            Some(super::convert::StatusViewerContext { favourited, reblogged, muted: false, bookmarked, pinned: false })
        } else {
            None
        };
        result.push(build_status(&state, s, &account, media, reblog, ctx).await?);
    }

    Ok(Json(result))
}

// ── GET /api/v1/trends/links ──────────────────────────────────────────────

pub async fn trending_links() -> Json<Vec<serde_json::Value>> {
    Json(vec![])
}
