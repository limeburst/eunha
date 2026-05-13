use axum::{
    extract::{Extension, Multipart, Path, Query, RawQuery, State},
    http::{header, HeaderMap, Uri},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    db::models::Account,
    error::{AppError, AppResult},
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
    let account = fetch_account(&state, auth.account_id).await?;
    let mut api_account = account_from_db(&account);

    // Attach `source` field for the credential account
    let follow_requests: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM follows WHERE target_account_id = $1 AND state = 'pending'",
        account.id
    )
    .fetch_one(&state.db)
    .await?
    .unwrap_or(0);

    api_account.source = Some(super::types::AccountSource {
        privacy: "public".into(),
        sensitive: false,
        language: None,
        note: account.note_text.clone(),
        fields: vec![],
        follow_requests_count: follow_requests,
        discoverable: Some(account.discoverable),
        indexable: account.indexable,
        hide_collections: None,
        attribution_domains: vec![],
        quote_policy: "public".into(),
    });

    Ok(Json(api_account))
}

// ── GET /api/v1/accounts/lookup ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct LookupQuery {
    pub acct: String,
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

    let account = match domain {
        None => sqlx::query_as!(
            Account,
            "SELECT * FROM accounts WHERE lower(username) = $1 AND instance_id = $2 AND domain IS NULL",
            username,
            instance.id,
        )
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?,

        Some(ref d) => sqlx::query_as!(
            Account,
            "SELECT * FROM accounts WHERE lower(username) = $1 AND lower(domain) = $2",
            username,
            d,
        )
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?,
    };

    Ok(Json(account_from_db(&account)))
}

// ── GET /api/v1/accounts/:id ───────────────────────────────────────────────

