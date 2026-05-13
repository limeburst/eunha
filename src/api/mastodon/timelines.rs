use axum::{
    extract::{Extension, Path, Query, State},
    http::{header, HeaderMap, Uri},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use crate::{
    db::models::{Account, Status as DbStatus},
    error::{AppError, AppResult},
    middleware::{AuthenticatedUser, ResolvedInstance},
    state::AppState,
};
use super::{
    accounts::{batch_reblog_data, batch_status_media, batch_status_mentions, batch_status_tags},
    convert::status_from_db,
    types::{PaginationParams, Status},
};

#[derive(Debug, Deserialize)]
pub struct PublicTimelineQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    pub local: Option<bool>,
    pub remote: Option<bool>,
    pub only_media: Option<bool>,
}

// ── GET /api/v1/timelines/public ──────────────────────────────────────────

pub async fn public_timeline(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    uri: Uri,
    req_headers: HeaderMap,
    Query(q): Query<PublicTimelineQuery>,
) -> AppResult<impl IntoResponse> {
    let limit = q.pagination.limit_clamped(20, 40);
    let max_id = q.pagination.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = q.pagination.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = q.pagination.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let local_only = q.local.unwrap_or(false);
    let remote_only = q.remote.unwrap_or(false);

    // min_id: return oldest items just after min_id (ASC); else DESC
    let statuses = if min_id.is_some() {
        sqlx::query_as!(
            DbStatus,
            r#"SELECT s.*
               FROM statuses s
               JOIN accounts a ON a.id = s.account_id
               WHERE s.visibility = 'public'
                 AND s.deleted_at IS NULL
                 AND s.reblog_of_id IS NULL
                 AND s.instance_id = $2
                 AND (NOT $1::bool OR a.domain IS NULL)
                 AND (NOT $5::bool OR a.domain IS NOT NULL)
                 AND a.suspended_at IS NULL
                 AND (a.domain IS NULL OR NOT EXISTS (
                     SELECT 1 FROM domain_blocks db WHERE db.domain = a.domain
                 ))
                 AND ($3::bigint IS NULL OR s.id > $3)
                 AND (s.text != '' OR s.content != ''
                      OR EXISTS (SELECT 1 FROM media_attachments WHERE status_id = s.id))
               ORDER BY s.id ASC
               LIMIT $4"#,
            local_only,
            instance.id,
            min_id,
            limit,
            remote_only,
        )
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as!(
            DbStatus,
            r#"SELECT s.*
               FROM statuses s
               JOIN accounts a ON a.id = s.account_id
               WHERE s.visibility = 'public'
                 AND s.deleted_at IS NULL
                 AND s.reblog_of_id IS NULL
                 AND s.instance_id = $2
                 AND (NOT $1::bool OR a.domain IS NULL)
                 AND (NOT $6::bool OR a.domain IS NOT NULL)
                 AND a.suspended_at IS NULL
                 AND (a.domain IS NULL OR NOT EXISTS (
                     SELECT 1 FROM domain_blocks db WHERE db.domain = a.domain
                 ))
                 AND ($3::bigint IS NULL OR s.id < $3)
                 AND ($5::bigint IS NULL OR s.id > $5)
                 AND (s.text != '' OR s.content != ''
                      OR EXISTS (SELECT 1 FROM media_attachments WHERE status_id = s.id))
               ORDER BY s.id DESC
               LIMIT $4"#,
            local_only,
            instance.id,
            max_id,
            limit,
            since_id,
            remote_only,
        )
        .fetch_all(&state.db)
        .await?
    };

    let result = build_status_list(&state, statuses, None).await?;
    let resp = with_pagination_link(&req_headers, &uri, result);
    Ok(resp)
}

// ── GET /api/v1/timelines/home ────────────────────────────────────────────

