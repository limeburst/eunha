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

    // Get top trending tag IDs ordered by usage in last 7 days
    let top_tags = sqlx::query!(
        r#"SELECT t.id, t.name,
                  COUNT(DISTINCT st.status_id) FILTER (WHERE s.created_at > now() - interval '1 day') AS day_uses,
                  COUNT(DISTINCT st.status_id) AS week_uses
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

    if top_tags.is_empty() {
        return Ok(Json(vec![]));
    }

    let tag_ids: Vec<uuid::Uuid> = top_tags.iter().map(|r| r.id).collect();

    // Fetch 7-day per-day breakdown for all returned tags in one query
    let history_rows = sqlx::query!(
        r#"SELECT st.tag_id,
                  date_trunc('day', s.created_at)::timestamptz AS day,
                  COUNT(DISTINCT st.status_id) AS uses,
                  COUNT(DISTINCT s.account_id) AS accounts
           FROM status_tags st
           JOIN statuses s ON s.id = st.status_id
           WHERE st.tag_id = ANY($1::uuid[])
             AND s.created_at >= now() - interval '7 days'
             AND s.deleted_at IS NULL
           GROUP BY st.tag_id, date_trunc('day', s.created_at)
           ORDER BY day DESC"#,
        &tag_ids,
    )
    .fetch_all(&state.db)
    .await?;

    // Build per-tag history map
    let mut history_map: std::collections::HashMap<uuid::Uuid, Vec<TagHistory>> =
        std::collections::HashMap::new();
    for row in history_rows {
        let day_ts = row.day.map(|d| d.timestamp()).unwrap_or(0).to_string();
        history_map.entry(row.tag_id).or_default().push(TagHistory {
            day: day_ts,
            uses: row.uses.unwrap_or(0).to_string(),
            accounts: row.accounts.unwrap_or(0).to_string(),
        });
    }

    let tags = top_tags
        .into_iter()
        .map(|r| Tag {
            id: r.id.to_string(),
            name: r.name.clone(),
            url: format!("https://{}/tags/{}", domain, r.name),
            history: history_map.remove(&r.id).unwrap_or_default(),
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
            Some(super::convert::StatusViewerContext { account_id: vid, favourited, reblogged, muted: false, bookmarked, pinned: false })
        } else {
            None
        };
        result.push(build_status(&state, s, &account, media, reblog, ctx).await?);
    }

    Ok(Json(result))
}

// ── GET /api/v1/trends/links ──────────────────────────────────────────────

pub async fn trending_links(
    State(state): State<AppState>,
    Query(params): Query<TrendParams>,
) -> AppResult<Json<Vec<super::types::PreviewCard>>> {
    let limit = params.limit.unwrap_or(10).min(40).max(1) as i64;
    let offset = params.offset.unwrap_or(0).max(0) as i64;

    let rows = sqlx::query!(
        r#"SELECT pc.url, pc.title, pc.description, pc.card_type,
                  pc.image_url, pc.author_name, pc.author_url,
                  pc.provider_name, pc.provider_url, pc.html,
                  pc.width, pc.height, pc.embed_url, pc.blurhash,
                  COUNT(spc.status_id) AS uses
           FROM preview_cards pc
           JOIN status_preview_cards spc ON spc.card_id = pc.id
           JOIN statuses s ON s.id = spc.status_id
           WHERE s.deleted_at IS NULL
             AND s.created_at > now() - interval '2 days'
             AND s.visibility = 'public'
           GROUP BY pc.id
           ORDER BY uses DESC
           LIMIT $1 OFFSET $2"#,
        limit,
        offset,
    )
    .fetch_all(&state.db)
    .await?;

    let cards = rows.into_iter().map(|r| super::types::PreviewCard {
        url: r.url,
        title: r.title,
        description: r.description,
        language: None,
        card_type: r.card_type,
        author_name: r.author_name,
        author_url: r.author_url,
        provider_name: r.provider_name,
        provider_url: r.provider_url,
        html: r.html,
        width: r.width,
        height: r.height,
        embed_url: r.embed_url,
        image: r.image_url,
        image_description: String::new(),
        blurhash: r.blurhash,
        published_at: None,
        authors: vec![],
    }).collect();

    Ok(Json(cards))
}
