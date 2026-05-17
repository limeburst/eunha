use axum::{
    extract::{Extension, Multipart, Path, Query, RawQuery, State},
    http::{header, HeaderMap, Uri},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    db::models::Account,
    error::{AppError, AppResult},
    feed,
    middleware::{AuthenticatedUser, ResolvedInstance},
    push,
    state::AppState,
};
use super::{
    convert::{account_from_db, status_from_db},
    types::{Account as ApiAccount, PaginationParams, Preferences, Relationship, SuggestionV2},
};

// ── GET /api/v1/accounts/verify_credentials ────────────────────────────────

pub async fn verify_credentials(
    State(state): State<AppState>,
    Extension(ResolvedInstance(_instance)): Extension<ResolvedInstance>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<ApiAccount>> {
    auth.require_scope("read:accounts")?;
    let account = fetch_account(&state, auth.account_id).await?;
    let mut api_account = account_from_db(&account);
    api_account.emojis = fetch_account_emojis(&state, &account).await;

    let user_prefs = sqlx::query!(
        "SELECT default_privacy, default_sensitive, default_language FROM users WHERE account_id = $1",
        account.id
    )
    .fetch_optional(&state.db)
    .await?;

    let (default_privacy, default_sensitive, default_language) = user_prefs.map_or(
        ("public".to_string(), false, None),
        |u| (u.default_privacy, u.default_sensitive, u.default_language),
    );

    let follow_requests: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM follows WHERE target_account_id = $1 AND state = 'pending'",
        account.id
    )
    .fetch_one(&state.db)
    .await?
    .unwrap_or(0);

    api_account.source = Some(super::types::AccountSource {
        privacy: default_privacy,
        sensitive: default_sensitive,
        language: default_language,
        note: account.note_text.clone(),
        fields: super::convert::fields_from_db(&account.fields),
        follow_requests_count: follow_requests,
        discoverable: account.discoverable,
        indexable: account.indexable,
        hide_collections: Some(account.hide_collections),
        attribution_domains: vec![],
        quote_policy: "public".into(),
    });

    // Populate roles for admin/moderator accounts
    if let Ok(Some(role)) = sqlx::query_scalar!(
        "SELECT role FROM users WHERE account_id = $1",
        account.id
    )
    .fetch_optional(&state.db)
    .await
    {
        api_account.roles = match role.as_str() {
            "admin" => vec![super::types::Role {
                id: "1".into(), name: "Admin".into(), color: "#6364ff".into(),
            }],
            "moderator" => vec![super::types::Role {
                id: "2".into(), name: "Moderator".into(), color: "#6364ff".into(),
            }],
            _ => vec![],
        };
    }

    Ok(Json(api_account))
}

// ── GET /api/v1/accounts/lookup ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct LookupQuery {
    pub acct: String,
    pub resolve: Option<bool>,
}

pub async fn lookup_account(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Query(q): Query<LookupQuery>,
) -> AppResult<Json<ApiAccount>> {
    // acct can be "username" (local) or "username@domain" (remote)
    let (username, domain) = match q.acct.split_once('@') {
        Some((user, domain)) => (user.to_lowercase(), Some(domain.to_lowercase())),
        None => (q.acct.to_lowercase(), None),
    };

    let found = match domain {
        None => sqlx::query_as!(
            Account,
            "SELECT * FROM accounts WHERE lower(username) = $1 AND instance_id = $2 AND domain IS NULL",
            username,
            instance.id,
        )
        .fetch_optional(&state.db)
        .await?,

        Some(ref d) => sqlx::query_as!(
            Account,
            "SELECT * FROM accounts WHERE lower(username) = $1 AND lower(domain) = $2",
            username,
            d,
        )
        .fetch_optional(&state.db)
        .await?,
    };

    if let Some(account) = found {
        return Ok(Json(account_from_db(&account)));
    }

    // Not found locally — attempt WebFinger resolution if requested and domain is known
    if q.resolve.unwrap_or(false) {
        if let Some(ref d) = domain {
            let acct_uri = format!("acct:{}@{}", username, d);
            let wf_url = format!("https://{}/.well-known/webfinger?resource={}", d, acct_uri);
            if let Ok(resp) = state.http.get(&wf_url)
                .header("Accept", "application/jrd+json, application/json")
                .send()
                .await
            {
                if let Ok(jrd) = resp.json::<serde_json::Value>().await {
                    let actor_uri = jrd
                        .get("links")
                        .and_then(|l| l.as_array())
                        .and_then(|links| {
                            links.iter().find(|l| {
                                l.get("rel").and_then(|r| r.as_str()) == Some("self")
                                    && l.get("type").and_then(|t| t.as_str())
                                        .map(|t| t.contains("activity+json") || t.contains("ld+json"))
                                        .unwrap_or(false)
                            })
                        })
                        .and_then(|l| l.get("href"))
                        .and_then(|h| h.as_str())
                        .map(str::to_owned);

                    if let Some(uri) = actor_uri {
                        let account_id = crate::api::ap::inbox::resolve_or_fetch_remote_account(
                            &state, &uri,
                        ).await?;
                        let account = sqlx::query_as!(
                            Account,
                            "SELECT * FROM accounts WHERE id = $1",
                            account_id,
                        )
                        .fetch_one(&state.db)
                        .await?;
                        return Ok(Json(account_from_db(&account)));
                    }
                }
            }
        }
    }

    Err(AppError::NotFound)
}

// ── GET /api/v1/accounts/:id ───────────────────────────────────────────────

pub async fn get_account(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<ApiAccount>> {
    let account = fetch_account(&state, id).await?;
    let mut api_account = account_from_db(&account);
    api_account.emojis = fetch_account_emojis(&state, &account).await;
    if let Some(ref moved_uri) = account.moved_to_uri {
        if let Ok(Some(moved)) = sqlx::query_as!(
            Account,
            "SELECT * FROM accounts WHERE uri = $1 LIMIT 1",
            moved_uri,
        )
        .fetch_optional(&state.db)
        .await {
            api_account.moved = Some(Box::new(account_from_db(&moved)));
        }
    }
    Ok(Json(api_account))
}

// ── GET /api/v1/accounts/:id/statuses ─────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct StatusesQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    pub only_media: Option<bool>,
    pub exclude_replies: Option<bool>,
    pub exclude_reblogs: Option<bool>,
    pub pinned: Option<bool>,
    pub tagged: Option<String>,
}