pub async fn home_timeline(
    State(state): State<AppState>,
    uri: Uri,
    req_headers: HeaderMap,
    Query(q): Query<PaginationParams>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<impl IntoResponse> {
    let limit = q.limit_clamped(20, 40);
    let max_id = q.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = q.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = q.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let statuses = if min_id.is_some() {
        sqlx::query_as!(
            DbStatus,
            r#"SELECT s.*
               FROM statuses s
               JOIN accounts a ON a.id = s.account_id
               WHERE s.account_id IN (
                   SELECT target_account_id FROM follows
                   WHERE account_id = $1 AND state = 'accepted'
                   UNION ALL SELECT $1
               )
               AND s.deleted_at IS NULL
               AND a.suspended_at IS NULL
               AND (a.domain IS NULL OR NOT EXISTS (
                   SELECT 1 FROM domain_blocks db WHERE db.domain = a.domain
               ))
               AND ($2::bigint IS NULL OR s.id > $2)
               AND (s.text != '' OR s.content != ''
                    OR s.reblog_of_id IS NOT NULL
                    OR EXISTS (SELECT 1 FROM media_attachments WHERE status_id = s.id))
               ORDER BY s.id ASC
               LIMIT $3"#,
            auth.account_id,
            min_id,
            limit,
        )
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as!(
            DbStatus,
            r#"SELECT s.*
               FROM statuses s
               JOIN accounts a ON a.id = s.account_id
               WHERE s.account_id IN (
                   SELECT target_account_id FROM follows
                   WHERE account_id = $1 AND state = 'accepted'
                   UNION ALL SELECT $1
               )
               AND s.deleted_at IS NULL
               AND a.suspended_at IS NULL
               AND (a.domain IS NULL OR NOT EXISTS (
                   SELECT 1 FROM domain_blocks db WHERE db.domain = a.domain
               ))
               AND ($2::bigint IS NULL OR s.id < $2)
               AND ($3::bigint IS NULL OR s.id > $3)
               AND (s.text != '' OR s.content != ''
                    OR s.reblog_of_id IS NOT NULL
                    OR EXISTS (SELECT 1 FROM media_attachments WHERE status_id = s.id))
               ORDER BY s.id DESC
               LIMIT $4"#,
            auth.account_id,
            max_id,
            since_id,
            limit,
        )
        .fetch_all(&state.db)
        .await?
    };

    let result = build_status_list(&state, statuses, Some(auth.account_id)).await?;
    let resp = with_pagination_link(&req_headers, &uri, result);
    Ok(resp)
}

// ── GET /api/v1/timelines/list/:id ───────────────────────────────────────

pub async fn list_timeline(
    State(state): State<AppState>,
    Path(list_id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
    uri: Uri,
    req_headers: HeaderMap,
    Query(q): Query<PaginationParams>,
) -> AppResult<impl IntoResponse> {
    sqlx::query!(
        "SELECT id FROM lists WHERE id = $1 AND account_id = $2",
        list_id, auth.account_id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let limit = q.limit_clamped(20, 40);
    let max_id = q.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = q.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = q.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let statuses = if min_id.is_some() {
        sqlx::query_as!(
            DbStatus,
            r#"SELECT s.* FROM statuses s
               JOIN list_accounts la ON la.account_id = s.account_id
               WHERE la.list_id = $1
                 AND s.deleted_at IS NULL
                 AND ($2::bigint IS NULL OR s.id > $2)
                 AND (s.text != '' OR s.content != ''
                      OR s.reblog_of_id IS NOT NULL
                      OR EXISTS (SELECT 1 FROM media_attachments WHERE status_id = s.id))
               ORDER BY s.id ASC
               LIMIT $3"#,
            list_id, min_id, limit,
        )
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as!(
            DbStatus,
            r#"SELECT s.* FROM statuses s
               JOIN list_accounts la ON la.account_id = s.account_id
               WHERE la.list_id = $1
                 AND s.deleted_at IS NULL
                 AND ($2::bigint IS NULL OR s.id < $2)
                 AND ($3::bigint IS NULL OR s.id > $3)
                 AND (s.text != '' OR s.content != ''
                      OR s.reblog_of_id IS NOT NULL
                      OR EXISTS (SELECT 1 FROM media_attachments WHERE status_id = s.id))
               ORDER BY s.id DESC
               LIMIT $4"#,
            list_id, max_id, since_id, limit,
        )
        .fetch_all(&state.db)
        .await?
    };

    let result = build_status_list(&state, statuses, Some(auth.account_id)).await?;
    let resp = with_pagination_link(&req_headers, &uri, result);
    Ok(resp)
}

// ── GET /api/v1/timelines/tag/:hashtag ───────────────────────────────────

pub async fn tag_timeline(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Path(hashtag): Path<String>,
    uri: Uri,
    req_headers: HeaderMap,
    Query(q): Query<PaginationParams>,
) -> AppResult<impl IntoResponse> {
    let limit = q.limit_clamped(20, 40);
    let max_id = q.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = q.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = q.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let tag_name = hashtag.to_lowercase();

    let statuses = if min_id.is_some() {
        sqlx::query_as!(
            DbStatus,
            r#"SELECT s.* FROM statuses s
               JOIN status_tags st ON st.status_id = s.id
               JOIN tags t ON t.id = st.tag_id
               WHERE lower(t.name) = $1
                 AND s.instance_id = $2
                 AND s.visibility = 'public'
                 AND s.deleted_at IS NULL
                 AND ($3::bigint IS NULL OR s.id > $3)
               ORDER BY s.id ASC
               LIMIT $4"#,
            tag_name, instance.id, min_id, limit,
        )
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as!(
            DbStatus,
            r#"SELECT s.* FROM statuses s
               JOIN status_tags st ON st.status_id = s.id
               JOIN tags t ON t.id = st.tag_id
               WHERE lower(t.name) = $1
                 AND s.instance_id = $2
                 AND s.visibility = 'public'
                 AND s.deleted_at IS NULL
                 AND ($3::bigint IS NULL OR s.id < $3)
                 AND ($4::bigint IS NULL OR s.id > $4)
               ORDER BY s.id DESC
               LIMIT $5"#,
            tag_name, instance.id, max_id, since_id, limit,
        )
        .fetch_all(&state.db)
        .await?
    };

    let result = build_status_list(&state, statuses, None).await?;
    let resp = with_pagination_link(&req_headers, &uri, result);
    Ok(resp)
}

// ── Helpers ────────────────────────────────────────────────────────────────

async fn build_status_list(
    state: &AppState,
    statuses: Vec<DbStatus>,
    viewer_id: Option<uuid::Uuid>,
) -> AppResult<Vec<Status>> {
    // For reblogs, check viewer context against the original status.
    let effective_ids: Vec<i64> = statuses.iter()
        .map(|s| s.reblog_of_id.unwrap_or(s.id))
        .collect();

    let ctxs = if let Some(vid) = viewer_id {
        super::statuses::batch_viewer_contexts(state, vid, &effective_ids).await?
    } else {
        std::collections::HashMap::new()
    };

    if statuses.is_empty() {
        return Ok(vec![]);
    }

    let account_ids: Vec<uuid::Uuid> = statuses.iter()
        .map(|s| s.account_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let accounts = sqlx::query_as!(
        Account,
        "SELECT * FROM accounts WHERE id = ANY($1::uuid[])",
        &account_ids,
    )
    .fetch_all(&state.db)
    .await?;
    let account_map: std::collections::HashMap<uuid::Uuid, Account> = accounts
        .into_iter()
        .map(|a| (a.id, a))
        .collect();

    let all_status_ids: Vec<i64> = statuses.iter().map(|s| s.id).collect();
    let media_map = batch_status_media(state, &all_status_ids).await?;
    let reblog_map = batch_reblog_data(state, &statuses).await?;
    let reblog_ids: Vec<i64> = reblog_map.values().map(|(rs, _, _)| rs.id).collect();
    let mut enrich_ids = all_status_ids.clone();
    enrich_ids.extend_from_slice(&reblog_ids);
    let tags_map = batch_status_tags(state, &enrich_ids).await?;
    let mentions_map = batch_status_mentions(state, &enrich_ids).await?;

    let mut result = Vec::with_capacity(statuses.len());
    for s in &statuses {
        let account = account_map.get(&s.account_id).ok_or(AppError::NotFound)?;
        let media = media_map.get(&s.id).cloned().unwrap_or_default();
        let reblog = reblog_map.get(&s.id).cloned();
        let effective_id = s.reblog_of_id.unwrap_or(s.id);
        let ctx = ctxs.get(&effective_id).cloned();
        let mut api = status_from_db(s, account, media, reblog, ctx);
        api.tags = tags_map.get(&s.id).cloned().unwrap_or_default();
        api.mentions = mentions_map.get(&s.id).cloned().unwrap_or_default();
        if let Some(ref mut rb) = api.reblog {
            let rid: i64 = rb.id.parse().unwrap_or(0);
            rb.tags = tags_map.get(&rid).cloned().unwrap_or_default();
            rb.mentions = mentions_map.get(&rid).cloned().unwrap_or_default();
        }
        result.push(api);
    }
    Ok(result)
}

fn with_pagination_link(
    req_headers: &HeaderMap,
    uri: &Uri,
    statuses: Vec<Status>,
) -> impl IntoResponse {
    let link = statuses.first().zip(statuses.last()).map(|(newest, oldest)| {
        let extra = super::non_pagination_query(uri.query());
        super::link_header(req_headers, uri.path(), &extra, &newest.id, &oldest.id)
    });
    let mut headers = HeaderMap::new();
    if let Some(v) = link {
        if let Ok(val) = v.parse() {
            headers.insert(header::LINK, val);
        }
    }
    (headers, Json(statuses))
}
