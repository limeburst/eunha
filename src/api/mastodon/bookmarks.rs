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
    accounts::{batch_reblog_data, batch_status_emojis, batch_status_media, batch_status_mentions, batch_status_tags, build_status},
    convert::status_from_db,
    types::PaginationParams,
};

// ── GET /api/v1/bookmarks ─────────────────────────────────────────────────

pub async fn get_bookmarks(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    uri: Uri,
    req_headers: HeaderMap,
    Query(q): Query<PaginationParams>,
) -> AppResult<impl IntoResponse> {
    auth.require_scope("read:bookmarks")?;
    let limit = q.limit_clamped(20, 40);
    let max_id = q.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = q.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = q.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let statuses: Vec<crate::db::models::Status> = if min_id.is_some() {
        sqlx::query_as!(
            crate::db::models::Status,
            r#"SELECT s.* FROM statuses s
               JOIN bookmarks b ON b.status_id = s.id
               WHERE b.account_id = $1
                 AND s.deleted_at IS NULL
                 AND ($2::bigint IS NULL OR s.id > $2)
               ORDER BY s.id ASC LIMIT $3"#,
            auth.account_id, min_id, limit
        )
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as!(
            crate::db::models::Status,
            r#"SELECT s.* FROM statuses s
               JOIN bookmarks b ON b.status_id = s.id
               WHERE b.account_id = $1
                 AND s.deleted_at IS NULL
                 AND ($2::bigint IS NULL OR s.id < $2)
                 AND ($3::bigint IS NULL OR s.id > $3)
               ORDER BY b.created_at DESC LIMIT $4"#,
            auth.account_id, max_id, since_id, limit
        )
        .fetch_all(&state.db)
        .await?
    };

    if statuses.is_empty() {
        return Ok((HeaderMap::new(), Json(vec![])));
    }

    let all_ids: Vec<i64> = statuses.iter().map(|s| s.id).collect();
    let media_map = batch_status_media(&state, &all_ids).await?;
    let reblog_map = batch_reblog_data(&state, &statuses).await?;
    let reblog_ids: Vec<i64> = reblog_map.values().map(|(rs, _, _)| rs.id).collect();
    let mut enrich_ids = all_ids.clone();
    enrich_ids.extend_from_slice(&reblog_ids);
    let tags_map = batch_status_tags(&state, &enrich_ids).await?;
    let mentions_map = batch_status_mentions(&state, &enrich_ids).await?;
    let all_statuses_for_emoji: Vec<crate::db::models::Status> = statuses.iter().cloned()
        .chain(reblog_map.values().map(|(rs, _, _)| rs.clone()))
        .collect();
    let emojis_map = batch_status_emojis(&state, &all_statuses_for_emoji).await?;
    let ctxs = super::statuses::batch_viewer_contexts(&state, auth.account_id, &all_ids).await?;

    let accounts: Vec<crate::db::models::Account> = {
        let account_ids: Vec<i64> = statuses.iter().map(|s| s.account_id)
            .collect::<std::collections::HashSet<_>>().into_iter().collect();
        sqlx::query_as!(
            crate::db::models::Account,
            "SELECT * FROM accounts WHERE id = ANY($1::bigint[])",
            &account_ids,
        )
        .fetch_all(&state.db)
        .await?
    };
    let account_map: std::collections::HashMap<i64, crate::db::models::Account> =
        accounts.into_iter().map(|a| (a.id, a)).collect();

    let mut result = Vec::with_capacity(statuses.len());
    for s in &statuses {
        let Some(account) = account_map.get(&s.account_id) else { continue };
        let media = media_map.get(&s.id).cloned().unwrap_or_default();
        let reblog = reblog_map.get(&s.id).cloned();
        let mentions = mentions_map.get(&s.id).cloned().unwrap_or_default();
        let rb_mentions = reblog.as_ref()
            .and_then(|(rs, _, _)| mentions_map.get(&rs.id))
            .cloned()
            .unwrap_or_default();
        let mut ctx = ctxs.get(&s.id).cloned().unwrap_or(super::convert::StatusViewerContext {
            account_id: auth.account_id,
            favourited: false,
            reblogged: false,
            muted: false,
            bookmarked: true,
            pinned: false,
        });
        ctx.bookmarked = true;
        let mut api = status_from_db(s, account, media, reblog, Some(ctx), &mentions, &rb_mentions);
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
