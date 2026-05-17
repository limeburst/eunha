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
    accounts::{batch_reblog_data, batch_status_emojis, batch_status_media, batch_status_mentions, batch_status_tags},
    convert::status_from_db,
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
    let offset = params.offset.unwrap_or(0).max(0);
    let domain = instance.custom_domain.as_deref().unwrap_or(&instance.domain);

    // Tags with most status uses in the last 7 days
    let rows = sqlx::query!(
        r#"SELECT t.id, t.name, COUNT(st.status_id) AS uses
           FROM tags t
           JOIN status_tags st ON st.tag_id = t.id
           JOIN statuses s ON s.id = st.status_id
           WHERE s.instance_id = $1
             AND s.deleted_at IS NULL
             AND s.visibility = 'public'
             AND s.created_at > now() - interval '7 days'
           GROUP BY t.id, t.name
           ORDER BY uses DESC, t.name ASC
           LIMIT $2 OFFSET $3"#,
        instance.id, limit, offset,
    )
    .fetch_all(&state.db)
    .await?;

    let tags: Vec<Tag> = rows
        .into_iter()
        .map(|r| {
            let name_lower = r.name.to_lowercase();
            Tag {
                id: r.id.to_string(),
                name: r.name,
                url: format!("https://{}/tags/{}", domain, urlencoding::encode(&name_lower)),
                history: vec![TagHistory { day: "0".into(), uses: r.uses.unwrap_or(0).to_string(), accounts: "0".into() }],
                following: None,
                featuring: None,
            }
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

    if rows.is_empty() {
        return Ok(Json(vec![]));
    }

    let all_ids: Vec<i64> = rows.iter().map(|s| s.id).collect();
    let media_map = batch_status_media(&state, &all_ids).await?;
    let reblog_map = batch_reblog_data(&state, &rows).await?;
    let reblog_ids: Vec<i64> = reblog_map.values().map(|(rs, _, _)| rs.id).collect();
    let mut enrich_ids = all_ids.clone();
    enrich_ids.extend_from_slice(&reblog_ids);
    let tags_map = batch_status_tags(&state, &enrich_ids).await?;
    let mentions_map = batch_status_mentions(&state, &enrich_ids).await?;
    let all_statuses_for_emoji: Vec<crate::db::models::Status> = rows.iter().cloned()
        .chain(reblog_map.values().map(|(rs, _, _)| rs.clone()))
        .collect();
    let emojis_map = batch_status_emojis(&state, &all_statuses_for_emoji).await?;
    let ctxs = if let Some(vid) = viewer_id {
        super::statuses::batch_viewer_contexts(&state, vid, &all_ids).await?
    } else {
        std::collections::HashMap::new()
    };

    let account_ids: Vec<i64> = rows.iter().map(|s| s.account_id)
        .collect::<std::collections::HashSet<_>>().into_iter().collect();
    let accounts: Vec<crate::db::models::Account> = sqlx::query_as!(
        crate::db::models::Account,
        "SELECT * FROM accounts WHERE id = ANY($1::bigint[])",
        &account_ids,
    )
    .fetch_all(&state.db)
    .await?;
    let account_map: std::collections::HashMap<i64, crate::db::models::Account> =
        accounts.into_iter().map(|a| (a.id, a)).collect();

    let mut result = Vec::with_capacity(rows.len());
    for s in &rows {
        let Some(account) = account_map.get(&s.account_id) else { continue };
        let media = media_map.get(&s.id).cloned().unwrap_or_default();
        let reblog = reblog_map.get(&s.id).cloned();
        let mentions = mentions_map.get(&s.id).cloned().unwrap_or_default();
        let rb_mentions = reblog.as_ref()
            .and_then(|(rs, _, _)| mentions_map.get(&rs.id))
            .cloned()
            .unwrap_or_default();
        let ctx = ctxs.get(&s.id).cloned();
        let mut api = status_from_db(s, account, media, reblog, ctx, &mentions, &rb_mentions);
        api.tags = tags_map.get(&s.id).cloned().unwrap_or_default();
        api.mentions = mentions;
        api.emojis = emojis_map.get(&s.id).cloned().unwrap_or_default();
        if let Some(ref mut rb) = api.reblog {
            let rid: i64 = rb.id.parse().unwrap_or(0);
            rb.tags = tags_map.get(&rid).cloned().unwrap_or_default();
            rb.mentions = rb_mentions;
            rb.emojis = emojis_map.get(&rid).cloned().unwrap_or_default();
        }
        result.push(api);
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
                  pc.author_name, pc.author_url, pc.provider_name, pc.provider_url,
                  pc.html, pc.width, pc.height, pc.image_url, pc.embed_url, pc.blurhash,
                  COUNT(s.id) AS uses
           FROM preview_cards pc
           JOIN status_preview_cards spc ON spc.card_id = pc.id
           JOIN statuses s ON s.id = spc.status_id
           WHERE s.deleted_at IS NULL
             AND s.visibility = 'public'
             AND s.created_at > now() - interval '7 days'
           GROUP BY pc.url, pc.title, pc.description, pc.card_type,
                    pc.author_name, pc.author_url, pc.provider_name, pc.provider_url,
                    pc.html, pc.width, pc.height, pc.image_url, pc.embed_url, pc.blurhash
           ORDER BY uses DESC
           LIMIT $1 OFFSET $2"#,
        limit, offset,
    )
    .fetch_all(&state.db)
    .await?;

    let cards = rows.into_iter().map(|r| super::types::PreviewCard {
        url: r.url,
        title: r.title,
        description: r.description,
        card_type: r.card_type,
        author_name: r.author_name,
        author_url: r.author_url,
        provider_name: r.provider_name,
        provider_url: r.provider_url,
        html: r.html,
        width: r.width,
        height: r.height,
        image: r.image_url,
        embed_url: r.embed_url,
        blurhash: r.blurhash,
        language: None,
        published_at: None,
        authors: vec![],
        image_description: String::new(),
    }).collect();

    Ok(Json(cards))
}