pub async fn get_account(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApiAccount>> {
    let account = fetch_account(&state, id).await?;
    Ok(Json(account_from_db(&account)))
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
    Path(id): Path<Uuid>,
    uri: Uri,
    req_headers: HeaderMap,
    Query(q): Query<StatusesQuery>,
    auth: Option<Extension<AuthenticatedUser>>,
) -> AppResult<impl IntoResponse> {
    let account = fetch_account(&state, id).await?;
    let viewer_id = auth.as_ref().map(|Extension(a)| a.account_id);

    if q.pinned == Some(true) {
        let pinned_statuses = sqlx::query_as!(
            crate::db::models::Status,
            r#"SELECT s.* FROM statuses s
               JOIN status_pins sp ON sp.status_id = s.id
               WHERE sp.account_id = $1 AND s.deleted_at IS NULL
               ORDER BY sp.id DESC"#,
            account.id,
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
        let mut result = Vec::with_capacity(pinned_statuses.len());
        for s in &pinned_statuses {
            let media = pin_media_map.get(&s.id).cloned().unwrap_or_default();
            let reblog = pin_reblog_map.get(&s.id).cloned();
            let effective_id = s.reblog_of_id.unwrap_or(s.id);
            let ctx = pin_ctxs.get(&effective_id).cloned();
            let mut api_status = status_from_db(s, &account, media, reblog, ctx);
            api_status.tags = pin_tags_map.get(&s.id).cloned().unwrap_or_default();
            api_status.mentions = pin_mentions_map.get(&s.id).cloned().unwrap_or_default();
            if let Some(ref mut rb) = api_status.reblog {
                let rid: i64 = rb.id.parse().unwrap_or(0);
                rb.tags = pin_tags_map.get(&rid).cloned().unwrap_or_default();
                rb.mentions = pin_mentions_map.get(&rid).cloned().unwrap_or_default();
            }
            api_status.pinned = Some(true);
            result.push(api_status);
        }
        return Ok((HeaderMap::new(), Json(result)));
    }

    let limit = q.pagination.limit_clamped(20, 40);
    let max_id = q.pagination.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = q.pagination.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());

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

    let statuses = sqlx::query_as!(
        crate::db::models::Status,
        r#"SELECT * FROM statuses
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
               text != '' OR content != ''
               OR reblog_of_id IS NOT NULL
               OR EXISTS (SELECT 1 FROM media_attachments WHERE status_id = statuses.id)
             )
             AND ($9::boolean IS NOT TRUE OR
               EXISTS (SELECT 1 FROM media_attachments WHERE status_id = statuses.id) OR
               (reblog_of_id IS NOT NULL AND EXISTS (SELECT 1 FROM media_attachments WHERE status_id = reblog_of_id))
             )
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
    )
    .fetch_all(&state.db)
    .await?;

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

    let mut result = Vec::with_capacity(statuses.len());
    for s in &statuses {
        let media = media_map.get(&s.id).cloned().unwrap_or_default();
        let reblog = reblog_map.get(&s.id).cloned();
        let effective_id = s.reblog_of_id.unwrap_or(s.id);
        let ctx = ctxs.get(&effective_id).cloned();
        let mut api = status_from_db(s, &account, media, reblog, ctx);
        api.tags = tags_map.get(&s.id).cloned().unwrap_or_default();
        api.mentions = mentions_map.get(&s.id).cloned().unwrap_or_default();
        if let Some(ref mut rb) = api.reblog {
            let rid: i64 = rb.id.parse().unwrap_or(0);
            rb.tags = tags_map.get(&rid).cloned().unwrap_or_default();
            rb.mentions = mentions_map.get(&rid).cloned().unwrap_or_default();
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
    // serde_urlencoded treats id[]=v1&id[]=v2 as a duplicate field → 400.
    // Parse with form_urlencoded which correctly returns each pair separately.
    let ids: Vec<Uuid> = url::form_urlencoded::parse(
            qs.as_deref().unwrap_or("").as_bytes()
        )
        .filter(|(k, _)| k == "id[]" || k == "id")
        .filter_map(|(_, v)| v.parse::<Uuid>().ok())
        .collect();

    let mut results = Vec::with_capacity(ids.len());
    for target_id in &ids {
        results.push(build_relationship(&state, auth.account_id, *target_id).await?);
    }
    Ok(Json(results))
}

// ── POST /api/v1/accounts/:id/follow ──────────────────────────────────────

pub async fn follow_account(
    State(state): State<AppState>,
    Path(target_id): Path<Uuid>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
    if auth.account_id == target_id {
        return Err(AppError::Unprocessable("Cannot follow yourself".into()));
    }
    let target = fetch_account(&state, target_id).await?;
    let state_val = if target.locked { "pending" } else { "accepted" };

    sqlx::query!(
        r#"INSERT INTO follows (account_id, target_account_id, state)
           VALUES ($1, $2, $3)
           ON CONFLICT (account_id, target_account_id) DO NOTHING"#,
        auth.account_id,
        target_id,
        state_val,
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
    Path(target_id): Path<Uuid>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
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
    Path(id): Path<Uuid>,
    Query(q): Query<FollowersQuery>,
) -> AppResult<Json<Vec<ApiAccount>>> {
    let limit = q.pagination.limit_clamped(40, 80);
    let max_id_str = q.pagination.max_id.as_deref();

    let accounts = if let Some(cursor) = max_id_str.and_then(|s| s.parse::<Uuid>().ok()) {
        sqlx::query_as!(
            Account,
            r#"SELECT a.* FROM accounts a
               JOIN follows f ON f.account_id = a.id
               WHERE f.target_account_id = $1 AND f.state = 'accepted'
                 AND f.id < $2
               ORDER BY f.id DESC LIMIT $3"#,
            id, cursor, limit
        )
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as!(
            Account,
            r#"SELECT a.* FROM accounts a
               JOIN follows f ON f.account_id = a.id
               WHERE f.target_account_id = $1 AND f.state = 'accepted'
               ORDER BY f.id DESC LIMIT $2"#,
            id, limit
        )
        .fetch_all(&state.db)
        .await?
    };

    Ok(Json(accounts.iter().map(account_from_db).collect()))
}

// ── GET /api/v1/accounts/:id/following ────────────────────────────────────

pub async fn get_account_following(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<FollowersQuery>,
) -> AppResult<Json<Vec<ApiAccount>>> {
    let limit = q.pagination.limit_clamped(40, 80);
    let max_id_str = q.pagination.max_id.as_deref();

    let accounts = if let Some(cursor) = max_id_str.and_then(|s| s.parse::<Uuid>().ok()) {
        sqlx::query_as!(
            Account,
            r#"SELECT a.* FROM accounts a
               JOIN follows f ON f.target_account_id = a.id
               WHERE f.account_id = $1 AND f.state = 'accepted'
                 AND f.id < $2
               ORDER BY f.id DESC LIMIT $3"#,
            id, cursor, limit
        )
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as!(
            Account,
            r#"SELECT a.* FROM accounts a
               JOIN follows f ON f.target_account_id = a.id
               WHERE f.account_id = $1 AND f.state = 'accepted'
               ORDER BY f.id DESC LIMIT $2"#,
            id, limit
        )
        .fetch_all(&state.db)
        .await?
    };

    Ok(Json(accounts.iter().map(account_from_db).collect()))
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

    let accounts = if q.following.unwrap_or(false) {
        if let Some(Extension(auth)) = auth {
            sqlx::query_as!(
                Account,
                r#"SELECT a.* FROM accounts a
                   JOIN follows f ON f.target_account_id = a.id
                   WHERE f.account_id = $1 AND f.state = 'accepted'
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
                 AND (lower(username) LIKE $2 OR lower(display_name) LIKE $2)
               ORDER BY username LIMIT $3"#,
            instance.id, pattern, limit
        )
        .fetch_all(&state.db)
        .await?
    };

    Ok(Json(accounts.iter().map(account_from_db).collect()))
}

// ── PATCH /api/v1/accounts/update_credentials ─────────────────────────────

pub async fn update_credentials(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Extension(crate::middleware::ResolvedInstance(instance)): Extension<crate::middleware::ResolvedInstance>,
    mut multipart: Multipart,
) -> AppResult<Json<ApiAccount>> {
    let mut display_name: Option<String> = None;
    let mut note: Option<String> = None;
    let mut locked: Option<bool> = None;
    let mut bot: Option<bool> = None;
    let mut discoverable: Option<bool> = None;
    let mut avatar_url: Option<String> = None;
    let mut header_url: Option<String> = None;
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
        let note_html = format!("<p>{}</p>", ammonia::clean_text(n));
        sqlx::query!("UPDATE accounts SET note = $1, note_text = $2 WHERE id = $3", note_html, n, auth.account_id)
            .execute(&state.db).await?;
    }
    if let Some(l) = locked {
        sqlx::query!("UPDATE accounts SET locked = $1 WHERE id = $2", l, auth.account_id)
            .execute(&state.db).await?;
    }
    if let Some(b) = bot {
        sqlx::query!("UPDATE accounts SET bot = $1 WHERE id = $2", b, auth.account_id)
            .execute(&state.db).await?;
    }
    if let Some(d) = discoverable {
        sqlx::query!("UPDATE accounts SET discoverable = $1 WHERE id = $2", d, auth.account_id)
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

    let account = fetch_account(&state, auth.account_id).await?;
    let fields = super::convert::fields_from_db(&account.fields);
    let mut api_account = account_from_db(&account);
    api_account.source = Some(super::types::AccountSource {
        privacy: "public".into(),
        sensitive: false,
        language: None,
        note: account.note_text.clone(),
        fields: fields.clone(),
        follow_requests_count: 0,
        discoverable: Some(account.discoverable),
        indexable: account.indexable,
        hide_collections: None,
        attribution_domains: vec![],
        quote_policy: "public".into(),
    });
    Ok(Json(api_account))
}

// ── POST /api/v1/accounts/:id/mute ────────────────────────────────────────

pub async fn mute_account(
    State(state): State<AppState>,
    Path(target_id): Path<Uuid>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
    sqlx::query!(
        r#"INSERT INTO mutes (account_id, target_account_id) VALUES ($1, $2)
           ON CONFLICT (account_id, target_account_id) DO NOTHING"#,
        auth.account_id, target_id
    )
    .execute(&state.db)
    .await?;

    build_relationship(&state, auth.account_id, target_id).await.map(Json)
}

// ── POST /api/v1/accounts/:id/unmute ──────────────────────────────────────

pub async fn unmute_account(
    State(state): State<AppState>,
    Path(target_id): Path<Uuid>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
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
    Path(target_id): Path<Uuid>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
    sqlx::query!(
        r#"INSERT INTO blocks (account_id, target_account_id) VALUES ($1, $2)
           ON CONFLICT (account_id, target_account_id) DO NOTHING"#,
        auth.account_id, target_id
    )
    .execute(&state.db)
    .await?;

    // Remove any follow relationship in both directions
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
    Path(target_id): Path<Uuid>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
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
    Query(q): Query<PaginationParams>,
) -> AppResult<Json<Vec<ApiAccount>>> {
    let limit = q.limit_clamped(40, 80);
    let accounts = sqlx::query_as!(
        Account,
        r#"SELECT a.* FROM accounts a
           JOIN blocks b ON b.target_account_id = a.id
           WHERE b.account_id = $1
           ORDER BY b.created_at DESC LIMIT $2"#,
        auth.account_id, limit,
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(accounts.iter().map(account_from_db).collect()))
}

// ── GET /api/v1/mutes ─────────────────────────────────────────────────────

pub async fn get_mutes(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Query(q): Query<PaginationParams>,
) -> AppResult<Json<Vec<ApiAccount>>> {
    let limit = q.limit_clamped(40, 80);
    let accounts = sqlx::query_as!(
        Account,
        r#"SELECT a.* FROM accounts a
           JOIN mutes m ON m.target_account_id = a.id
           WHERE m.account_id = $1
           ORDER BY m.created_at DESC LIMIT $2"#,
        auth.account_id, limit,
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(accounts.iter().map(account_from_db).collect()))
}

// ── GET /api/v1/preferences ───────────────────────────────────────────────

pub async fn get_preferences(
    Extension(_auth): Extension<AuthenticatedUser>,
) -> Json<Preferences> {
    Json(Preferences {
        posting_default_visibility: "public".into(),
        posting_default_sensitive: false,
        posting_default_language: None,
        reading_expand_media: "default".into(),
        reading_expand_spoilers: false,
    })
}

// ── GET /api/v1/follow_requests ───────────────────────────────────────────

pub async fn get_follow_requests(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Query(q): Query<PaginationParams>,
) -> AppResult<Json<Vec<ApiAccount>>> {
    let limit = q.limit_clamped(40, 80);
    let accounts = sqlx::query_as!(
        Account,
        r#"SELECT a.* FROM accounts a
           JOIN follows f ON f.account_id = a.id
           WHERE f.target_account_id = $1 AND f.state = 'pending'
           ORDER BY f.created_at DESC LIMIT $2"#,
        auth.account_id, limit
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(accounts.iter().map(account_from_db).collect()))
}

// ── POST /api/v1/follow_requests/:id/authorize ────────────────────────────

pub async fn authorize_follow_request(
    State(state): State<AppState>,
    Path(requester_id): Path<Uuid>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
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

    build_relationship(&state, auth.account_id, requester_id).await.map(Json)
}

// ── POST /api/v1/follow_requests/:id/reject ───────────────────────────────

pub async fn reject_follow_request(
    State(state): State<AppState>,
    Path(requester_id): Path<Uuid>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
    sqlx::query!(
        "DELETE FROM follows WHERE account_id = $1 AND target_account_id = $2 AND state = 'pending'",
        requester_id, auth.account_id
    )
    .execute(&state.db)
    .await?;

    build_relationship(&state, auth.account_id, requester_id).await.map(Json)
}

// ── Helpers ────────────────────────────────────────────────────────────────

async fn fetch_account(state: &AppState, id: Uuid) -> AppResult<Account> {
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

    let reblog_account_ids: Vec<Uuid> = reblog_statuses.iter()
        .map(|s| s.account_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let reblog_accounts = sqlx::query_as!(
        Account,
        "SELECT * FROM accounts WHERE id = ANY($1::uuid[])",
        &reblog_account_ids,
    )
    .fetch_all(&state.db)
    .await?;

    let reblog_account_map: HashMap<Uuid, Account> = reblog_accounts
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

async fn build_relationship(state: &AppState, source_id: Uuid, target_id: Uuid) -> AppResult<Relationship> {
    let follow = sqlx::query!(
        "SELECT state FROM follows WHERE account_id = $1 AND target_account_id = $2",
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

    let muting = sqlx::query!(
        "SELECT hide_notifications FROM mutes WHERE account_id = $1 AND target_account_id = $2",
        source_id, target_id
    )
    .fetch_optional(&state.db)
    .await?;

    let note = sqlx::query_scalar!(
        "SELECT comment FROM account_notes WHERE account_id = $1 AND target_account_id = $2",
        source_id, target_id
    )
    .fetch_optional(&state.db)
    .await?
    .unwrap_or_default();

    Ok(Relationship {
        id: target_id.to_string(),
        following: follow.as_ref().map_or(false, |f| f.state == "accepted"),
        showing_reblogs: true,
        notifying: false,
        languages: vec![],
        followed_by,
        blocking,
        blocked_by: false,
        muting: muting.is_some(),
        muting_notifications: muting.map_or(false, |m| m.hide_notifications),
        muting_expires_at: None,
        requested: follow.as_ref().map_or(false, |f| f.state == "pending"),
        requested_by: false,
        domain_blocking: false,
        endorsed: false,
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
    Extension(_auth): Extension<AuthenticatedUser>,
    Path(_account_id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
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

// ── POST /api/v1/accounts/:id/note ────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct NoteForm {
    pub comment: Option<String>,
}

pub async fn set_account_note(
    State(state): State<AppState>,
    Path(target_id): Path<Uuid>,
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
    Path(requester_id): Path<Uuid>,
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
    Path(target_id): Path<Uuid>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
    // Endorsements not persisted yet; just return relationship
    build_relationship(&state, auth.account_id, target_id).await.map(Json)
}

// ── POST /api/v1/accounts/:id/unendorse ──────────────────────────────────

pub async fn unendorse_account(
    State(state): State<AppState>,
    Path(target_id): Path<Uuid>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Relationship>> {
    build_relationship(&state, auth.account_id, target_id).await.map(Json)
}

// ── GET /api/v1/accounts/:id/endorsements ────────────────────────────────

pub async fn get_endorsements(
    Path(_id): Path<Uuid>,
) -> Json<Vec<ApiAccount>> {
    Json(vec![])
}

// ── GET /api/v1/accounts/:id/featured_tags ───────────────────────────────

pub async fn get_account_featured_tags(
    Path(_id): Path<Uuid>,
) -> Json<Vec<super::types::FeaturedTag>> {
    Json(vec![])
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

// ── GET /api/v1/accounts/familiar_followers ──────────────────────────────

pub async fn get_familiar_followers(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    RawQuery(qs): RawQuery,
) -> AppResult<Json<Vec<super::types::FamiliarFollowers>>> {
    let ids: Vec<Uuid> = url::form_urlencoded::parse(
            qs.as_deref().unwrap_or("").as_bytes()
        )
        .filter(|(k, _)| k == "id[]" || k == "id")
        .filter_map(|(_, v)| v.parse::<Uuid>().ok())
        .collect();

    let mut result = Vec::with_capacity(ids.len());
    for target_id in &ids {
        // Find followers of target_id that also follow the viewer (auth.account_id)
        let accounts = sqlx::query_as!(
            crate::db::models::Account,
            r#"SELECT a.* FROM accounts a
               JOIN follows f1 ON f1.account_id = a.id AND f1.target_account_id = $1 AND f1.state = 'accepted'
               JOIN follows f2 ON f2.account_id = a.id AND f2.target_account_id = $2 AND f2.state = 'accepted'
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

// ── GET /api/v1/accounts (batch lookup) ──────────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct BatchAccountsQuery {
    #[serde(default, rename = "id[]")]
    pub ids: Vec<Uuid>,
}

pub async fn get_accounts_batch(
    State(state): State<AppState>,
    Query(q): Query<BatchAccountsQuery>,
) -> AppResult<Json<Vec<ApiAccount>>> {
    if q.ids.is_empty() {
        return Ok(Json(vec![]));
    }
    let accounts = sqlx::query_as!(
        crate::db::models::Account,
        "SELECT * FROM accounts WHERE id = ANY($1::uuid[]) ORDER BY created_at DESC",
        &q.ids,
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(accounts.iter().map(account_from_db).collect()))
}

// ── GET /api/v1/accounts/:id/lists ───────────────────────────────────────

pub async fn get_account_lists(
    State(state): State<AppState>,
    Path(target_id): Path<Uuid>,
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

/// Builds a `Status` API object with tags and mentions populated from the DB.
pub async fn build_status(
    state: &AppState,
    s: &crate::db::models::Status,
    account: &Account,
    media: Vec<crate::db::models::MediaAttachment>,
    reblog: Option<(crate::db::models::Status, Account, Vec<crate::db::models::MediaAttachment>)>,
    viewer_ctx: Option<super::convert::StatusViewerContext>,
) -> AppResult<super::types::Status> {
    let mut api = super::convert::status_from_db(s, account, media, reblog, viewer_ctx);
    let id: i64 = api.id.parse().unwrap_or(0);
    api.tags = fetch_status_tags(state, id).await?;
    api.mentions = fetch_status_mentions(state, id).await?;
    if let Some(ref mut rb) = api.reblog {
        let rid: i64 = rb.id.parse().unwrap_or(0);
        rb.tags = fetch_status_tags(state, rid).await?;
        rb.mentions = fetch_status_mentions(state, rid).await?;
    }
    Ok(api)
}