pub async fn get_account_statuses(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    uri: Uri,
    req_headers: HeaderMap,
    Query(q): Query<StatusesQuery>,
    auth: Option<Extension<AuthenticatedUser>>,
) -> AppResult<impl IntoResponse> {
    let account = fetch_account(&state, id).await?;
    if account.suspended_at.is_some() {
        return Ok((HeaderMap::new(), Json(Vec::<super::types::Status>::new())));
    }
    let viewer_id = auth.as_ref().map(|Extension(a)| a.account_id);

    // If target has blocked the viewer, return 403.
    if let Some(vid) = viewer_id {
        if vid != account.id {
            let blocked = sqlx::query_scalar!(
                "SELECT 1 FROM blocks WHERE account_id = $1 AND target_account_id = $2",
                account.id, vid,
            )
            .fetch_optional(&state.db)
            .await?
            .is_some();
            if blocked {
                return Err(AppError::Forbidden);
            }
        }
    }

    let is_self = viewer_id == Some(account.id);
    let is_follower = if !is_self {
        if let Some(vid) = viewer_id {
            sqlx::query_scalar!(
                "SELECT EXISTS(SELECT 1 FROM follows WHERE account_id = $1 AND target_account_id = $2 AND state = 'accepted')",
                vid, account.id,
            )
            .fetch_one(&state.db)
            .await?
            .unwrap_or(false)
        } else {
            false
        }
    } else {
        false
    };

    if q.pinned == Some(true) {
        let pinned_statuses = sqlx::query_as!(
            crate::db::models::Status,
            r#"SELECT s.* FROM statuses s
               JOIN status_pins sp ON sp.status_id = s.id
               WHERE sp.account_id = $1 AND s.deleted_at IS NULL
                 AND (
                   s.visibility IN ('public', 'unlisted')
                   OR ($2::boolean = true)
                   OR ($3::boolean = true AND s.visibility = 'private')
                 )
               ORDER BY sp.id DESC"#,
            account.id,
            is_self,
            is_follower,
        )
        .fetch_all(&state.db)
        .await?;
        let pin_ids: Vec<i64> = pinned_statuses.iter()
            .map(|s| s.reblog_of_id.unwrap_or(s.id))
            .collect();
        let pin_ctxs = if let Some(vid) = viewer_id {
            super::statuses::batch_viewer_contexts(&state, vid, &pin_ids).await?
        } else {
            std::collections::HashMap::new()
        };
        let pin_status_ids: Vec<i64> = pinned_statuses.iter().map(|s| s.id).collect();
        let pin_media_map = batch_status_media(&state, &pin_status_ids).await?;
        let pin_reblog_map = batch_reblog_data(&state, &pinned_statuses).await?;
        let pin_reblog_ids: Vec<i64> = pin_reblog_map.values().map(|(rs, _, _)| rs.id).collect();
        let mut pin_enrich_ids = pin_status_ids.clone();
        pin_enrich_ids.extend_from_slice(&pin_reblog_ids);
        let pin_tags_map = batch_status_tags(&state, &pin_enrich_ids).await?;
        let pin_mentions_map = batch_status_mentions(&state, &pin_enrich_ids).await?;
        let all_pin_statuses: Vec<crate::db::models::Status> = pinned_statuses.iter().cloned()
            .chain(pin_reblog_map.values().map(|(rs, _, _)| rs.clone()))
            .collect();
        let pin_emojis_map = batch_status_emojis(&state, &all_pin_statuses).await?;
        let pin_polls_map = batch_status_polls(&state, &pin_enrich_ids, viewer_id).await?;
        let pin_cards_map = batch_status_cards(&state, &pin_enrich_ids).await?;
        let mut result = Vec::with_capacity(pinned_statuses.len());
        for s in &pinned_statuses {
            let media = pin_media_map.get(&s.id).cloned().unwrap_or_default();
            let reblog = pin_reblog_map.get(&s.id).cloned();
            let effective_id = s.reblog_of_id.unwrap_or(s.id);
            let ctx = pin_ctxs.get(&effective_id).cloned();
            let mentions = pin_mentions_map.get(&s.id).cloned().unwrap_or_default();
            let rb_mentions = reblog.as_ref()
                .and_then(|(rs, _, _)| pin_mentions_map.get(&rs.id))
                .cloned()
                .unwrap_or_default();
            let mut api_status = status_from_db(s, &account, media, reblog, ctx, &mentions, &rb_mentions);
            api_status.tags = pin_tags_map.get(&s.id).cloned().unwrap_or_default();
            api_status.mentions = mentions;
            api_status.emojis = pin_emojis_map.get(&s.id).cloned().unwrap_or_default();
            api_status.poll = pin_polls_map.get(&s.id).cloned();
            api_status.card = pin_cards_map.get(&s.id).cloned();
            if let Some(ref mut rb) = api_status.reblog {
                let rid: i64 = rb.id.parse().unwrap_or(0);
                rb.tags = pin_tags_map.get(&rid).cloned().unwrap_or_default();
                rb.mentions = rb_mentions;
                rb.emojis = pin_emojis_map.get(&rid).cloned().unwrap_or_default();
                rb.poll = pin_polls_map.get(&rid).cloned();
                rb.card = pin_cards_map.get(&rid).cloned();
            }
            api_status.pinned = Some(true);
            result.push(api_status);
        }
        return Ok((HeaderMap::new(), Json(result)));
    }

    let limit = q.pagination.limit_clamped(20, 40);
    let max_id = q.pagination.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = q.pagination.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = q.pagination.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let tagged_lower = q.tagged.as_deref().map(|t| t.to_lowercase());
    let statuses = if min_id.is_some() {
        sqlx::query_as!(
            crate::db::models::Status,
            r#"SELECT statuses.* FROM statuses
               WHERE account_id = $1
                 AND deleted_at IS NULL
                 AND ($2::bigint IS NULL OR id > $2)
                 AND ($3::boolean IS NOT TRUE OR reblog_of_id IS NULL)
                 AND ($4::boolean IS NOT TRUE OR in_reply_to_id IS NULL)
                 AND (
                   visibility IN ('public', 'unlisted')
                   OR ($5::boolean = true)
                   OR ($6::boolean = true AND visibility = 'private')
                 )
                 AND (
                   text != ''
                   OR reblog_of_id IS NOT NULL
                   OR EXISTS (SELECT 1 FROM media_attachments WHERE status_id = statuses.id)
                 )
                 AND ($8::boolean IS NOT TRUE OR
                   EXISTS (SELECT 1 FROM media_attachments WHERE status_id = statuses.id) OR
                   (reblog_of_id IS NOT NULL AND EXISTS (SELECT 1 FROM media_attachments WHERE status_id = reblog_of_id))
                 )
                 AND ($9::text IS NULL OR EXISTS (
                   SELECT 1 FROM status_tags st
                   JOIN tags t ON t.id = st.tag_id
                   WHERE st.status_id = statuses.id AND t.name = $9
                 ))
               ORDER BY id ASC
               LIMIT $7"#,
            account.id,
            min_id,
            q.exclude_reblogs.unwrap_or(false),
            q.exclude_replies.unwrap_or(false),
            is_self,
            is_follower,
            limit,
            q.only_media.unwrap_or(false),
            tagged_lower,
        )
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as!(
            crate::db::models::Status,
            r#"SELECT statuses.* FROM statuses
               WHERE account_id = $1
                 AND deleted_at IS NULL
                 AND ($2::bigint IS NULL OR id < $2)
                 AND ($3::bigint IS NULL OR id > $3)
                 AND ($4::boolean IS NOT TRUE OR reblog_of_id IS NULL)
                 AND ($5::boolean IS NOT TRUE OR in_reply_to_id IS NULL)
                 AND (
                   visibility IN ('public', 'unlisted')
                   OR ($6::boolean = true)
                   OR ($7::boolean = true AND visibility = 'private')
                 )
                 AND (
                   text != ''
                   OR reblog_of_id IS NOT NULL
                   OR EXISTS (SELECT 1 FROM media_attachments WHERE status_id = statuses.id)
                 )
                 AND ($9::boolean IS NOT TRUE OR
                   EXISTS (SELECT 1 FROM media_attachments WHERE status_id = statuses.id) OR
                   (reblog_of_id IS NOT NULL AND EXISTS (SELECT 1 FROM media_attachments WHERE status_id = reblog_of_id))
                 )
                 AND ($10::text IS NULL OR EXISTS (
                   SELECT 1 FROM status_tags st
                   JOIN tags t ON t.id = st.tag_id
                   WHERE st.status_id = statuses.id AND t.name = $10
                 ))
               ORDER BY id DESC
               LIMIT $8"#,
            account.id,
            max_id,
            since_id,
            q.exclude_reblogs.unwrap_or(false),
            q.exclude_replies.unwrap_or(false),
            is_self,
            is_follower,
            limit,
            q.only_media.unwrap_or(false),
            tagged_lower,
        )
        .fetch_all(&state.db)
        .await?
    };

    let effective_ids: Vec<i64> = statuses.iter()
        .map(|s| s.reblog_of_id.unwrap_or(s.id))
        .collect();
    let ctxs = if let Some(vid) = viewer_id {
        super::statuses::batch_viewer_contexts(&state, vid, &effective_ids).await?
    } else {
        std::collections::HashMap::new()
    };

    let all_status_ids: Vec<i64> = statuses.iter().map(|s| s.id).collect();
    let media_map = batch_status_media(&state, &all_status_ids).await?;
    let reblog_map = batch_reblog_data(&state, &statuses).await?;
    let reblog_ids: Vec<i64> = reblog_map.values().map(|(rs, _, _)| rs.id).collect();
    let mut enrich_ids = all_status_ids.clone();
    enrich_ids.extend_from_slice(&reblog_ids);
    let tags_map = batch_status_tags(&state, &enrich_ids).await?;
    let mentions_map = batch_status_mentions(&state, &enrich_ids).await?;
    let all_statuses_for_emoji: Vec<crate::db::models::Status> = statuses.iter().cloned()
        .chain(reblog_map.values().map(|(rs, _, _)| rs.clone()))
        .collect();
    let emojis_map = batch_status_emojis(&state, &all_statuses_for_emoji).await?;
    let polls_map = batch_status_polls(&state, &enrich_ids, viewer_id).await?;
    let cards_map = batch_status_cards(&state, &enrich_ids).await?;

    let mut result = Vec::with_capacity(statuses.len());
    for s in &statuses {
        let media = media_map.get(&s.id).cloned().unwrap_or_default();
        let reblog = reblog_map.get(&s.id).cloned();
        let effective_id = s.reblog_of_id.unwrap_or(s.id);
        let ctx = ctxs.get(&effective_id).cloned();
        let mentions = mentions_map.get(&s.id).cloned().unwrap_or_default();
        let rb_mentions = reblog.as_ref()
            .and_then(|(rs, _, _)| mentions_map.get(&rs.id))
            .cloned()
            .unwrap_or_default();
        let mut api = status_from_db(s, &account, media, reblog, ctx, &mentions, &rb_mentions);
        api.tags = tags_map.get(&s.id).cloned().unwrap_or_default();
        api.mentions = mentions;
        api.emojis = emojis_map.get(&s.id).cloned().unwrap_or_default();
        api.poll = polls_map.get(&s.id).cloned();
        api.card = cards_map.get(&s.id).cloned();
        if let Some(ref mut rb) = api.reblog {
            let rid: i64 = rb.id.parse().unwrap_or(0);
            rb.tags = tags_map.get(&rid).cloned().unwrap_or_default();
            rb.mentions = rb_mentions;
            rb.emojis = emojis_map.get(&rid).cloned().unwrap_or_default();
            rb.poll = polls_map.get(&rid).cloned();
            rb.card = cards_map.get(&rid).cloned();
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

// ── GET /api/v1/accounts/relationships ────────────────────────────────────

pub async fn get_relationships(
    State(state): State<AppState>,
    RawQuery(qs): RawQuery,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<Relationship>>> {
    auth.require_scope("read:follows")?;
    // serde_urlencoded treats id[]=v1&id[]=v2 as a duplicate field → 400.
    // Parse with form_urlencoded which correctly returns each pair separately.
    let ids: Vec<i64> = url::form_urlencoded::parse(
            qs.as_deref().unwrap_or("").as_bytes()
        )
        .filter(|(k, _)| k == "id[]" || k == "id")
        .filter_map(|(_, v)| v.parse::<i64>().ok())
        .collect();

    if ids.is_empty() {
        return Ok(Json(vec![]));
    }
    let results = batch_build_relationships(&state, auth.account_id, &ids).await?;
    Ok(Json(results))
}

// ── POST /api/v1/accounts/:id/follow ──────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct FollowParams {
    pub reblogs: Option<bool>,
    pub notify: Option<bool>,
    pub languages: Option<Vec<String>>,
}

pub async fn follow_account(
    State(state): State<AppState>,
    Path(target_id): Path<i64>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Extension(auth): Extension<AuthenticatedUser>,
    body: Option<Json<FollowParams>>,
) -> AppResult<Json<Relationship>> {
    auth.require_scope("write:follows")?;
    if auth.account_id == target_id {
        return Err(AppError::Forbidden);
    }
    let params = body.map(|Json(p)| p).unwrap_or_default();
    let show_reblogs = params.reblogs.unwrap_or(true);
    let notify = params.notify.unwrap_or(false);
    let languages: Vec<String> = params.languages.unwrap_or_default();

    // If the target has blocked the requester, silently return current relationship
    let blocked_by_target = sqlx::query_scalar!(
        "SELECT 1 FROM blocks WHERE account_id = $1 AND target_account_id = $2",
        target_id, auth.account_id,
    )
    .fetch_optional(&state.db)
    .await?
    .is_some();
    if blocked_by_target {
        return build_relationship(&state, auth.account_id, target_id).await.map(Json);
    }

    // Check if follow already exists
    let existing = sqlx::query!(
        "SELECT state FROM follows WHERE account_id = $1 AND target_account_id = $2",
        auth.account_id, target_id,
    )
    .fetch_optional(&state.db)
    .await?;

    if existing.is_some() {
        // Already following — update settings only, no counts or notifications
        sqlx::query!(
            "UPDATE follows SET show_reblogs = $3, notify = $4, languages = $5
             WHERE account_id = $1 AND target_account_id = $2",
            auth.account_id, target_id, show_reblogs, notify, &languages,
        )
        .execute(&state.db)
        .await?;
        return build_relationship(&state, auth.account_id, target_id).await.map(Json);
    }

    let target = fetch_account(&state, target_id).await?;
    let state_val = if target.locked { "pending" } else { "accepted" };

    sqlx::query!(
        r#"INSERT INTO follows (account_id, target_account_id, state, show_reblogs, notify, languages)
           VALUES ($1, $2, $3, $4, $5, $6)"#,
        auth.account_id,
        target_id,
        state_val,
        show_reblogs,
        notify,
        &languages,
    )
    .execute(&state.db)
    .await?;

    if state_val == "accepted" {
        sqlx::query!(
            "UPDATE accounts SET followers_count = followers_count + 1 WHERE id = $1",
            target_id
        )
        .execute(&state.db)
        .await?;
        sqlx::query!(
            "UPDATE accounts SET following_count = following_count + 1 WHERE id = $1",
            auth.account_id
        )
        .execute(&state.db)
        .await?;

        let follower = fetch_account(&state, auth.account_id).await?;
        push::create_and_push(
            &state,
            target_id,
            auth.account_id,
            "follow",
            None,
            format!("{} followed you", follower.display_name),
            follower.acct().clone(),
            follower.avatar.clone().unwrap_or_default(),
        ).await;

        // Backfill the new follower's feed with recent statuses from the followed account
        let mut redis = state.redis.clone();
        let db = state.db.clone();
        let iid = instance.id;
        let follower_id = auth.account_id;
        if feed::sync_fanout() {
            feed::backfill_follow(&mut redis, &db, iid, follower_id, target_id).await;
        } else {
            tokio::spawn(async move {
                feed::backfill_follow(&mut redis, &db, iid, follower_id, target_id).await;
            });
        }
    } else {
        let requester = fetch_account(&state, auth.account_id).await?;
        push::create_and_push(
            &state,
            target_id,
            auth.account_id,
            "follow_request",
            None,
            format!("{} wants to follow you", requester.display_name),
            requester.acct().clone(),
            requester.avatar.clone().unwrap_or_default(),
        ).await;
    }

    build_relationship(&state, auth.account_id, target_id).await.map(Json)
}

// ── POST /api/v1/accounts/:id/unfollow ────────────────────────────────────

pub async fn unfollow_account(
    State(state): State<AppState>,
    Path(target_id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
    auth.require_scope("write:follows")?;
    let deleted = sqlx::query!(
        "DELETE FROM follows WHERE account_id = $1 AND target_account_id = $2 RETURNING state",
        auth.account_id,
        target_id,
    )
    .fetch_optional(&state.db)
    .await?;

    if let Some(row) = deleted {
        if row.state == "accepted" {
            sqlx::query!(
                "UPDATE accounts SET followers_count = GREATEST(followers_count - 1, 0) WHERE id = $1",
                target_id
            )
            .execute(&state.db)
            .await?;
            sqlx::query!(
                "UPDATE accounts SET following_count = GREATEST(following_count - 1, 0) WHERE id = $1",
                auth.account_id
            )
            .execute(&state.db)
            .await?;
        }
    }

    build_relationship(&state, auth.account_id, target_id).await.map(Json)
}

// ── GET /api/v1/accounts/:id/followers ────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct FollowersQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,
}

pub async fn get_account_followers(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    uri: Uri,
    req_headers: HeaderMap,
    Query(q): Query<FollowersQuery>,
    viewer: Option<Extension<AuthenticatedUser>>,
) -> AppResult<impl IntoResponse> {
    let target = fetch_account(&state, id).await?;
    if target.suspended_at.is_some() {
        return Ok((HeaderMap::new(), Json(Vec::<ApiAccount>::new())));
    }
    let viewer_id = viewer.map(|Extension(a)| a.account_id);
    // Respect hide_collections unless the viewer is the account owner
    if target.hide_collections && viewer_id != Some(id) {
        return Ok((HeaderMap::new(), Json(Vec::<ApiAccount>::new())));
    }

    let limit = q.pagination.limit_clamped(40, 80);
    let max_id = q.pagination.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = q.pagination.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = q.pagination.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let accounts = sqlx::query_as!(
        Account,
        r#"SELECT a.* FROM accounts a
           JOIN follows f ON f.account_id = a.id
           WHERE f.target_account_id = $1 AND f.state = 'accepted'
             AND ($2::bigint IS NULL OR a.id < $2)
             AND ($3::bigint IS NULL OR a.id > $3)
             AND ($6::bigint IS NULL OR a.id > $6)
             AND ($4::bigint IS NULL OR NOT EXISTS (
                 SELECT 1 FROM blocks b
                 WHERE (b.account_id = $4 AND b.target_account_id = a.id)
                    OR (b.account_id = a.id AND b.target_account_id = $4)
             ))
           ORDER BY a.id DESC LIMIT $5"#,
        id, max_id, since_id, viewer_id, limit, min_id
    )
    .fetch_all(&state.db)
    .await?;

    let api_accounts: Vec<ApiAccount> = accounts.iter().map(account_from_db).collect();
    let link = api_accounts.first().zip(api_accounts.last()).map(|(newest, oldest)| {
        let extra = super::non_pagination_query(uri.query());
        super::link_header(&req_headers, uri.path(), &extra, &newest.id, &oldest.id)
    });
    let mut resp_headers = HeaderMap::new();
    if let Some(v) = link {
        if let Ok(val) = v.parse() {
            resp_headers.insert(header::LINK, val);
        }
    }
    Ok((resp_headers, Json(api_accounts)))
}

// ── GET /api/v1/accounts/:id/following ────────────────────────────────────

pub async fn get_account_following(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    uri: Uri,
    req_headers: HeaderMap,
    Query(q): Query<FollowersQuery>,
    viewer: Option<Extension<AuthenticatedUser>>,
) -> AppResult<impl IntoResponse> {
    let target = fetch_account(&state, id).await?;
    if target.suspended_at.is_some() {
        return Ok((HeaderMap::new(), Json(Vec::<ApiAccount>::new())));
    }
    let viewer_id = viewer.map(|Extension(a)| a.account_id);
    // Respect hide_collections unless the viewer is the account owner
    if target.hide_collections && viewer_id != Some(id) {
        return Ok((HeaderMap::new(), Json(Vec::<ApiAccount>::new())));
    }

    let limit = q.pagination.limit_clamped(40, 80);
    let max_id = q.pagination.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = q.pagination.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = q.pagination.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let accounts = sqlx::query_as!(
        Account,
        r#"SELECT a.* FROM accounts a
           JOIN follows f ON f.target_account_id = a.id
           WHERE f.account_id = $1 AND f.state = 'accepted'
             AND ($2::bigint IS NULL OR a.id < $2)
             AND ($3::bigint IS NULL OR a.id > $3)
             AND ($6::bigint IS NULL OR a.id > $6)
             AND ($4::bigint IS NULL OR NOT EXISTS (
                 SELECT 1 FROM blocks b
                 WHERE (b.account_id = $4 AND b.target_account_id = a.id)
                    OR (b.account_id = a.id AND b.target_account_id = $4)
             ))
           ORDER BY a.id DESC LIMIT $5"#,
        id, max_id, since_id, viewer_id, limit, min_id
    )
    .fetch_all(&state.db)
    .await?;

    let api_accounts: Vec<ApiAccount> = accounts.iter().map(account_from_db).collect();
    let link = api_accounts.first().zip(api_accounts.last()).map(|(newest, oldest)| {
        let extra = super::non_pagination_query(uri.query());
        super::link_header(&req_headers, uri.path(), &extra, &newest.id, &oldest.id)
    });
    let mut resp_headers = HeaderMap::new();
    if let Some(v) = link {
        if let Ok(val) = v.parse() {
            resp_headers.insert(header::LINK, val);
        }
    }
    Ok((resp_headers, Json(api_accounts)))
}

// ── GET /api/v1/accounts/search ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AccountSearchQuery {
    pub q: String,
    pub limit: Option<i64>,
    pub resolve: Option<bool>,
    pub following: Option<bool>,
}

pub async fn search_accounts(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Query(q): Query<AccountSearchQuery>,
    auth: Option<Extension<AuthenticatedUser>>,
) -> AppResult<Json<Vec<ApiAccount>>> {
    let limit = q.limit.unwrap_or(40).min(80).max(1);
    let pattern = format!("%{}%", q.q.to_lowercase());

    let mut accounts = if q.following.unwrap_or(false) {
        if let Some(Extension(ref auth)) = auth {
            sqlx::query_as!(
                Account,
                r#"SELECT a.* FROM accounts a
                   JOIN follows f ON f.target_account_id = a.id
                   WHERE f.account_id = $1 AND f.state = 'accepted'
                     AND a.suspended_at IS NULL
                     AND (lower(a.username) LIKE $2 OR lower(a.display_name) LIKE $2)
                   ORDER BY a.username LIMIT $3"#,
                auth.account_id, pattern, limit
            )
            .fetch_all(&state.db)
            .await?
        } else {
            vec![]
        }
    } else {
        sqlx::query_as!(
            Account,
            r#"SELECT * FROM accounts
               WHERE instance_id = $1
                 AND suspended_at IS NULL
                 AND (lower(username) LIKE $2 OR lower(display_name) LIKE $2)
               ORDER BY username LIMIT $3"#,
            instance.id, pattern, limit
        )
        .fetch_all(&state.db)
        .await?
    };

    // If resolve=true and the query looks like user@domain, try WebFinger for any
    // remote account not already in the local results.
    if q.resolve.unwrap_or(false) && accounts.is_empty() {
        if let Some((username, domain)) = q.q.split_once('@') {
            let username = username.to_lowercase();
            let domain = domain.to_lowercase();
            // Only attempt fetch if not already present locally
            let already_known = sqlx::query_scalar!(
                "SELECT id FROM accounts WHERE lower(username) = $1 AND lower(domain) = $2",
                username, domain,
            )
            .fetch_optional(&state.db)
            .await?
            .is_some();

            if !already_known {
                let acct_uri = format!("acct:{}@{}", username, domain);
                let wf_url = format!("https://{}/.well-known/webfinger?resource={}", domain, acct_uri);
                if let Ok(resp) = state.http.get(&wf_url)
                    .header("Accept", "application/jrd+json, application/json")
                    .send()
                    .await
                {
                    if let Ok(jrd) = resp.json::<serde_json::Value>().await {
                        let actor_uri = jrd
                            .get("links").and_then(|l| l.as_array())
                            .and_then(|links| links.iter().find(|l| {
                                l.get("rel").and_then(|r| r.as_str()) == Some("self")
                                    && l.get("type").and_then(|t| t.as_str())
                                        .map(|t| t.contains("activity+json") || t.contains("ld+json"))
                                        .unwrap_or(false)
                            }))
                            .and_then(|l| l.get("href"))
                            .and_then(|h| h.as_str())
                            .map(str::to_owned);

                        if let Some(uri) = actor_uri {
                            if let Ok(account_id) =
                                crate::api::ap::inbox::resolve_or_fetch_remote_account(&state, &uri).await
                            {
                                if let Ok(account) = sqlx::query_as!(
                                    Account,
                                    "SELECT * FROM accounts WHERE id = $1",
                                    account_id,
                                )
                                .fetch_one(&state.db)
                                .await
                                {
                                    accounts.push(account);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(Json(accounts.iter().map(account_from_db).collect()))
}

// ── PATCH /api/v1/accounts/update_credentials ─────────────────────────────

pub async fn update_credentials(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Extension(crate::middleware::ResolvedInstance(instance)): Extension<crate::middleware::ResolvedInstance>,
    mut multipart: Multipart,
) -> AppResult<Json<ApiAccount>> {
    auth.require_scope("write:accounts")?;
    let mut display_name: Option<String> = None;
    let mut note: Option<String> = None;
    let mut locked: Option<bool> = None;
    let mut bot: Option<bool> = None;
    let mut discoverable: Option<bool> = None;
    let mut avatar_url: Option<String> = None;
    let mut header_url: Option<String> = None;
    let mut source_privacy: Option<String> = None;
    let mut source_sensitive: Option<bool> = None;
    let mut source_language: Option<Option<String>> = None;
    let mut source_hide_collections: Option<bool> = None;
    let mut indexable: Option<bool> = None;
    // fields_attributes[N][name] / fields_attributes[N][value]
    let mut fields_map: std::collections::BTreeMap<u32, (String, String)> = std::collections::BTreeMap::new();
    let mut fields_submitted = false;

    while let Some(field) = multipart.next_field().await.map_err(|e| AppError::Unprocessable(e.to_string()))? {
        let name = field.name().unwrap_or("").to_string();
        // Parse fields_attributes[N][name] and fields_attributes[N][value]
        if let Some(rest) = name.strip_prefix("fields_attributes[") {
            if let Some((idx_str, key)) = rest.split_once(']') {
                if let Ok(idx) = idx_str.parse::<u32>() {
                    let text = field.text().await.map_err(|e| AppError::Unprocessable(e.to_string()))?;
                    fields_submitted = true;
                    let entry = fields_map.entry(idx).or_default();
                    match key {
                        "[name]" => entry.0 = text,
                        "[value]" => entry.1 = text,
                        _ => {}
                    }
                }
            }
            continue;
        }
        match name.as_str() {
            "display_name" => {
                display_name = Some(field.text().await.map_err(|e| AppError::Unprocessable(e.to_string()))?);
            }
            "note" => {
                note = Some(field.text().await.map_err(|e| AppError::Unprocessable(e.to_string()))?);
            }
            "locked" => {
                let v = field.text().await.map_err(|e| AppError::Unprocessable(e.to_string()))?;
                locked = Some(v == "true" || v == "1");
            }
            "bot" => {
                let v = field.text().await.map_err(|e| AppError::Unprocessable(e.to_string()))?;
                bot = Some(v == "true" || v == "1");
            }
            "discoverable" => {
                let v = field.text().await.map_err(|e| AppError::Unprocessable(e.to_string()))?;
                discoverable = Some(v == "true" || v == "1");
            }
            "source[privacy]" => {
                let v = field.text().await.map_err(|e| AppError::Unprocessable(e.to_string()))?;
                if matches!(v.as_str(), "public" | "unlisted" | "private" | "direct") {
                    source_privacy = Some(v);
                }
            }
            "source[sensitive]" => {
                let v = field.text().await.map_err(|e| AppError::Unprocessable(e.to_string()))?;
                source_sensitive = Some(v == "true" || v == "1");
            }
            "source[language]" => {
                let v = field.text().await.map_err(|e| AppError::Unprocessable(e.to_string()))?;
                source_language = Some(if v.is_empty() { None } else { Some(v) });
            }
            "source[hide_collections]" => {
                let v = field.text().await.map_err(|e| AppError::Unprocessable(e.to_string()))?;
                source_hide_collections = Some(v == "true" || v == "1");
            }
            "indexable" | "source[indexable]" => {
                let v = field.text().await.map_err(|e| AppError::Unprocessable(e.to_string()))?;
                indexable = Some(v == "true" || v == "1");
            }
            "avatar" => {
                let content_type = field.content_type().unwrap_or("application/octet-stream").to_string();
                let data = field.bytes().await.map_err(|e| AppError::Unprocessable(e.to_string()))?;
                if !data.is_empty() {
                    let key = crate::media::account_avatar_key(instance.id, auth.account_id, &content_type);
                    state.storage.store(&data, &key, &content_type).await?;
                    avatar_url = Some(state.storage.public_url(&key));
                }
            }
            "header" => {
                let content_type = field.content_type().unwrap_or("application/octet-stream").to_string();
                let data = field.bytes().await.map_err(|e| AppError::Unprocessable(e.to_string()))?;
                if !data.is_empty() {
                    let key = crate::media::account_header_key(instance.id, auth.account_id, &content_type);
                    state.storage.store(&data, &key, &content_type).await?;
                    header_url = Some(state.storage.public_url(&key));
                }
            }
            _ => {}
        }
    }

    if let Some(ref dn) = display_name {
        sqlx::query!("UPDATE accounts SET display_name = $1 WHERE id = $2", dn, auth.account_id)
            .execute(&state.db).await?;
    }
    if let Some(ref n) = note {
        let domain = instance.domain.as_str();
        let note_html = super::formatting::render_content(n, domain, &std::collections::HashMap::new());
        let note_html = if note_html.is_empty() { String::new() } else { note_html };
        sqlx::query!("UPDATE accounts SET note = $1, note_text = $2 WHERE id = $3", note_html, n, auth.account_id)
            .execute(&state.db).await?;
    }
    if let Some(l) = locked {
        sqlx::query!("UPDATE accounts SET locked = $1 WHERE id = $2", l, auth.account_id)
            .execute(&state.db).await?;
        // Auto-approve pending follow requests when account becomes unlocked
        if !l {
            let pending = sqlx::query!(
                "UPDATE follows SET state = 'accepted' WHERE target_account_id = $1 AND state = 'pending' RETURNING account_id",
                auth.account_id,
            )
            .fetch_all(&state.db)
            .await?;
            for row in &pending {
                let _ = sqlx::query!(
                    "UPDATE accounts SET followers_count = followers_count + 1 WHERE id = $1",
                    auth.account_id
                )
                .execute(&state.db)
                .await;
                let _ = sqlx::query!(
                    "UPDATE accounts SET following_count = following_count + 1 WHERE id = $1",
                    row.account_id
                )
                .execute(&state.db)
                .await;
                crate::push::create_and_push(
                    &state,
                    auth.account_id,
                    row.account_id,
                    "follow",
                    None,
                    "New follower".into(),
                    "".into(),
                    "".into(),
                )
                .await;
            }
        }
    }
    if let Some(b) = bot {
        sqlx::query!("UPDATE accounts SET bot = $1 WHERE id = $2", b, auth.account_id)
            .execute(&state.db).await?;
    }
    if let Some(d) = discoverable {
        sqlx::query!("UPDATE accounts SET discoverable = $1 WHERE id = $2", d, auth.account_id)
            .execute(&state.db).await?;
    }
    if let Some(ix) = indexable {
        sqlx::query!("UPDATE accounts SET indexable = $1 WHERE id = $2", ix, auth.account_id)
            .execute(&state.db).await?;
    }
    if let Some(ref url) = avatar_url {
        sqlx::query!(
            "UPDATE accounts SET avatar = $1, avatar_static = $1 WHERE id = $2",
            url, auth.account_id
        )
        .execute(&state.db).await?;
    }
    if let Some(ref url) = header_url {
        sqlx::query!(
            "UPDATE accounts SET header = $1, header_static = $1 WHERE id = $2",
            url, auth.account_id
        )
        .execute(&state.db).await?;
    }

    // Collect non-empty fields and save as JSONB
    if fields_submitted {
        let fields_json: serde_json::Value = fields_map
            .into_values()
            .filter(|(n, _)| !n.is_empty())
            .map(|(n, v)| serde_json::json!({"name": n, "value": v, "verified_at": null}))
            .collect();
        sqlx::query!(
            "UPDATE accounts SET fields = $1 WHERE id = $2",
            fields_json, auth.account_id
        )
        .execute(&state.db).await?;
    }

    if let Some(ref p) = source_privacy {
        sqlx::query!(
            "UPDATE users SET default_privacy = $1 WHERE account_id = $2",
            p, auth.account_id
        )
        .execute(&state.db).await?;
    }
    if let Some(s) = source_sensitive {
        sqlx::query!(
            "UPDATE users SET default_sensitive = $1 WHERE account_id = $2",
            s, auth.account_id
        )
        .execute(&state.db).await?;
    }
    if let Some(ref lang) = source_language {
        sqlx::query!(
            "UPDATE users SET default_language = $1 WHERE account_id = $2",
            *lang, auth.account_id
        )
        .execute(&state.db).await?;
    }
    if let Some(hc) = source_hide_collections {
        sqlx::query!(
            "UPDATE accounts SET hide_collections = $1 WHERE id = $2",
            hc, auth.account_id
        )
        .execute(&state.db).await?;
    }

    let account = fetch_account(&state, auth.account_id).await?;
    let fields = super::convert::fields_from_db(&account.fields);
    let mut api_account = account_from_db(&account);
    let follow_requests_count: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM follows WHERE target_account_id = $1 AND state = 'pending'",
        auth.account_id,
    )
    .fetch_one(&state.db)
    .await?
    .unwrap_or(0);

    let user_prefs = sqlx::query!(
        "SELECT default_privacy, default_sensitive, default_language FROM users WHERE account_id = $1",
        auth.account_id
    )
    .fetch_optional(&state.db)
    .await?;

    let (default_privacy, default_sensitive, default_language) = user_prefs.map_or(
        ("public".to_string(), false, None),
        |u| (u.default_privacy, u.default_sensitive, u.default_language),
    );

    api_account.source = Some(super::types::AccountSource {
        privacy: default_privacy,
        sensitive: default_sensitive,
        language: default_language,
        note: account.note_text.clone(),
        fields: fields.clone(),
        follow_requests_count,
        discoverable: account.discoverable,
        indexable: account.indexable,
        hide_collections: Some(account.hide_collections),
        attribution_domains: vec![],
        quote_policy: "public".into(),
    });
    Ok(Json(api_account))
}

// ── POST /api/v1/accounts/:id/mute ────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct MuteParams {
    /// Whether to also mute notifications from this account (default true).
    pub notifications: Option<bool>,
    /// Mute duration in seconds; 0 or absent means indefinite.
    pub duration: Option<i64>,
}

pub async fn mute_account(
    State(state): State<AppState>,
    Path(target_id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
    body: Option<Json<MuteParams>>,
) -> AppResult<Json<Relationship>> {
    auth.require_scope("write:mutes")?;
    let params = body.map(|Json(p)| p).unwrap_or_default();
    let hide_notifications = params.notifications.unwrap_or(true);
    let expires_at: Option<chrono::DateTime<chrono::Utc>> = params.duration
        .filter(|&d| d > 0)
        .map(|d| chrono::Utc::now() + chrono::Duration::seconds(d));

    sqlx::query!(
        r#"INSERT INTO mutes (account_id, target_account_id, hide_notifications, expires_at)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (account_id, target_account_id)
           DO UPDATE SET hide_notifications = EXCLUDED.hide_notifications,
                         expires_at = EXCLUDED.expires_at"#,
        auth.account_id, target_id, hide_notifications, expires_at,
    )
    .execute(&state.db)
    .await?;

    build_relationship(&state, auth.account_id, target_id).await.map(Json)
}

// ── POST /api/v1/accounts/:id/unmute ──────────────────────────────────────

pub async fn unmute_account(
    State(state): State<AppState>,
    Path(target_id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
    auth.require_scope("write:mutes")?;
    sqlx::query!(
        "DELETE FROM mutes WHERE account_id = $1 AND target_account_id = $2",
        auth.account_id, target_id
    )
    .execute(&state.db)
    .await?;

    build_relationship(&state, auth.account_id, target_id).await.map(Json)
}

// ── POST /api/v1/accounts/:id/block ───────────────────────────────────────

pub async fn block_account(
    State(state): State<AppState>,
    Path(target_id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
    auth.require_scope("write:blocks")?;
    sqlx::query!(
        r#"INSERT INTO blocks (account_id, target_account_id) VALUES ($1, $2)
           ON CONFLICT (account_id, target_account_id) DO NOTHING"#,
        auth.account_id, target_id
    )
    .execute(&state.db)
    .await?;

    // Remove any follow relationship in both directions and update counts
    let deleted = sqlx::query!(
        "DELETE FROM follows WHERE ((account_id = $1 AND target_account_id = $2) OR (account_id = $2 AND target_account_id = $1)) AND state = 'accepted' RETURNING account_id, target_account_id",
        auth.account_id, target_id
    )
    .fetch_all(&state.db)
    .await?;
    for row in &deleted {
        let _ = sqlx::query!(
            "UPDATE accounts SET following_count = GREATEST(following_count - 1, 0) WHERE id = $1",
            row.account_id,
        ).execute(&state.db).await;
        let _ = sqlx::query!(
            "UPDATE accounts SET followers_count = GREATEST(followers_count - 1, 0) WHERE id = $1",
            row.target_account_id,
        ).execute(&state.db).await;
    }
    // Also delete any pending follow requests
    sqlx::query!(
        "DELETE FROM follows WHERE (account_id = $1 AND target_account_id = $2) OR (account_id = $2 AND target_account_id = $1)",
        auth.account_id, target_id
    )
    .execute(&state.db)
    .await?;

    build_relationship(&state, auth.account_id, target_id).await.map(Json)
}

// ── POST /api/v1/accounts/:id/unblock ─────────────────────────────────────

pub async fn unblock_account(
    State(state): State<AppState>,
    Path(target_id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
    auth.require_scope("write:blocks")?;
    sqlx::query!(
        "DELETE FROM blocks WHERE account_id = $1 AND target_account_id = $2",
        auth.account_id, target_id
    )
    .execute(&state.db)
    .await?;

    build_relationship(&state, auth.account_id, target_id).await.map(Json)
}

// ── GET /api/v1/blocks ────────────────────────────────────────────────────

pub async fn get_blocks(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    uri: Uri,
    req_headers: HeaderMap,
    Query(q): Query<PaginationParams>,
) -> AppResult<impl IntoResponse> {
    auth.require_scope("read:blocks")?;
    let limit = q.limit_clamped(40, 80);
    let max_id = q.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = q.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = q.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let accounts = sqlx::query_as!(
        Account,
        r#"SELECT a.* FROM accounts a
           JOIN blocks b ON b.target_account_id = a.id
           WHERE b.account_id = $1
             AND ($2::bigint IS NULL OR a.id < $2)
             AND ($3::bigint IS NULL OR a.id > $3)
             AND ($5::bigint IS NULL OR a.id > $5)
           ORDER BY a.id DESC LIMIT $4"#,
        auth.account_id, max_id, since_id, limit, min_id,
    )
    .fetch_all(&state.db)
    .await?;
    let api_accounts: Vec<ApiAccount> = accounts.iter().map(account_from_db).collect();
    let link = api_accounts.first().zip(api_accounts.last()).map(|(newest, oldest)| {
        let extra = super::non_pagination_query(uri.query());
        super::link_header(&req_headers, uri.path(), &extra, &newest.id, &oldest.id)
    });
    let mut resp_headers = HeaderMap::new();
    if let Some(v) = link {
        if let Ok(val) = v.parse() {
            resp_headers.insert(header::LINK, val);
        }
    }
    Ok((resp_headers, Json(api_accounts)))
}

// ── GET /api/v1/mutes ─────────────────────────────────────────────────────

pub async fn get_mutes(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    uri: Uri,
    req_headers: HeaderMap,
    Query(q): Query<PaginationParams>,
) -> AppResult<impl IntoResponse> {
    auth.require_scope("read:mutes")?;
    let limit = q.limit_clamped(40, 80);
    let max_id = q.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = q.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = q.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let accounts = sqlx::query_as!(
        Account,
        r#"SELECT a.* FROM accounts a
           JOIN mutes m ON m.target_account_id = a.id
           WHERE m.account_id = $1
             AND (m.expires_at IS NULL OR m.expires_at > now())
             AND ($2::bigint IS NULL OR a.id < $2)
             AND ($3::bigint IS NULL OR a.id > $3)
             AND ($5::bigint IS NULL OR a.id > $5)
           ORDER BY a.id DESC LIMIT $4"#,
        auth.account_id, max_id, since_id, limit, min_id,
    )
    .fetch_all(&state.db)
    .await?;
    let api_accounts: Vec<ApiAccount> = accounts.iter().map(account_from_db).collect();
    let link = api_accounts.first().zip(api_accounts.last()).map(|(newest, oldest)| {
        let extra = super::non_pagination_query(uri.query());
        super::link_header(&req_headers, uri.path(), &extra, &newest.id, &oldest.id)
    });
    let mut resp_headers = HeaderMap::new();
    if let Some(v) = link {
        if let Ok(val) = v.parse() {
            resp_headers.insert(header::LINK, val);
        }
    }
    Ok((resp_headers, Json(api_accounts)))
}

// ── GET /api/v1/preferences ───────────────────────────────────────────────

pub async fn get_preferences(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Preferences>> {
    auth.require_scope("read:accounts")?;
    let user = sqlx::query!(
        "SELECT default_privacy, default_sensitive, default_language FROM users WHERE account_id = $1",
        auth.account_id,
    )
    .fetch_optional(&state.db)
    .await?;

    let (privacy, sensitive, language) = user.map_or(
        ("public".to_string(), false, None),
        |u| (u.default_privacy, u.default_sensitive, u.default_language),
    );

    Ok(Json(Preferences {
        posting_default_visibility: privacy,
        posting_default_sensitive: sensitive,
        posting_default_language: language,
        reading_expand_media: "default".into(),
        reading_expand_spoilers: false,
    }))
}

// ── GET /api/v1/follow_requests ───────────────────────────────────────────

pub async fn get_follow_requests(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    uri: Uri,
    req_headers: HeaderMap,
    Query(q): Query<PaginationParams>,
) -> AppResult<impl IntoResponse> {
    auth.require_scope("read:follows")?;
    let limit = q.limit_clamped(40, 80);
    let max_id = q.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = q.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = q.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let accounts = sqlx::query_as!(
        Account,
        r#"SELECT a.* FROM accounts a
           JOIN follows f ON f.account_id = a.id
           WHERE f.target_account_id = $1 AND f.state = 'pending'
             AND ($2::bigint IS NULL OR a.id < $2)
             AND ($3::bigint IS NULL OR a.id > $3)
             AND ($5::bigint IS NULL OR a.id > $5)
           ORDER BY a.id DESC LIMIT $4"#,
        auth.account_id, max_id, since_id, limit, min_id
    )
    .fetch_all(&state.db)
    .await?;

    let api_accounts: Vec<ApiAccount> = accounts.iter().map(account_from_db).collect();
    let link = api_accounts.first().zip(api_accounts.last()).map(|(newest, oldest)| {
        let extra = super::non_pagination_query(uri.query());
        super::link_header(&req_headers, uri.path(), &extra, &newest.id, &oldest.id)
    });
    let mut resp_headers = HeaderMap::new();
    if let Some(v) = link {
        if let Ok(val) = v.parse() {
            resp_headers.insert(header::LINK, val);
        }
    }
    Ok((resp_headers, Json(api_accounts)))
}

// ── POST /api/v1/follow_requests/:id/authorize ────────────────────────────

pub async fn authorize_follow_request(
    State(state): State<AppState>,
    Path(requester_id): Path<i64>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
    auth.require_scope("write:follows")?;
    sqlx::query!(
        "UPDATE follows SET state = 'accepted' WHERE account_id = $1 AND target_account_id = $2 AND state = 'pending'",
        requester_id, auth.account_id
    )
    .execute(&state.db)
    .await?;

    sqlx::query!(
        "UPDATE accounts SET followers_count = followers_count + 1 WHERE id = $1",
        auth.account_id
    )
    .execute(&state.db)
    .await?;

    sqlx::query!(
        "UPDATE accounts SET following_count = following_count + 1 WHERE id = $1",
        requester_id
    )
    .execute(&state.db)
    .await?;

    let accepter = fetch_account(&state, auth.account_id).await?;
    push::create_and_push(
        &state,
        requester_id,
        auth.account_id,
        "follow",
        None,
        format!("{} accepted your follow request", accepter.display_name),
        accepter.acct().clone(),
        accepter.avatar.clone().unwrap_or_default(),
    ).await;

    // Backfill the requester's feed with recent statuses from the accepted account
    {
        let mut redis = state.redis.clone();
        let db = state.db.clone();
        let iid = instance.id;
        let followed_id = auth.account_id;
        if feed::sync_fanout() {
            feed::backfill_follow(&mut redis, &db, iid, requester_id, followed_id).await;
        } else {
            tokio::spawn(async move {
                feed::backfill_follow(&mut redis, &db, iid, requester_id, followed_id).await;
            });
        }
    }

    build_relationship(&state, auth.account_id, requester_id).await.map(Json)
}

// ── POST /api/v1/follow_requests/:id/reject ───────────────────────────────

pub async fn reject_follow_request(
    State(state): State<AppState>,
    Path(requester_id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
    auth.require_scope("write:follows")?;
    sqlx::query!(
        "DELETE FROM follows WHERE account_id = $1 AND target_account_id = $2 AND state = 'pending'",
        requester_id, auth.account_id
    )
    .execute(&state.db)
    .await?;

    build_relationship(&state, auth.account_id, requester_id).await.map(Json)
}

// ── Helpers ────────────────────────────────────────────────────────────────

pub async fn fetch_account(state: &AppState, id: i64) -> AppResult<Account> {
    sqlx::query_as!(Account, "SELECT * FROM accounts WHERE id = $1", id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn batch_status_media(
    state: &AppState,
    status_ids: &[i64],
) -> AppResult<std::collections::HashMap<i64, Vec<crate::db::models::MediaAttachment>>> {
    if status_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let rows = sqlx::query_as!(
        crate::db::models::MediaAttachment,
        "SELECT * FROM media_attachments WHERE status_id = ANY($1::bigint[]) ORDER BY id",
        status_ids,
    )
    .fetch_all(&state.db)
    .await?;
    let mut map: std::collections::HashMap<i64, Vec<_>> = std::collections::HashMap::new();
    for m in rows {
        if let Some(sid) = m.status_id {
            map.entry(sid).or_default().push(m);
        }
    }
    Ok(map)
}

pub async fn batch_reblog_data(
    state: &AppState,
    statuses: &[crate::db::models::Status],
) -> AppResult<std::collections::HashMap<i64, (crate::db::models::Status, crate::db::models::Account, Vec<crate::db::models::MediaAttachment>)>> {
    use std::collections::{HashMap, HashSet};

    let reblog_ids: Vec<i64> = statuses.iter()
        .filter_map(|s| s.reblog_of_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    if reblog_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let reblog_statuses = sqlx::query_as!(
        crate::db::models::Status,
        "SELECT * FROM statuses WHERE id = ANY($1::bigint[]) AND deleted_at IS NULL",
        &reblog_ids,
    )
    .fetch_all(&state.db)
    .await?;

    let reblog_account_ids: Vec<i64> = reblog_statuses.iter()
        .map(|s| s.account_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let reblog_accounts = sqlx::query_as!(
        Account,
        "SELECT * FROM accounts WHERE id = ANY($1::bigint[])",
        &reblog_account_ids,
    )
    .fetch_all(&state.db)
    .await?;

    let reblog_account_map: HashMap<i64, Account> = reblog_accounts
        .into_iter()
        .map(|a| (a.id, a))
        .collect();

    let reblog_status_ids: Vec<i64> = reblog_statuses.iter().map(|s| s.id).collect();
    let reblog_media = batch_status_media(state, &reblog_status_ids).await?;

    let reblog_status_map: HashMap<i64, crate::db::models::Status> = reblog_statuses
        .into_iter()
        .map(|s| (s.id, s))
        .collect();

    let mut result = HashMap::new();
    for s in statuses {
        if let Some(reblog_id) = s.reblog_of_id {
            if let Some(rs) = reblog_status_map.get(&reblog_id) {
                if let Some(ra) = reblog_account_map.get(&rs.account_id) {
                    let media = reblog_media.get(&reblog_id).cloned().unwrap_or_default();
                    result.insert(s.id, (rs.clone(), ra.clone(), media));
                }
            }
        }
    }
    Ok(result)
}

pub async fn fetch_status_poll(
    state: &AppState,
    status_id: i64,
    viewer_id: Option<i64>,
) -> AppResult<Option<super::types::Poll>> {
    let row = sqlx::query!(
        "SELECT id, options, multiple, votes_count, voters_count, expires_at FROM polls WHERE status_id = $1",
        status_id,
    )
    .fetch_optional(&state.db)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let now = chrono::Utc::now();
    let expired = row.expires_at.map_or(false, |t| t < now);

    let options: Vec<super::types::PollOption> = row.options
        .as_array()
        .map(|arr| arr.iter().map(|o| super::types::PollOption {
            title: o["title"].as_str().unwrap_or("").to_string(),
            votes_count: o["votes_count"].as_i64(),
        }).collect())
        .unwrap_or_default();

    let (voted, own_votes) = if let Some(vid) = viewer_id {
        let votes = sqlx::query!(
            "SELECT choice FROM poll_votes WHERE poll_id = $1 AND account_id = $2 ORDER BY choice",
            row.id, vid,
        )
        .fetch_all(&state.db)
        .await?;
        if votes.is_empty() {
            (Some(false), Some(vec![]))
        } else {
            let choices: Vec<i32> = votes.iter().map(|v| v.choice).collect();
            (Some(true), Some(choices))
        }
    } else {
        (None, None)
    };

    Ok(Some(super::types::Poll {
        id: row.id.to_string(),
        expires_at: row.expires_at.map(|t| t.to_rfc3339()),
        expired,
        multiple: row.multiple,
        votes_count: row.votes_count,
        voters_count: row.voters_count,
        options,
        emojis: vec![],
        voted,
        own_votes,
    }))
}

pub async fn fetch_status_media(
    state: &AppState,
    status_id: i64,
) -> AppResult<Vec<crate::db::models::MediaAttachment>> {
    Ok(sqlx::query_as!(
        crate::db::models::MediaAttachment,
        "SELECT * FROM media_attachments WHERE status_id = $1 ORDER BY id",
        status_id,
    )
    .fetch_all(&state.db)
    .await?)
}

pub async fn fetch_reblog_data(
    state: &AppState,
    status: &crate::db::models::Status,
) -> AppResult<Option<(crate::db::models::Status, Account, Vec<crate::db::models::MediaAttachment>)>> {
    let Some(reblog_id) = status.reblog_of_id else {
        return Ok(None);
    };
    let reblog = sqlx::query_as!(
        crate::db::models::Status,
        "SELECT * FROM statuses WHERE id = $1 AND deleted_at IS NULL",
        reblog_id,
    )
    .fetch_optional(&state.db)
    .await?;
    let Some(reblog) = reblog else {
        return Ok(None);
    };
    let reblog_account = sqlx::query_as!(
        Account,
        "SELECT * FROM accounts WHERE id = $1",
        reblog.account_id,
    )
    .fetch_one(&state.db)
    .await?;
    let reblog_media = fetch_status_media(state, reblog.id).await?;
    Ok(Some((reblog, reblog_account, reblog_media)))
}

async fn batch_build_relationships(state: &AppState, source_id: i64, target_ids: &[i64]) -> AppResult<Vec<Relationship>> {
    struct FollowRow { state: String, show_reblogs: bool, notify: bool, languages: Option<Vec<String>> }
    struct MuteRow { hide_notifications: bool, expires_at: Option<chrono::DateTime<chrono::Utc>> }

    let follows_out = sqlx::query!(
        "SELECT target_account_id, state, show_reblogs, notify, languages FROM follows WHERE account_id = $1 AND target_account_id = ANY($2::bigint[])",
        source_id, target_ids,
    )
    .fetch_all(&state.db)
    .await?;
    let follows_out_map: std::collections::HashMap<i64, _> = follows_out.into_iter()
        .map(|r| (r.target_account_id, FollowRow {
            state: r.state,
            show_reblogs: r.show_reblogs,
            notify: r.notify,
            languages: if r.languages.is_empty() { None } else { Some(r.languages) },
        }))
        .collect();

    let follows_in = sqlx::query!(
        "SELECT account_id, state FROM follows WHERE target_account_id = $1 AND account_id = ANY($2::bigint[])",
        source_id, target_ids,
    )
    .fetch_all(&state.db)
    .await?;
    let followed_by_set: std::collections::HashSet<i64> = follows_in.iter().filter(|r| r.state == "accepted").map(|r| r.account_id).collect();
    let requested_by_set: std::collections::HashSet<i64> = follows_in.iter().filter(|r| r.state == "pending").map(|r| r.account_id).collect();

    let blocks_out: std::collections::HashSet<i64> = sqlx::query_scalar!(
        "SELECT target_account_id FROM blocks WHERE account_id = $1 AND target_account_id = ANY($2::bigint[])",
        source_id, target_ids,
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .collect();

    let blocks_in: std::collections::HashSet<i64> = sqlx::query_scalar!(
        "SELECT account_id FROM blocks WHERE target_account_id = $1 AND account_id = ANY($2::bigint[])",
        source_id, target_ids,
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .collect();

    let mutes = sqlx::query!(
        "SELECT target_account_id, hide_notifications, expires_at FROM mutes WHERE account_id = $1 AND target_account_id = ANY($2::bigint[]) AND (expires_at IS NULL OR expires_at > now())",
        source_id, target_ids,
    )
    .fetch_all(&state.db)
    .await?;
    let mutes_map: std::collections::HashMap<i64, MuteRow> = mutes.into_iter()
        .map(|r| (r.target_account_id, MuteRow { hide_notifications: r.hide_notifications, expires_at: r.expires_at }))
        .collect();

    let target_domains: std::collections::HashMap<i64, Option<String>> = sqlx::query!(
        "SELECT id, domain FROM accounts WHERE id = ANY($1::bigint[])",
        target_ids,
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|r| (r.id, r.domain))
    .collect();

    let domains_to_check: Vec<String> = target_domains.values().filter_map(|d| d.clone()).collect();
    let domain_blocked_set: std::collections::HashSet<String> = if domains_to_check.is_empty() {
        Default::default()
    } else {
        sqlx::query_scalar!(
            "SELECT domain FROM user_domain_blocks WHERE account_id = $1 AND domain = ANY($2)",
            source_id, &domains_to_check,
        )
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .collect()
    };

    let notes: std::collections::HashMap<i64, String> = sqlx::query!(
        "SELECT target_account_id, comment FROM account_notes WHERE account_id = $1 AND target_account_id = ANY($2::bigint[])",
        source_id, target_ids,
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|r| (r.target_account_id, r.comment))
    .collect();

    let endorsed_set: std::collections::HashSet<i64> = sqlx::query_scalar!(
        "SELECT target_account_id FROM account_pins WHERE account_id = $1 AND target_account_id = ANY($2::bigint[])",
        source_id, target_ids,
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .collect();

    let mut results = Vec::with_capacity(target_ids.len());
    for &target_id in target_ids {
        let follow = follows_out_map.get(&target_id);
        let mute = mutes_map.get(&target_id);
        let domain = target_domains.get(&target_id).and_then(|d| d.clone());
        let domain_blocking = domain.map_or(false, |d| domain_blocked_set.contains(&d));
        results.push(Relationship {
            id: target_id.to_string(),
            following: follow.map_or(false, |f| f.state == "accepted"),
            showing_reblogs: follow.map_or(true, |f| f.show_reblogs),
            notifying: follow.map_or(false, |f| f.notify),
            languages: follow.and_then(|f| f.languages.clone()),
            followed_by: followed_by_set.contains(&target_id),
            blocking: blocks_out.contains(&target_id),
            blocked_by: blocks_in.contains(&target_id),
            muting: mute.is_some(),
            muting_notifications: mute.map_or(false, |m| m.hide_notifications),
            muting_expires_at: mute.and_then(|m| m.expires_at).map(|t| t.to_rfc3339()),
            requested: follow.map_or(false, |f| f.state == "pending"),
            requested_by: requested_by_set.contains(&target_id),
            domain_blocking,
            endorsed: endorsed_set.contains(&target_id),
            note: notes.get(&target_id).cloned().unwrap_or_default(),
        });
    }
    Ok(results)
}

async fn build_relationship(state: &AppState, source_id: i64, target_id: i64) -> AppResult<Relationship> {
    let follow = sqlx::query!(
        "SELECT state, show_reblogs, notify, languages FROM follows WHERE account_id = $1 AND target_account_id = $2",
        source_id, target_id
    )
    .fetch_optional(&state.db)
    .await?;

    let followed_by = sqlx::query!(
        "SELECT 1 as exists FROM follows WHERE account_id = $1 AND target_account_id = $2 AND state = 'accepted'",
        target_id, source_id
    )
    .fetch_optional(&state.db)
    .await?
    .is_some();

    let blocking = sqlx::query!(
        "SELECT 1 as exists FROM blocks WHERE account_id = $1 AND target_account_id = $2",
        source_id, target_id
    )
    .fetch_optional(&state.db)
    .await?
    .is_some();

    let blocked_by = sqlx::query!(
        "SELECT 1 as exists FROM blocks WHERE account_id = $1 AND target_account_id = $2",
        target_id, source_id
    )
    .fetch_optional(&state.db)
    .await?
    .is_some();

    let requested_by = sqlx::query!(
        "SELECT 1 as exists FROM follows WHERE account_id = $1 AND target_account_id = $2 AND state = 'pending'",
        target_id, source_id
    )
    .fetch_optional(&state.db)
    .await?
    .is_some();

    let muting = sqlx::query!(
        "SELECT hide_notifications, expires_at FROM mutes WHERE account_id = $1 AND target_account_id = $2 AND (expires_at IS NULL OR expires_at > now())",
        source_id, target_id
    )
    .fetch_optional(&state.db)
    .await?;

    // Check if source has domain-blocked target's domain
    let target_domain = sqlx::query_scalar!(
        "SELECT domain FROM accounts WHERE id = $1",
        target_id
    )
    .fetch_optional(&state.db)
    .await?
    .flatten();

    let domain_blocking = if let Some(domain) = target_domain {
        sqlx::query!(
            "SELECT 1 as exists FROM user_domain_blocks WHERE account_id = $1 AND domain = $2",
            source_id, domain
        )
        .fetch_optional(&state.db)
        .await?
        .is_some()
    } else {
        false
    };

    let note = sqlx::query_scalar!(
        "SELECT comment FROM account_notes WHERE account_id = $1 AND target_account_id = $2",
        source_id, target_id
    )
    .fetch_optional(&state.db)
    .await?
    .unwrap_or_default();

    let showing_reblogs = follow.as_ref().map_or(true, |f| f.show_reblogs);
    let notifying = follow.as_ref().map_or(false, |f| f.notify);
    let languages = follow.as_ref().and_then(|f| if f.languages.is_empty() { None } else { Some(f.languages.clone()) });
    let muting_expires_at = muting.as_ref().and_then(|m| m.expires_at)
        .map(|t| t.to_rfc3339());

    Ok(Relationship {
        id: target_id.to_string(),
        following: follow.as_ref().map_or(false, |f| f.state == "accepted"),
        showing_reblogs,
        notifying,
        languages,
        followed_by,
        blocking,
        blocked_by,
        muting: muting.is_some(),
        muting_notifications: muting.map_or(false, |m| m.hide_notifications),
        muting_expires_at,
        requested: follow.as_ref().map_or(false, |f| f.state == "pending"),
        requested_by,
        domain_blocking,
        endorsed: sqlx::query!(
            "SELECT 1 AS e FROM account_pins WHERE account_id = $1 AND target_account_id = $2",
            source_id, target_id
        )
        .fetch_optional(&state.db)
        .await?
        .is_some(),
        note,
    })
}

// ── GET /api/v1/suggestions ────────────────────────────────────────────────

pub async fn get_suggestions(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Extension(auth): Extension<AuthenticatedUser>,
    Query(params): Query<PaginationParams>,
) -> AppResult<Json<Vec<ApiAccount>>> {
    let limit = params.limit_clamped(40, 80);

    let accounts = sqlx::query_as!(
        Account,
        r#"SELECT a.* FROM accounts a
           JOIN follows f ON f.account_id = a.id
           WHERE f.target_account_id = $1
             AND f.state = 'accepted'
             AND a.instance_id = $2
             AND a.domain IS NULL
             AND NOT EXISTS (
               SELECT 1 FROM follows f2
               WHERE f2.account_id = $1 AND f2.target_account_id = a.id
             )
           ORDER BY f.created_at DESC
           LIMIT $3"#,
        auth.account_id,
        instance.id,
        limit,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(accounts.iter().map(account_from_db).collect()))
}

// ── DELETE /api/v1/suggestions/:account_id ────────────────────────────────

pub async fn dismiss_suggestion(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(account_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    sqlx::query!(
        r#"INSERT INTO suggestion_dismissals (account_id, target_account_id)
           VALUES ($1, $2) ON CONFLICT DO NOTHING"#,
        auth.account_id, account_id,
    )
    .execute(&state.db)
    .await?;
    Ok(Json(serde_json::json!({})))
}

// ── GET /api/v2/suggestions ───────────────────────────────────────────────

pub async fn get_suggestions_v2(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Extension(auth): Extension<AuthenticatedUser>,
    Query(params): Query<PaginationParams>,
) -> AppResult<Json<Vec<SuggestionV2>>> {
    let limit = params.limit_clamped(40, 80);

    let accounts = sqlx::query_as!(
        Account,
        r#"SELECT a.* FROM accounts a
           JOIN follows f ON f.account_id = a.id
           WHERE f.target_account_id = $1
             AND f.state = 'accepted'
             AND a.instance_id = $2
             AND a.domain IS NULL
             AND NOT EXISTS (
               SELECT 1 FROM follows f2
               WHERE f2.account_id = $1 AND f2.target_account_id = a.id
             )
             AND NOT EXISTS (
               SELECT 1 FROM suggestion_dismissals sd
               WHERE sd.account_id = $1 AND sd.target_account_id = a.id
             )
           ORDER BY f.created_at DESC
           LIMIT $3"#,
        auth.account_id,
        instance.id,
        limit,
    )
    .fetch_all(&state.db)
    .await?;

    let suggestions = accounts
        .iter()
        .map(|a| SuggestionV2 {
            source: "friends_of_friends".to_string(),
            sources: vec!["friends_of_friends".to_string()],
            account: account_from_db(a),
        })
        .collect();

    Ok(Json(suggestions))
}

// ── POST /api/v1/accounts/move ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct MoveAccountForm {
    pub acct: String,
    pub current_password: String,
}

pub async fn move_account(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<MoveAccountForm>,
) -> AppResult<Json<serde_json::Value>> {
    // Verify password
    let user = sqlx::query!(
        "SELECT password_hash FROM users WHERE account_id = $1",
        auth.account_id
    )
    .fetch_one(&state.db)
    .await?;

    let valid = crate::crypto::verify_password(&form.current_password, &user.password_hash).is_ok();
    if !valid {
        return Err(AppError::Unauthorized);
    }

    // Look up target account URI (by acct handle or URL)
    let target_uri = form.acct.clone();
    sqlx::query!(
        "UPDATE accounts SET moved_to_uri = $1, updated_at = now() WHERE id = $2",
        target_uri, auth.account_id,
    )
    .execute(&state.db)
    .await?;

    Ok(Json(serde_json::json!({})))
}

// ── GET /api/v1/profile/aliases ───────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AccountAlias {
    pub id: String,
    pub account_id: String,
    pub uri: String,
    pub created_at: String,
}

pub async fn list_aliases(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<AccountAlias>>> {
    let rows = sqlx::query!(
        "SELECT id, account_id, uri, created_at FROM account_aliases WHERE account_id = $1 ORDER BY created_at",
        auth.account_id,
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(|r| AccountAlias {
        id: r.id.to_string(),
        account_id: r.account_id.to_string(),
        uri: r.uri,
        created_at: r.created_at.to_rfc3339(),
    }).collect()))
}

#[derive(Debug, Deserialize)]
pub struct CreateAliasForm {
    pub acct: String,
}

pub async fn create_alias(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<CreateAliasForm>,
) -> AppResult<Json<AccountAlias>> {
    let r = sqlx::query!(
        r#"INSERT INTO account_aliases (account_id, uri) VALUES ($1, $2)
           ON CONFLICT (account_id, uri) DO UPDATE SET updated_at = now()
           RETURNING id, account_id, uri, created_at"#,
        auth.account_id, form.acct,
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(AccountAlias {
        id: r.id.to_string(),
        account_id: r.account_id.to_string(),
        uri: r.uri,
        created_at: r.created_at.to_rfc3339(),
    }))
}

pub async fn delete_alias(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    sqlx::query!(
        "DELETE FROM account_aliases WHERE id = $1 AND account_id = $2",
        id, auth.account_id,
    )
    .execute(&state.db)
    .await?;
    Ok(Json(serde_json::json!({})))
}

// ── POST /api/v1/accounts/:id/note ────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct NoteForm {
    pub comment: Option<String>,
}

pub async fn set_account_note(
    State(state): State<AppState>,
    Path(target_id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<NoteForm>,
) -> AppResult<Json<Relationship>> {
    let comment = form.comment.unwrap_or_default();
    sqlx::query!(
        r#"INSERT INTO account_notes (account_id, target_account_id, comment)
           VALUES ($1, $2, $3)
           ON CONFLICT (account_id, target_account_id)
           DO UPDATE SET comment = EXCLUDED.comment, updated_at = now()"#,
        auth.account_id,
        target_id,
        comment,
    )
    .execute(&state.db)
    .await?;

    build_relationship(&state, auth.account_id, target_id).await.map(Json)
}

// ── POST /api/v1/accounts/:id/remove_from_followers ───────────────────────

pub async fn remove_from_followers(
    State(state): State<AppState>,
    Path(requester_id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
    let deleted = sqlx::query!(
        "DELETE FROM follows WHERE account_id = $1 AND target_account_id = $2 AND state = 'accepted' RETURNING 1 as exists",
        requester_id,
        auth.account_id,
    )
    .fetch_optional(&state.db)
    .await?;

    if deleted.is_some() {
        sqlx::query!(
            "UPDATE accounts SET followers_count = GREATEST(followers_count - 1, 0) WHERE id = $1",
            auth.account_id
        )
        .execute(&state.db)
        .await?;
        sqlx::query!(
            "UPDATE accounts SET following_count = GREATEST(following_count - 1, 0) WHERE id = $1",
            requester_id
        )
        .execute(&state.db)
        .await?;
    }

    build_relationship(&state, auth.account_id, requester_id).await.map(Json)
}

// ── POST /api/v1/accounts/:id/endorse ────────────────────────────────────

pub async fn endorse_account(
    State(state): State<AppState>,
    Path(target_id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
    sqlx::query!(
        "INSERT INTO account_pins (account_id, target_account_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        auth.account_id, target_id,
    )
    .execute(&state.db)
    .await?;
    build_relationship(&state, auth.account_id, target_id).await.map(Json)
}

// ── POST /api/v1/accounts/:id/unendorse ──────────────────────────────────

pub async fn unendorse_account(
    State(state): State<AppState>,
    Path(target_id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
    sqlx::query!(
        "DELETE FROM account_pins WHERE account_id = $1 AND target_account_id = $2",
        auth.account_id, target_id,
    )
    .execute(&state.db)
    .await?;
    build_relationship(&state, auth.account_id, target_id).await.map(Json)
}

// ── GET /api/v1/accounts/:id/endorsements ────────────────────────────────

pub async fn get_endorsements(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Vec<ApiAccount>>> {
    let accounts = sqlx::query_as!(
        Account,
        r#"SELECT a.* FROM accounts a
           JOIN account_pins ap ON ap.target_account_id = a.id
           WHERE ap.account_id = $1
           ORDER BY ap.created_at DESC"#,
        id,
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(accounts.iter().map(account_from_db).collect()))
}

// ── GET /api/v1/endorsements ──────────────────────────────────────────────

pub async fn get_my_endorsements(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<ApiAccount>>> {
    auth.require_scope("read:accounts")?;
    let accounts = sqlx::query_as!(
        Account,
        r#"SELECT a.* FROM accounts a
           JOIN account_pins ap ON ap.target_account_id = a.id
           WHERE ap.account_id = $1
           ORDER BY ap.created_at DESC"#,
        auth.account_id,
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(accounts.iter().map(account_from_db).collect()))
}

// ── GET /api/v1/accounts/:id/featured_tags ───────────────────────────────

pub async fn get_account_featured_tags(
    State(state): State<AppState>,
    Extension(crate::middleware::ResolvedInstance(instance)): Extension<crate::middleware::ResolvedInstance>,
    Path(id): Path<i64>,
) -> AppResult<Json<Vec<super::types::FeaturedTag>>> {
    let domain = instance.custom_domain.as_deref().unwrap_or(&instance.domain);
    let rows = sqlx::query!(
        r#"SELECT ft.id, t.name, ft.statuses_count, ft.last_status_at
           FROM featured_tags ft
           JOIN tags t ON t.id = ft.tag_id
           WHERE ft.account_id = $1
           ORDER BY ft.id"#,
        id,
    )
    .fetch_all(&state.db)
    .await?;
    let tags = rows
        .into_iter()
        .map(|r| super::types::FeaturedTag {
            id: r.id.to_string(),
            name: r.name.clone(),
            url: format!("https://{}/tags/{}", domain, r.name),
            statuses_count: r.statuses_count,
            last_status_at: r.last_status_at.map(|t| t.format("%Y-%m-%d").to_string()),
        })
        .collect();
    Ok(Json(tags))
}

// ── PUT /api/v1/profile (tab display settings) ───────────────────────────
// show_featured / show_media / show_media_replies stored in DB; for now stub.

pub async fn update_profile_settings(
    Extension(auth): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(_body): Json<serde_json::Value>,
) -> AppResult<Json<super::types::Account>> {
    let account = sqlx::query_as!(
        crate::db::models::Account,
        "SELECT * FROM accounts WHERE id = $1",
        auth.account_id,
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(account_from_db(&account)))
}

// ── DELETE /api/v1/profile/avatar ────────────────────────────────────────

pub async fn delete_profile_avatar(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<super::types::Account>> {
    sqlx::query!(
        "UPDATE accounts SET avatar = NULL, avatar_static = NULL WHERE id = $1",
        auth.account_id,
    )
    .execute(&state.db)
    .await?;
    let account = sqlx::query_as!(
        crate::db::models::Account,
        "SELECT * FROM accounts WHERE id = $1",
        auth.account_id,
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(account_from_db(&account)))
}

// ── DELETE /api/v1/profile/header ────────────────────────────────────────

pub async fn delete_profile_header(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<super::types::Account>> {
    sqlx::query!(
        "UPDATE accounts SET header = NULL, header_static = NULL WHERE id = $1",
        auth.account_id,
    )
    .execute(&state.db)
    .await?;
    let account = sqlx::query_as!(
        crate::db::models::Account,
        "SELECT * FROM accounts WHERE id = $1",
        auth.account_id,
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(account_from_db(&account)))
}

// ── GET /api/v1/accounts/familiar_followers ──────────────────────────────

pub async fn get_familiar_followers(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    RawQuery(qs): RawQuery,
) -> AppResult<Json<Vec<super::types::FamiliarFollowers>>> {
    let mut seen = std::collections::HashSet::new();
    let ids: Vec<i64> = url::form_urlencoded::parse(
            qs.as_deref().unwrap_or("").as_bytes()
        )
        .filter(|(k, _)| k == "id[]" || k == "id")
        .filter_map(|(_, v)| v.parse::<i64>().ok())
        .filter(|id| seen.insert(*id))
        .collect();

    let mut result = Vec::with_capacity(ids.len());
    for target_id in &ids {
        // Find followers of target_id that also follow the viewer (auth.account_id)
        // Find accounts that: (1) follow the target, and (2) are followed by the viewer
        let accounts = sqlx::query_as!(
            crate::db::models::Account,
            r#"SELECT a.* FROM accounts a
               JOIN follows f1 ON f1.account_id = a.id AND f1.target_account_id = $1 AND f1.state = 'accepted'
               JOIN follows f2 ON f2.account_id = $2 AND f2.target_account_id = a.id AND f2.state = 'accepted'
               LIMIT 10"#,
            target_id,
            auth.account_id,
        )
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        result.push(super::types::FamiliarFollowers {
            id: target_id.to_string(),
            accounts: accounts.iter().map(account_from_db).collect(),
        });
    }
    Ok(Json(result))
}

// ── GET /api/v1/directory ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DirectoryQuery {
    pub offset: Option<i64>,
    pub limit: Option<i64>,
    pub order: Option<String>,
    pub local: Option<bool>,
}

pub async fn get_directory(
    State(state): State<AppState>,
    Extension(crate::middleware::ResolvedInstance(instance)): Extension<crate::middleware::ResolvedInstance>,
    Query(q): Query<DirectoryQuery>,
) -> AppResult<Json<Vec<ApiAccount>>> {
    let limit = q.limit.unwrap_or(40).min(80).max(1);
    let offset = q.offset.unwrap_or(0).max(0);
    let local_only = q.local.unwrap_or(true);
    let order = q.order.as_deref().unwrap_or("active");

    let accounts = if order == "new" {
        sqlx::query_as!(
            Account,
            r#"SELECT * FROM accounts
               WHERE instance_id = $1
                 AND discoverable = true
                 AND suspended_at IS NULL
                 AND (NOT $2::bool OR domain IS NULL)
                 AND (domain IS NULL OR NOT EXISTS (
                     SELECT 1 FROM domain_blocks db WHERE db.domain = domain
                 ))
               ORDER BY created_at DESC
               LIMIT $3 OFFSET $4"#,
            instance.id, local_only, limit, offset,
        )
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as!(
            Account,
            r#"SELECT a.* FROM accounts a
               WHERE a.instance_id = $1
                 AND a.discoverable = true
                 AND a.suspended_at IS NULL
                 AND (NOT $2::bool OR a.domain IS NULL)
                 AND (a.domain IS NULL OR NOT EXISTS (
                     SELECT 1 FROM domain_blocks db WHERE db.domain = a.domain
                 ))
               ORDER BY (
                   SELECT MAX(s.created_at) FROM statuses s
                   WHERE s.account_id = a.id AND s.deleted_at IS NULL
               ) DESC NULLS LAST
               LIMIT $3 OFFSET $4"#,
            instance.id, local_only, limit, offset,
        )
        .fetch_all(&state.db)
        .await?
    };

    Ok(Json(accounts.iter().map(account_from_db).collect()))
}

// ── GET /api/v1/accounts (batch lookup) ──────────────────────────────────

pub async fn get_accounts_batch(
    State(state): State<AppState>,
    RawQuery(qs): RawQuery,
) -> AppResult<Json<Vec<ApiAccount>>> {
    // serde_urlencoded treats id[]=v1&id[]=v2 as a duplicate field → 400.
    // Parse with form_urlencoded which correctly returns each pair separately.
    let ids: Vec<i64> = url::form_urlencoded::parse(
            qs.as_deref().unwrap_or("").as_bytes()
        )
        .filter(|(k, _)| k == "id[]" || k == "id")
        .filter_map(|(_, v)| v.parse::<i64>().ok())
        .collect();

    if ids.is_empty() {
        return Ok(Json(vec![]));
    }
    let accounts = sqlx::query_as!(
        crate::db::models::Account,
        "SELECT * FROM accounts WHERE id = ANY($1::bigint[]) ORDER BY created_at DESC",
        &ids,
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(accounts.iter().map(account_from_db).collect()))
}

// ── GET /api/v1/accounts/:id/lists ───────────────────────────────────────

pub async fn get_account_lists(
    State(state): State<AppState>,
    Path(target_id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<super::types::List>>> {
    let rows = sqlx::query!(
        r#"SELECT l.id, l.title, l.replies_policy, l.exclusive
           FROM lists l
           JOIN list_accounts la ON la.list_id = l.id
           WHERE l.account_id = $1 AND la.account_id = $2
           ORDER BY l.id"#,
        auth.account_id,
        target_id,
    )
    .fetch_all(&state.db)
    .await?;

    let lists = rows
        .into_iter()
        .map(|r| super::types::List {
            id: r.id.to_string(),
            title: r.title,
            replies_policy: r.replies_policy,
            exclusive: r.exclusive,
        })
        .collect();

    Ok(Json(lists))
}

// ── Tag / mention fetchers ─────────────────────────────────────────────────

pub async fn fetch_status_tags(
    state: &AppState,
    status_id: i64,
) -> AppResult<Vec<super::types::StatusTag>> {
    let rows = sqlx::query!(
        r#"SELECT t.name, i.domain
           FROM tags t
           JOIN status_tags st ON st.tag_id = t.id
           JOIN statuses s ON s.id = st.status_id
           JOIN instances i ON i.id = s.instance_id
           WHERE st.status_id = $1"#,
        status_id,
    )
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(|r| {
        let tag_lower = r.name.to_lowercase();
        super::types::StatusTag {
            url: format!("https://{}/tags/{}", r.domain, urlencoding::encode(&tag_lower)),
            name: r.name,
        }
    }).collect())
}

pub async fn fetch_status_mentions(
    state: &AppState,
    status_id: i64,
) -> AppResult<Vec<super::types::StatusMention>> {
    let rows = sqlx::query!(
        r#"SELECT a.id as account_id, a.username, a.domain, a.url
           FROM accounts a
           JOIN mentions m ON m.account_id = a.id
           WHERE m.status_id = $1"#,
        status_id,
    )
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(|r| super::types::StatusMention {
        id: r.account_id.to_string(),
        acct: match &r.domain {
            Some(d) => format!("{}@{}", r.username, d),
            None => r.username.clone(),
        },
        url: r.url,
        username: r.username,
    }).collect())
}

pub async fn batch_status_tags(
    state: &AppState,
    status_ids: &[i64],
) -> AppResult<std::collections::HashMap<i64, Vec<super::types::StatusTag>>> {
    if status_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let rows = sqlx::query!(
        r#"SELECT st.status_id, t.name, i.domain
           FROM tags t
           JOIN status_tags st ON st.tag_id = t.id
           JOIN statuses s ON s.id = st.status_id
           JOIN instances i ON i.id = s.instance_id
           WHERE st.status_id = ANY($1::bigint[])"#,
        status_ids,
    )
    .fetch_all(&state.db)
    .await?;
    let mut map: std::collections::HashMap<i64, Vec<super::types::StatusTag>> = std::collections::HashMap::new();
    for r in rows {
        let tag_lower = r.name.to_lowercase();
        map.entry(r.status_id).or_default().push(super::types::StatusTag {
            url: format!("https://{}/tags/{}", r.domain, urlencoding::encode(&tag_lower)),
            name: r.name,
        });
    }
    Ok(map)
}

pub async fn batch_status_mentions(
    state: &AppState,
    status_ids: &[i64],
) -> AppResult<std::collections::HashMap<i64, Vec<super::types::StatusMention>>> {
    if status_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let rows = sqlx::query!(
        r#"SELECT m.status_id, a.id as account_id, a.username, a.domain, a.url
           FROM accounts a
           JOIN mentions m ON m.account_id = a.id
           WHERE m.status_id = ANY($1::bigint[])"#,
        status_ids,
    )
    .fetch_all(&state.db)
    .await?;
    let mut map: std::collections::HashMap<i64, Vec<super::types::StatusMention>> = std::collections::HashMap::new();
    for r in rows {
        map.entry(r.status_id).or_default().push(super::types::StatusMention {
            id: r.account_id.to_string(),
            acct: match &r.domain {
                Some(d) => format!("{}@{}", r.username, d),
                None => r.username.clone(),
            },
            url: r.url,
            username: r.username,
        });
    }
    Ok(map)
}

pub async fn batch_status_emojis(
    state: &AppState,
    statuses: &[crate::db::models::Status],
) -> AppResult<std::collections::HashMap<i64, Vec<super::types::CustomEmoji>>> {
    if statuses.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    fn extract_shortcodes(text: &str) -> Vec<String> {
        let mut codes = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find(':') {
            rest = &rest[start + 1..];
            if let Some(end) = rest.find(':') {
                let code = &rest[..end];
                if !code.is_empty() && code.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    codes.push(code.to_string());
                }
                rest = &rest[end + 1..];
            } else {
                break;
            }
        }
        codes
    }

    // Group statuses by instance_id; collect all shortcodes per status
    let mut per_instance: std::collections::HashMap<uuid::Uuid, Vec<(i64, Vec<String>)>> = std::collections::HashMap::new();
    for s in statuses {
        let combined = format!("{} {}", s.spoiler_text, s.text);
        let codes = extract_shortcodes(&combined);
        if !codes.is_empty() {
            per_instance.entry(s.instance_id).or_default().push((s.id, codes));
        }
    }

    let mut map: std::collections::HashMap<i64, Vec<super::types::CustomEmoji>> = std::collections::HashMap::new();

    for (instance_id, id_codes) in per_instance {
        let all_codes: Vec<String> = id_codes.iter()
            .flat_map(|(_, codes)| codes.iter().cloned())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let rows = sqlx::query!(
            r#"SELECT shortcode, image_url, static_image_url, visible_in_picker
               FROM custom_emojis
               WHERE instance_id = $1 AND shortcode = ANY($2) AND NOT disabled"#,
            instance_id,
            &all_codes,
        )
        .fetch_all(&state.db)
        .await?;

        let emoji_by_code: std::collections::HashMap<String, super::types::CustomEmoji> = rows
            .into_iter()
            .map(|r| (r.shortcode.clone(), super::types::CustomEmoji {
                shortcode: r.shortcode,
                url: r.image_url.clone(),
                static_url: r.static_image_url.unwrap_or(r.image_url),
                visible_in_picker: r.visible_in_picker,
                category: None,
            }))
            .collect();

        for (status_id, codes) in id_codes {
            let unique_codes: std::collections::HashSet<&String> = codes.iter().collect();
            let emojis: Vec<super::types::CustomEmoji> = unique_codes.iter()
                .filter_map(|c| emoji_by_code.get(*c).cloned())
                .collect();
            if !emojis.is_empty() {
                map.insert(status_id, emojis);
            }
        }
    }

    Ok(map)
}

/// Batch-fetch polls for a list of status IDs. Returns map from status_id → Poll.
pub async fn batch_status_polls(
    state: &AppState,
    status_ids: &[i64],
    viewer_id: Option<i64>,
) -> AppResult<std::collections::HashMap<i64, super::types::Poll>> {
    use std::collections::HashMap;

    if status_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query!(
        r#"SELECT id, status_id, options, multiple, votes_count, voters_count, expires_at
           FROM polls WHERE status_id = ANY($1::bigint[])"#,
        status_ids,
    )
    .fetch_all(&state.db)
    .await?;

    if rows.is_empty() {
        return Ok(HashMap::new());
    }

    let poll_ids: Vec<uuid::Uuid> = rows.iter().map(|r| r.id).collect();
    let vote_rows = if let Some(vid) = viewer_id {
        sqlx::query!(
            "SELECT poll_id, choice FROM poll_votes WHERE poll_id = ANY($1::uuid[]) AND account_id = $2 ORDER BY poll_id, choice",
            &poll_ids, vid,
        )
        .fetch_all(&state.db)
        .await?
    } else {
        vec![]
    };

    let mut votes_by_poll: HashMap<uuid::Uuid, Vec<i32>> = HashMap::new();
    for v in vote_rows {
        votes_by_poll.entry(v.poll_id).or_default().push(v.choice);
    }

    let now = chrono::Utc::now();
    let mut result = HashMap::new();
    for row in rows {
        let expired = row.expires_at.map_or(false, |t| t < now);
        let options: Vec<super::types::PollOption> = row.options
            .as_array()
            .map(|arr| arr.iter().map(|o| super::types::PollOption {
                title: o["title"].as_str().unwrap_or("").to_string(),
                votes_count: o["votes_count"].as_i64(),
            }).collect())
            .unwrap_or_default();
        let (voted, own_votes) = if viewer_id.is_some() {
            let votes = votes_by_poll.get(&row.id).cloned().unwrap_or_default();
            if votes.is_empty() {
                (Some(false), None)
            } else {
                (Some(true), Some(votes))
            }
        } else {
            (None, None)
        };
        result.insert(row.status_id, super::types::Poll {
            id: row.id.to_string(),
            expires_at: row.expires_at.map(|t| t.to_rfc3339()),
            expired,
            multiple: row.multiple,
            votes_count: row.votes_count,
            voters_count: row.voters_count,
            options,
            emojis: vec![],
            voted,
            own_votes,
        });
    }
    Ok(result)
}

/// Batch-fetch preview cards for a list of status IDs. Returns map from status_id → PreviewCard.
pub async fn batch_status_cards(
    state: &AppState,
    status_ids: &[i64],
) -> AppResult<std::collections::HashMap<i64, super::types::PreviewCard>> {
    use std::collections::HashMap;

    if status_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query!(
        r#"SELECT spc.status_id, pc.url, pc.title, pc.description, pc.card_type,
                  pc.image_url, pc.author_name, pc.author_url,
                  pc.provider_name, pc.provider_url, pc.html, pc.width, pc.height,
                  pc.embed_url, pc.blurhash
           FROM status_preview_cards spc
           JOIN preview_cards pc ON pc.id = spc.card_id
           WHERE spc.status_id = ANY($1::bigint[])"#,
        status_ids,
    )
    .fetch_all(&state.db)
    .await?;

    let mut result = HashMap::new();
    for r in rows {
        result.entry(r.status_id).or_insert_with(|| super::types::PreviewCard {
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
            image: r.image_url,
            image_description: String::new(),
            embed_url: r.embed_url,
            blurhash: r.blurhash,
            published_at: None,
            authors: vec![],
        });
    }
    Ok(result)
}

/// Builds a `Status` API object with tags and mentions populated from the DB.
pub async fn build_status(
    state: &AppState,
    s: &crate::db::models::Status,
    account: &Account,
    media: Vec<crate::db::models::MediaAttachment>,
    reblog: Option<(crate::db::models::Status, Account, Vec<crate::db::models::MediaAttachment>)>,
    viewer_ctx: Option<super::convert::StatusViewerContext>,
) -> AppResult<super::types::Status> {
    build_status_with_app(state, s, account, media, reblog, viewer_ctx, None).await
}

pub async fn build_status_with_app(
    state: &AppState,
    s: &crate::db::models::Status,
    account: &Account,
    media: Vec<crate::db::models::MediaAttachment>,
    reblog: Option<(crate::db::models::Status, Account, Vec<crate::db::models::MediaAttachment>)>,
    viewer_ctx: Option<super::convert::StatusViewerContext>,
    application: Option<super::types::Application>,
) -> AppResult<super::types::Status> {
    let viewer_account_id = viewer_ctx.as_ref().map(|c| c.account_id);

    // Pre-fetch mentions and emojis for content rendering and API fields
    let mentions = fetch_status_mentions(state, s.id).await?;
    let status_emojis = fetch_status_emojis(state, s).await;
    let (reblog_mentions, reblog_emojis) = if let Some((ref rs, _, _)) = reblog {
        (
            fetch_status_mentions(state, rs.id).await?,
            fetch_status_emojis(state, rs).await,
        )
    } else {
        (vec![], vec![])
    };

    let mut api = super::convert::status_from_db_with_app(
        s, account, media, reblog, viewer_ctx, application, &mentions, &reblog_mentions,
    );
    let id: i64 = api.id.parse().unwrap_or(0);
    api.tags = fetch_status_tags(state, id).await?;
    api.mentions = mentions;
    api.emojis = status_emojis;
    api.poll = fetch_status_poll(state, id, viewer_account_id).await?;
    api.card = fetch_status_card(state, id).await;
    if let Some(ref mut rb) = api.reblog {
        let rid: i64 = rb.id.parse().unwrap_or(0);
        rb.tags = fetch_status_tags(state, rid).await?;
        rb.mentions = reblog_mentions;
        rb.emojis = reblog_emojis;
        rb.poll = fetch_status_poll(state, rid, None).await?;
        rb.card = fetch_status_card(state, rid).await;
    }
    Ok(api)
}

/// Extract `:shortcode:` patterns from status text + spoiler and look them up
/// in `custom_emojis` for the status's instance.
async fn fetch_status_emojis(
    state: &AppState,
    s: &crate::db::models::Status,
) -> Vec<super::types::CustomEmoji> {
    let combined = format!("{} {}", s.spoiler_text, s.text);
    let shortcodes: Vec<&str> = {
        let mut v = Vec::new();
        let mut rest = combined.as_str();
        while let Some(start) = rest.find(':') {
            rest = &rest[start + 1..];
            if let Some(end) = rest.find(':') {
                let code = &rest[..end];
                if !code.is_empty() && code.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    v.push(code);
                }
                rest = &rest[end + 1..];
            } else {
                break;
            }
        }
        v
    };

    if shortcodes.is_empty() {
        return vec![];
    }

    let rows = sqlx::query!(
        r#"SELECT shortcode, image_url, static_image_url, visible_in_picker
           FROM custom_emojis
           WHERE instance_id = $1 AND shortcode = ANY($2) AND NOT disabled"#,
        s.instance_id,
        &shortcodes.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    rows.into_iter().map(|r| super::types::CustomEmoji {
        shortcode: r.shortcode,
        url: r.image_url.clone(),
        static_url: r.static_image_url.unwrap_or(r.image_url),
        visible_in_picker: r.visible_in_picker,
        category: None,
    }).collect()
}

/// Extract `:shortcode:` patterns from account profile fields and look them up.
pub async fn fetch_account_emojis(
    state: &AppState,
    a: &Account,
) -> Vec<super::types::CustomEmoji> {
    let mut combined = format!("{} {}", a.display_name, a.note);
    if let Some(fields) = a.fields.as_array() {
        for f in fields {
            if let (Some(n), Some(v)) = (f["name"].as_str(), f["value"].as_str()) {
                combined.push(' ');
                combined.push_str(n);
                combined.push(' ');
                combined.push_str(v);
            }
        }
    }
    let mut shortcodes: Vec<String> = Vec::new();
    let mut rest = combined.as_str();
    while let Some(start) = rest.find(':') {
        rest = &rest[start + 1..];
        if let Some(end) = rest.find(':') {
            let code = &rest[..end];
            if !code.is_empty() && code.chars().all(|c| c.is_alphanumeric() || c == '_') {
                shortcodes.push(code.to_string());
            }
            rest = &rest[end + 1..];
        } else {
            break;
        }
    }
    if shortcodes.is_empty() {
        return vec![];
    }
    let rows = sqlx::query!(
        r#"SELECT shortcode, image_url, static_image_url, visible_in_picker
           FROM custom_emojis
           WHERE instance_id = $1 AND shortcode = ANY($2) AND NOT disabled"#,
        a.instance_id,
        &shortcodes,
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    rows.into_iter().map(|r| super::types::CustomEmoji {
        shortcode: r.shortcode,
        url: r.image_url.clone(),
        static_url: r.static_image_url.unwrap_or(r.image_url),
        visible_in_picker: r.visible_in_picker,
        category: None,
    }).collect()
}

/// Look up an already-cached preview card for a status. Never does network I/O.
pub(super) async fn fetch_status_card(
    state: &AppState,
    status_id: i64,
) -> Option<super::types::PreviewCard> {
    let r = sqlx::query!(
        r#"SELECT pc.url, pc.title, pc.description, pc.card_type,
                  pc.image_url, pc.author_name, pc.author_url,
                  pc.provider_name, pc.provider_url, pc.html, pc.width, pc.height,
                  pc.embed_url, pc.blurhash
           FROM preview_cards pc
           JOIN status_preview_cards spc ON spc.card_id = pc.id
           WHERE spc.status_id = $1
           LIMIT 1"#,
        status_id,
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()?;

    Some(super::types::PreviewCard {
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
    })
}

/// Spawn a background task to fetch a preview card for a newly-created status.
/// Only fetches the first external URL found in the HTML content.
pub fn spawn_card_fetch(state: &AppState, status_id: i64, content: String) {
    let urls = crate::preview_card::extract_urls_from_content(&content);
    let url = match urls.into_iter().next() {
        Some(u) => u,
        None => return,
    };
    let state = state.clone();
    tokio::spawn(async move {
        let Some(card_id) = crate::preview_card::fetch_and_store(&state.db, &state.http, &url).await
        else {
            return;
        };
        let _ = sqlx::query!(
            "INSERT INTO status_preview_cards (status_id, card_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            status_id,
            card_id,
        )
        .execute(&state.db)
        .await;
    });
}

// ── DELETE /api/v1/accounts ────────────────────────────────────────────────

pub async fn delete_account(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    body: Option<Json<serde_json::Value>>,
) -> AppResult<axum::http::StatusCode> {
    let password = body.as_ref()
        .and_then(|b| b.get("password"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let user = sqlx::query!(
        "SELECT password_hash FROM users WHERE account_id = $1",
        auth.account_id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Unauthorized)?;

    crate::crypto::verify_password(password, &user.password_hash)?;

    // Soft-delete: mark account as suspended, revoke tokens, remove user row.
    // Hard delete of statuses/follows is deferred (could be a background job).
    let mut tx = state.db.begin().await?;
    sqlx::query!(
        "UPDATE statuses SET deleted_at = now() WHERE account_id = $1 AND deleted_at IS NULL",
        auth.account_id,
    ).execute(&mut *tx).await?;
    sqlx::query!(
        "UPDATE oauth_access_tokens SET revoked_at = now() WHERE account_id = $1 AND revoked_at IS NULL",
        auth.account_id,
    ).execute(&mut *tx).await?;
    sqlx::query!(
        "UPDATE accounts SET suspended_at = now() WHERE id = $1",
        auth.account_id,
    ).execute(&mut *tx).await?;
    sqlx::query!(
        "DELETE FROM users WHERE account_id = $1",
        auth.account_id,
    ).execute(&mut *tx).await?;
    tx.commit().await?;

    Ok(axum::http::StatusCode::OK)
}
