use axum::{
    extract::{Extension, Multipart, Path, Query, State},
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
        let mut result = Vec::with_capacity(pinned_statuses.len());
        for s in &pinned_statuses {
            let media = fetch_status_media(&state, s.id).await?;
            let reblog = fetch_reblog_data(&state, s).await?;
            let effective_id = s.reblog_of_id.unwrap_or(s.id);
            let ctx = pin_ctxs.get(&effective_id).cloned();
            let mut api_status = status_from_db(s, &account, media, reblog, ctx);
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

    let mut result = Vec::with_capacity(statuses.len());
    for s in &statuses {
        let media = fetch_status_media(&state, s.id).await?;
        let reblog = fetch_reblog_data(&state, s).await?;
        let effective_id = s.reblog_of_id.unwrap_or(s.id);
        let ctx = ctxs.get(&effective_id).cloned();
        result.push(status_from_db(s, &account, media, reblog, ctx));
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

#[derive(Debug, Deserialize)]
pub struct RelationshipsQuery {
    // Mastodon clients send id[]=uuid1&id[]=uuid2; serde_urlencoded can't coerce
    // a single string into Vec, so use a custom visitor that handles both cases.
    #[serde(default, rename = "id[]", deserialize_with = "deserialize_uuid_list")]
    id: Vec<Uuid>,
}

fn deserialize_uuid_list<'de, D>(de: D) -> Result<Vec<Uuid>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{SeqAccess, Visitor};
    struct UuidListVisitor;
    impl<'de> Visitor<'de> for UuidListVisitor {
        type Value = Vec<Uuid>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a UUID string or sequence of UUID strings")
        }
        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
            v.parse::<Uuid>().map(|u| vec![u]).map_err(serde::de::Error::custom)
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut out = Vec::new();
            while let Some(v) = seq.next_element::<Uuid>()? { out.push(v); }
            Ok(out)
        }
    }
    de.deserialize_any(UuidListVisitor)
}

pub async fn get_relationships(
    State(state): State<AppState>,
    Query(q): Query<RelationshipsQuery>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<Relationship>>> {
    let mut results = Vec::with_capacity(q.id.len());
    for target_id in &q.id {
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
    Extension(_auth): Extension<AuthenticatedUser>,
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
    Query(q): Query<super::types::FamiliarFollowersQuery>,
) -> AppResult<Json<Vec<super::types::FamiliarFollowers>>> {
    let mut result = Vec::with_capacity(q.ids.len());
    for raw_id in &q.ids {
        let Ok(target_id) = raw_id.parse::<Uuid>() else { continue };

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
            id: raw_id.clone(),
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
