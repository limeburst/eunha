use axum::{
    extract::{Extension, Path, Query, RawQuery, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;

use crate::{
    db::models::{Account, Notification as DbNotification},
    error::{AppError, AppResult},
    middleware::AuthenticatedUser,
    state::AppState,
};
use super::{
    accounts::{build_status, fetch_reblog_data, fetch_status_media},
    convert::account_from_db,
    types::{
        Notification, NotificationGroup, NotificationGroupsResponse, NotificationPagination,
        NotificationPolicy, NotificationPolicySummary, NotificationRequest, PaginationParams,
    },
};

// ── GET /api/v1/notifications ─────────────────────────────────────────────

pub async fn get_notifications(
    State(state): State<AppState>,
    Query(pagination): Query<PaginationParams>,
    RawQuery(qs): RawQuery,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<Notification>>> {
    auth.require_scope("read:notifications")?;
    let limit = pagination.limit_clamped(40, 80);
    let max_id = pagination.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = pagination.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let (types, exclude_types, account_id) = parse_notif_filters(qs.as_deref());

    let notifications = sqlx::query_as(
        r#"SELECT * FROM notifications
           WHERE account_id = $1
             AND ($2::bigint IS NULL OR id < $2)
             AND ($3::bigint IS NULL OR id > $3)
             AND ($5::text[] IS NULL OR notification_type = ANY($5))
             AND ($6::text[] IS NULL OR NOT (notification_type = ANY($6)))
             AND ($7::uuid IS NULL OR from_account_id = $7)
             AND NOT EXISTS (
                 SELECT 1 FROM mutes m
                 WHERE m.account_id = $1 AND m.target_account_id = from_account_id
                   AND m.hide_notifications = true
                   AND (m.expires_at IS NULL OR m.expires_at > now())
             )
           ORDER BY id DESC
           LIMIT $4"#,
    )
    .bind(auth.account_id)
    .bind(max_id)
    .bind(since_id)
    .bind(limit)
    .bind(types)
    .bind(exclude_types)
    .bind(account_id)
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::with_capacity(notifications.len());
    for n in &notifications {
        result.push(build_notification(&state, n).await?);
    }
    Ok(Json(result))
}

// ── GET /api/v1/notifications/:id ─────────────────────────────────────────

pub async fn get_notification(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Notification>> {
    auth.require_scope("read:notifications")?;
    let n = sqlx::query_as!(
        DbNotification,
        "SELECT * FROM notifications WHERE id = $1 AND account_id = $2",
        id,
        auth.account_id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    build_notification(&state, &n).await.map(Json)
}

// ── POST /api/v1/notifications/clear ──────────────────────────────────────

pub async fn clear_notifications(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<serde_json::Value>> {
    auth.require_scope("write:notifications")?;
    sqlx::query!(
        "DELETE FROM notifications WHERE account_id = $1",
        auth.account_id
    )
    .execute(&state.db)
    .await?;
    Ok(Json(serde_json::json!({})))
}

// ── POST /api/v1/notifications/:id/dismiss ────────────────────────────────

pub async fn dismiss_notification(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<serde_json::Value>> {
    auth.require_scope("write:notifications")?;
    let deleted = sqlx::query!(
        "DELETE FROM notifications WHERE id = $1 AND account_id = $2",
        id, auth.account_id,
    )
    .execute(&state.db)
    .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(Json(serde_json::json!({})))
}

// ── GET /api/v2/notifications ─────────────────────────────────────────────

pub async fn get_notifications_v2(
    State(state): State<AppState>,
    Query(pagination): Query<PaginationParams>,
    RawQuery(qs): RawQuery,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<NotificationGroupsResponse>> {
    auth.require_scope("read:notifications")?;
    let limit = pagination.limit_clamped(40, 80);
    let max_id = pagination.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = pagination.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let (types, exclude_types, account_id) = parse_notif_filters(qs.as_deref());

    let notifications: Vec<DbNotification> = sqlx::query_as(
        r#"SELECT * FROM notifications
           WHERE account_id = $1
             AND ($2::bigint IS NULL OR id < $2)
             AND ($3::bigint IS NULL OR id > $3)
             AND ($5::text[] IS NULL OR notification_type = ANY($5))
             AND ($6::text[] IS NULL OR NOT (notification_type = ANY($6)))
             AND ($7::uuid IS NULL OR from_account_id = $7)
             AND NOT EXISTS (
                 SELECT 1 FROM mutes m
                 WHERE m.account_id = $1 AND m.target_account_id = from_account_id
                   AND m.hide_notifications = true
                   AND (m.expires_at IS NULL OR m.expires_at > now())
             )
           ORDER BY id DESC
           LIMIT $4"#,
    )
    .bind(auth.account_id)
    .bind(max_id)
    .bind(since_id)
    .bind(limit)
    .bind(types)
    .bind(exclude_types)
    .bind(account_id)
    .fetch_all(&state.db)
    .await?;

    let mut groups = Vec::with_capacity(notifications.len());
    let mut accounts_map: std::collections::HashMap<String, super::types::Account> =
        std::collections::HashMap::new();
    let mut statuses_map: std::collections::HashMap<String, super::types::Status> =
        std::collections::HashMap::new();

    for n in &notifications {
        let from_account = sqlx::query_as!(
            Account,
            "SELECT * FROM accounts WHERE id = $1",
            n.from_account_id
        )
        .fetch_one(&state.db)
        .await?;
        let api_account = account_from_db(&from_account);
        accounts_map.insert(from_account.id.to_string(), api_account);

        let status_id = if let Some(sid) = n.status_id {
            if let Some(s) = sqlx::query_as!(
                crate::db::models::Status,
                "SELECT * FROM statuses WHERE id = $1 AND deleted_at IS NULL",
                sid
            )
            .fetch_optional(&state.db)
            .await?
            {
                let saccount = sqlx::query_as!(
                    Account,
                    "SELECT * FROM accounts WHERE id = $1",
                    s.account_id
                )
                .fetch_one(&state.db)
                .await?;
                let media = fetch_status_media(&state, s.id).await?;
                let reblog = fetch_reblog_data(&state, &s).await?;
                let api_status = build_status(&state, &s, &saccount, media, reblog, None).await?;
                let sid_str = s.id.to_string();
                statuses_map.insert(sid_str.clone(), api_status);
                Some(sid_str)
            } else {
                None
            }
        } else {
            None
        };

        let id_str = n.id.to_string();
        groups.push(NotificationGroup {
            group_key: format!("ungrouped-{}", id_str),
            notifications_count: 1,
            notification_type: n.notification_type.clone(),
            most_recent_notification_id: id_str.clone(),
            page_max_id: id_str.clone(),
            page_min_id: id_str.clone(),
            latest_page_notification_at: n.created_at.to_rfc3339(),
            sample_account_ids: vec![n.from_account_id.to_string()],
            status_id,
        });
    }

    Ok(Json(NotificationGroupsResponse {
        notification_groups: groups,
        accounts: accounts_map.into_values().collect(),
        statuses: statuses_map.into_values().collect(),
    }))
}

// ── GET /api/v1/notifications/unread_count ───────────────────────────────

pub async fn get_notifications_unread_count(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<serde_json::Value>> {
    auth.require_scope("read:notifications")?;

    // Find last read ID from markers (empty string means never read)
    let last_read_id: Option<String> = sqlx::query_scalar!(
        "SELECT NULLIF(last_read_id, '') FROM markers WHERE account_id = $1 AND timeline = 'notifications'",
        auth.account_id,
    )
    .fetch_optional(&state.db)
    .await?
    .flatten();

    let count: i64 = if let Some(last_id) = last_read_id {
        let last_id_int: i64 = last_id.parse().unwrap_or(0);
        sqlx::query_scalar!(
            "SELECT COUNT(*) FROM notifications WHERE account_id = $1 AND id > $2",
            auth.account_id, last_id_int,
        )
        .fetch_one(&state.db)
        .await?
        .unwrap_or(0)
        .min(100)
    } else {
        sqlx::query_scalar!(
            "SELECT COUNT(*) FROM notifications WHERE account_id = $1",
            auth.account_id,
        )
        .fetch_one(&state.db)
        .await?
        .unwrap_or(0)
        .min(100)
    };

    Ok(Json(serde_json::json!({ "count": count })))
}

// ── GET /api/v2/notifications/policy ─────────────────────────────────────

pub async fn get_notification_policy(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<NotificationPolicy>> {
    auth.require_scope("read:notifications")?;
    let user = sqlx::query!(
        r#"SELECT notif_filter_not_following, notif_filter_not_followers,
                  notif_filter_new_accounts, notif_filter_private_mentions
           FROM users WHERE account_id = $1"#,
        auth.account_id,
    )
    .fetch_one(&state.db)
    .await?;

    let (pending_requests, pending_notifs) = if user.notif_filter_not_following
        || user.notif_filter_not_followers
        || user.notif_filter_new_accounts
        || user.notif_filter_private_mentions
    {
        let pending_requests: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM notification_requests WHERE account_id = $1 AND NOT dismissed",
            auth.account_id,
        )
        .fetch_one(&state.db)
        .await?
        .unwrap_or(0);
        let pending_notifs: i64 = sqlx::query_scalar!(
            "SELECT COALESCE(SUM(notifications_count), 0)::bigint FROM notification_requests WHERE account_id = $1 AND NOT dismissed",
            auth.account_id,
        )
        .fetch_one(&state.db)
        .await?
        .unwrap_or(0);
        (pending_requests, pending_notifs)
    } else {
        (0_i64, 0_i64)
    };

    Ok(Json(NotificationPolicy {
        filter_not_following: user.notif_filter_not_following,
        filter_not_followers: user.notif_filter_not_followers,
        filter_new_accounts: user.notif_filter_new_accounts,
        filter_private_mentions: user.notif_filter_private_mentions,
        summary: NotificationPolicySummary {
            pending_requests_count: pending_requests,
            pending_notifications_count: pending_notifs,
        },
    }))
}

// ── PATCH /api/v2/notifications/policy ───────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct UpdateNotificationPolicyForm {
    pub filter_not_following: Option<bool>,
    pub filter_not_followers: Option<bool>,
    pub filter_new_accounts: Option<bool>,
    pub filter_private_mentions: Option<bool>,
}

pub async fn update_notification_policy(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<UpdateNotificationPolicyForm>,
) -> AppResult<Json<NotificationPolicy>> {
    auth.require_scope("write:notifications")?;
    sqlx::query!(
        r#"UPDATE users SET
               notif_filter_not_following    = COALESCE($2, notif_filter_not_following),
               notif_filter_not_followers    = COALESCE($3, notif_filter_not_followers),
               notif_filter_new_accounts     = COALESCE($4, notif_filter_new_accounts),
               notif_filter_private_mentions = COALESCE($5, notif_filter_private_mentions),
               updated_at = now()
           WHERE account_id = $1"#,
        auth.account_id,
        form.filter_not_following,
        form.filter_not_followers,
        form.filter_new_accounts,
        form.filter_private_mentions,
    )
    .execute(&state.db)
    .await?;

    get_notification_policy(State(state), Extension(auth)).await
}

// ── GET /api/v1/notifications/requests ───────────────────────────────────

pub async fn get_notification_requests(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Query(pagination): Query<NotificationPagination>,
) -> AppResult<Json<Vec<NotificationRequest>>> {
    auth.require_scope("read:notifications")?;
    let limit = pagination.limit.unwrap_or(40).min(80).max(1) as i64;
    let rows = sqlx::query!(
        r#"SELECT nr.id, nr.from_account_id, nr.last_status_id, nr.notifications_count, nr.created_at,
                  a.username, a.domain, a.display_name, a.avatar, a.avatar_static,
                  a.note, a.note_text, a.url, a.uri, a.header, a.header_static,
                  a.public_key, a.private_key, a.followers_count, a.following_count,
                  a.statuses_count, a.locked, a.bot, a.discoverable, a.indexable,
                  a.moved_to_uri, a.inbox_url, a.outbox_url, a.shared_inbox_url,
                  a.suspended_at, a.silenced_at, a.hide_collections, a.last_status_at, a.fields,
                  a.instance_id, a.created_at AS account_created_at, a.updated_at AS account_updated_at
           FROM notification_requests nr
           JOIN accounts a ON a.id = nr.from_account_id
           WHERE nr.account_id = $1 AND NOT nr.dismissed
           ORDER BY nr.updated_at DESC
           LIMIT $2"#,
        auth.account_id, limit,
    )
    .fetch_all(&state.db)
    .await?;

    let result = rows.into_iter().map(|r| {
        let acc = crate::db::models::Account {
            id: r.from_account_id,
            instance_id: r.instance_id,
            username: r.username,
            domain: r.domain,
            display_name: r.display_name,
            note: r.note,
            note_text: r.note_text,
            url: r.url,
            uri: r.uri,
            avatar: r.avatar,
            avatar_static: r.avatar_static,
            header: r.header,
            header_static: r.header_static,
            private_key: r.private_key,
            public_key: r.public_key,
            followers_count: r.followers_count,
            following_count: r.following_count,
            statuses_count: r.statuses_count,
            locked: r.locked,
            bot: r.bot,
            discoverable: r.discoverable,
            indexable: r.indexable,
            moved_to_uri: r.moved_to_uri,
            inbox_url: r.inbox_url,
            outbox_url: r.outbox_url,
            shared_inbox_url: r.shared_inbox_url,
            suspended_at: r.suspended_at,
            silenced_at: r.silenced_at,
            hide_collections: r.hide_collections,
            last_status_at: r.last_status_at,
            fields: r.fields,
            created_at: r.account_created_at,
            updated_at: r.account_updated_at,
        };
        NotificationRequest {
            id: r.id.to_string(),
            created_at: r.created_at.to_rfc3339(),
            updated_at: r.created_at.to_rfc3339(),
            notifications_count: r.notifications_count.to_string(),
            last_status: None,
            account: super::convert::account_from_db(&acc),
        }
    }).collect();

    Ok(Json(result))
}

// ── POST /api/v1/notifications/requests/:id/accept ───────────────────────

pub async fn accept_notification_request(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    auth.require_scope("write:notifications")?;
    sqlx::query!(
        "UPDATE notification_requests SET dismissed = false WHERE id = $1 AND account_id = $2",
        id, auth.account_id,
    )
    .execute(&state.db)
    .await?;
    Ok(Json(serde_json::json!({})))
}

// ── POST /api/v1/notifications/requests/:id/dismiss ──────────────────────

pub async fn dismiss_notification_request(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    auth.require_scope("write:notifications")?;
    sqlx::query!(
        "UPDATE notification_requests SET dismissed = true WHERE id = $1 AND account_id = $2",
        id, auth.account_id,
    )
    .execute(&state.db)
    .await?;
    Ok(Json(serde_json::json!({})))
}

// ── GET /api/v1/notifications/requests/:id ───────────────────────────────

pub async fn get_notification_request(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<NotificationRequest>> {
    auth.require_scope("read:notifications")?;
    let r = sqlx::query!(
        r#"SELECT nr.id, nr.from_account_id, nr.last_status_id, nr.notifications_count, nr.created_at,
                  a.username, a.domain, a.display_name, a.avatar, a.avatar_static,
                  a.note, a.note_text, a.url, a.uri, a.header, a.header_static,
                  a.public_key, a.private_key, a.followers_count, a.following_count,
                  a.statuses_count, a.locked, a.bot, a.discoverable, a.indexable,
                  a.moved_to_uri, a.inbox_url, a.outbox_url, a.shared_inbox_url,
                  a.suspended_at, a.silenced_at, a.hide_collections, a.last_status_at, a.fields,
                  a.instance_id, a.created_at AS account_created_at, a.updated_at AS account_updated_at
           FROM notification_requests nr
           JOIN accounts a ON a.id = nr.from_account_id
           WHERE nr.id = $1 AND nr.account_id = $2"#,
        id, auth.account_id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let acc = crate::db::models::Account {
        id: r.from_account_id,
        instance_id: r.instance_id,
        username: r.username,
        domain: r.domain,
        display_name: r.display_name,
        note: r.note,
        note_text: r.note_text,
        url: r.url,
        uri: r.uri,
        avatar: r.avatar,
        avatar_static: r.avatar_static,
        header: r.header,
        header_static: r.header_static,
        private_key: r.private_key,
        public_key: r.public_key,
        followers_count: r.followers_count,
        following_count: r.following_count,
        statuses_count: r.statuses_count,
        locked: r.locked,
        bot: r.bot,
        discoverable: r.discoverable,
        indexable: r.indexable,
        moved_to_uri: r.moved_to_uri,
        inbox_url: r.inbox_url,
        outbox_url: r.outbox_url,
        shared_inbox_url: r.shared_inbox_url,
        suspended_at: r.suspended_at,
        silenced_at: r.silenced_at,
        hide_collections: r.hide_collections,
        last_status_at: r.last_status_at,
        fields: r.fields,
        created_at: r.account_created_at,
        updated_at: r.account_updated_at,
    };
    Ok(Json(NotificationRequest {
        id: r.id.to_string(),
        created_at: r.created_at.to_rfc3339(),
        updated_at: r.created_at.to_rfc3339(),
        notifications_count: r.notifications_count.to_string(),
        last_status: None,
        account: super::convert::account_from_db(&acc),
    }))
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Parse `types[]=x`, `types=x`, `exclude_types[]=x`, `exclude_types=x`, and
/// `account_id=x` from the raw query string.  Mastodon clients use bracket
/// array notation (`foo[]=val`) which serde_urlencoded does not normalise.
fn parse_notif_filters(
    qs: Option<&str>,
) -> (Option<Vec<String>>, Option<Vec<String>>, Option<uuid::Uuid>) {
    let pairs: Vec<(std::borrow::Cow<str>, std::borrow::Cow<str>)> =
        url::form_urlencoded::parse(qs.unwrap_or("").as_bytes()).collect();

    let collect_arr = |plain: &str, bracket: &str| -> Option<Vec<String>> {
        let v: Vec<String> = pairs.iter()
            .filter(|(k, _)| k == plain || k == bracket)
            .map(|(_, v)| v.to_string())
            .collect();
        if v.is_empty() { None } else { Some(v) }
    };

    let types = collect_arr("types", "types[]");
    let exclude_types = collect_arr("exclude_types", "exclude_types[]");
    let account_id = pairs.iter()
        .find(|(k, _)| k == "account_id")
        .and_then(|(_, v)| v.parse::<uuid::Uuid>().ok());

    (types, exclude_types, account_id)
}

async fn build_notification(state: &AppState, n: &DbNotification) -> AppResult<Notification> {
    let from_account = sqlx::query_as!(
        Account,
        "SELECT * FROM accounts WHERE id = $1",
        n.from_account_id
    )
    .fetch_one(&state.db)
    .await?;

    let status = if let Some(status_id) = n.status_id {
        let s = sqlx::query_as!(
            crate::db::models::Status,
            "SELECT * FROM statuses WHERE id = $1 AND deleted_at IS NULL",
            status_id
        )
        .fetch_optional(&state.db)
        .await?;

        if let Some(s) = s {
            let account = sqlx::query_as!(
                Account,
                "SELECT * FROM accounts WHERE id = $1",
                s.account_id
            )
            .fetch_one(&state.db)
            .await?;
            let media = fetch_status_media(state, s.id).await?;
            let reblog = fetch_reblog_data(state, &s).await?;
            Some(build_status(state, &s, &account, media, reblog, None).await?)
        } else {
            None
        }
    } else {
        None
    };

    Ok(Notification {
        id: n.id.to_string(),
        notification_type: n.notification_type.clone(),
        created_at: n.created_at.to_rfc3339(),
        group_key: format!("ungrouped-{}", n.id),
        account: account_from_db(&from_account),
        status,
        filtered: None,
    })
}
