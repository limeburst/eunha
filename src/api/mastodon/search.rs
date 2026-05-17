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
    accounts::{batch_reblog_data, batch_status_emojis, batch_status_media, batch_status_mentions, batch_status_tags, build_status, fetch_reblog_data, fetch_status_media},
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
    let viewer_id = auth.as_ref().map(|Extension(a)| a.account_id);

    // URL-based lookup: if the query looks like a URL, try matching by uri/url first
    if q.q.starts_with("http://") || q.q.starts_with("https://") {
        let url = q.q.trim();
        // Try to find a status with this URL or URI
        if search_type.is_none() || search_type == Some("statuses") {
            if let Ok(Some(s)) = sqlx::query_as!(
                crate::db::models::Status,
                "SELECT * FROM statuses WHERE (uri = $1 OR url = $1) AND deleted_at IS NULL LIMIT 1",
                url,
            )
            .fetch_optional(&state.db)
            .await {
                let account = sqlx::query_as!(
                    crate::db::models::Account,
                    "SELECT * FROM accounts WHERE id = $1",
                    s.account_id
                )
                .fetch_one(&state.db)
                .await?;
                let media = fetch_status_media(&state, s.id).await?;
                let reblog = fetch_reblog_data(&state, &s).await?;
                let status = build_status(&state, &s, &account, media, reblog, None).await?;
                return Ok(Json(SearchResults { accounts: vec![], statuses: vec![status], hashtags: vec![] }));
            }
        }
        // Try to find an account with this URL or URI
        if search_type.is_none() || search_type == Some("accounts") {
            if let Ok(Some(a)) = sqlx::query_as!(
                crate::db::models::Account,
                "SELECT * FROM accounts WHERE (uri = $1 OR url = $1) AND suspended_at IS NULL LIMIT 1",
                url,
            )
            .fetch_optional(&state.db)
            .await {
                return Ok(Json(SearchResults { accounts: vec![account_from_db(&a)], statuses: vec![], hashtags: vec![] }));
            }
        }
    }

    let offset = q.offset.unwrap_or(0).max(0);

    // Detect @user@domain or user@domain handle patterns for exact acct lookup
    let handle_parts: Option<(String, Option<String>)> = {
        let trimmed = q.q.trim().trim_start_matches('@');
        if trimmed.contains('@') {
            let mut parts = trimmed.splitn(2, '@');
            let user = parts.next().unwrap_or("").to_lowercase();
            let dom = parts.next().map(|d| d.to_lowercase());
            if !user.is_empty() { Some((user, dom)) } else { None }
        } else {
            None
        }
    };

    let accounts = if search_type.is_none() || search_type == Some("accounts") {
        let following_filter = q.following.unwrap_or(false);

        // If query is a handle (user@domain), do an exact acct lookup first
        if let Some((ref uname, ref domain)) = handle_parts {
            let exact: Vec<crate::db::models::Account> = if let Some(dom) = domain {
                sqlx::query_as!(
                    crate::db::models::Account,
                    r#"SELECT * FROM accounts
                       WHERE instance_id = $1 AND suspended_at IS NULL
                         AND lower(username) = $2 AND lower(domain) = $3
                       LIMIT $4"#,
                    instance.id, uname, dom, limit
                )
                .fetch_all(&state.db)
                .await?
            } else {
                sqlx::query_as!(
                    crate::db::models::Account,
                    r#"SELECT * FROM accounts
                       WHERE instance_id = $1 AND suspended_at IS NULL
                         AND lower(username) = $2 AND domain IS NULL
                       LIMIT $3"#,
                    instance.id, uname, limit
                )
                .fetch_all(&state.db)
                .await?
            };
            if !exact.is_empty() {
                exact.iter().map(account_from_db).collect()
            } else if following_filter {
                let vid = viewer_id.ok_or(crate::error::AppError::Unauthorized)?;
                sqlx::query_as!(
                    crate::db::models::Account,
                    r#"SELECT a.* FROM accounts a
                       JOIN follows f ON f.target_account_id = a.id AND f.state = 'accepted'
                       WHERE a.instance_id = $1
                         AND a.suspended_at IS NULL
                         AND f.account_id = $4
                         AND (lower(a.username) LIKE $2 OR lower(a.display_name) LIKE $2)
                       ORDER BY a.followers_count DESC LIMIT $3 OFFSET $5"#,
                    instance.id, pattern, limit, vid, offset
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
                       ORDER BY followers_count DESC LIMIT $3 OFFSET $4"#,
                    instance.id, pattern, limit, offset
                )
                .fetch_all(&state.db)
                .await?
                .iter()
                .map(account_from_db)
                .collect()
            }
        } else if following_filter {
            let vid = viewer_id.ok_or(crate::error::AppError::Unauthorized)?;
            sqlx::query_as!(
                crate::db::models::Account,
                r#"SELECT a.* FROM accounts a
                   JOIN follows f ON f.target_account_id = a.id AND f.state = 'accepted'
                   WHERE a.instance_id = $1
                     AND a.suspended_at IS NULL
                     AND f.account_id = $4
                     AND (lower(a.username) LIKE $2 OR lower(a.display_name) LIKE $2)
                   ORDER BY a.followers_count DESC LIMIT $3 OFFSET $5"#,
                instance.id, pattern, limit, vid, offset
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
                   ORDER BY followers_count DESC LIMIT $3 OFFSET $4"#,
                instance.id, pattern, limit, offset
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
                 AND to_tsvector('simple', coalesce(s.text, ''))
                     @@ websearch_to_tsquery('simple', $2)
               ORDER BY s.id DESC LIMIT $3 OFFSET $6"#,
            instance.id, fts_query, limit, filter_account_id, viewer_id, offset
        )
        .fetch_all(&state.db)
        .await?;

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

        let mut result: Vec<Status> = Vec::with_capacity(rows.len());
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
        result
    } else {
        vec![]
    };

    let hashtags = if search_type.is_none() || search_type == Some("hashtags") {
        sqlx::query!(
            "SELECT id, name FROM tags WHERE lower(name) LIKE $1 ORDER BY name LIMIT $2 OFFSET $3",
            pattern, limit, offset
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
