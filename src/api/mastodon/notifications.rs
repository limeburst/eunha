use axum::{
    extract::{Extension, Path, Query, RawQuery, State},
    http::{header, HeaderMap, Uri},
    response::IntoResponse,
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
    accounts::{
        batch_reblog_data, batch_status_cards, batch_status_emojis, batch_status_media,
        batch_status_mentions, batch_status_polls, batch_status_tags, build_status,
        fetch_reblog_data, fetch_status_media,
    },
    convert::{account_from_db, status_from_db},
    types::{
        Notification, NotificationGroup, NotificationGroupsResponse, NotificationPagination,
        NotificationPolicy, NotificationPolicySummary, NotificationPolicyV1, NotificationRequest, PaginationParams,
    },
};

// ── GET /api/v1/notifications ─────────────────────────────────────────────

pub async fn get_notifications(
    State(state): State<AppState>,
    Query(pagination): Query<PaginationParams>,
    RawQuery(qs): RawQuery,
    uri: Uri,
    req_headers: HeaderMap,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<impl IntoResponse> {
    auth.require_scope("read:notifications")?;
    let limit = pagination.limit_clamped(40, 80);
    let max_id = pagination.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = pagination.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = pagination.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let (types, exclude_types, account_id) = parse_notif_filters(qs.as_deref());

    let notifications: Vec<DbNotification> = if min_id.is_some() {
        sqlx::query_as(
            r#"SELECT * FROM notifications
               WHERE account_id = $1
                 AND ($2::bigint IS NULL OR id > $2)
                 AND ($5::text[] IS NULL OR notification_type = ANY($5))
                 AND ($6::text[] IS NULL OR NOT (notification_type = ANY($6)))
                 AND ($7::bigint IS NULL OR from_account_id = $7)
                 AND NOT EXISTS (
                     SELECT 1 FROM mutes m
                     WHERE m.account_id = $1 AND m.target_account_id = from_account_id
                       AND m.hide_notifications = true
                       AND (m.expires_at IS NULL OR m.expires_at > now())
                 )
               ORDER BY id ASC
               LIMIT $4"#,
        )
        .bind(auth.account_id)
        .bind(min_id)
        .bind(Option::<i64>::None)
        .bind(limit)
        .bind(types)
        .bind(exclude_types)
        .bind(account_id)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as(
            r#"SELECT * FROM notifications
               WHERE account_id = $1
                 AND ($2::bigint IS NULL OR id < $2)
                 AND ($3::bigint IS NULL OR id > $3)
                 AND ($5::text[] IS NULL OR notification_type = ANY($5))
                 AND ($6::text[] IS NULL OR NOT (notification_type = ANY($6)))
                 AND ($7::bigint IS NULL OR from_account_id = $7)
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
        .await?
    };

    if notifications.is_empty() {
        return Ok((HeaderMap::new(), Json(vec![])));
    }

    let from_account_ids: Vec<i64> = notifications.iter()
        .map(|n| n.from_account_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let from_accounts_vec: Vec<Account> = sqlx::query_as!(
        Account,
        "SELECT * FROM accounts WHERE id = ANY($1::bigint[])",
        &from_account_ids,
    )
    .fetch_all(&state.db)
    .await?;
    let from_account_map: std::collections::HashMap<i64, Account> =
        from_accounts_vec.into_iter().map(|a| (a.id, a)).collect();

    let notif_status_ids: Vec<i64> = notifications.iter()
        .filter_map(|n| n.status_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let status_api_map: std::collections::HashMap<i64, super::types::Status> = if !notif_status_ids.is_empty() {
        let statuses: Vec<crate::db::models::Status> = sqlx::query_as!(
            crate::db::models::Status,
            "SELECT * FROM statuses WHERE id = ANY($1::bigint[]) AND deleted_at IS NULL",
            &notif_status_ids,
        )
        .fetch_all(&state.db)
        .await?;

        let stat_account_ids: Vec<i64> = statuses.iter()
            .map(|s| s.account_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        let stat_accounts: Vec<Account> = sqlx::query_as!(
            Account,
            "SELECT * FROM accounts WHERE id = ANY($1::bigint[])",
            &stat_account_ids,
        )
        .fetch_all(&state.db)
        .await?;
        let stat_account_map: std::collections::HashMap<i64, Account> =
            stat_accounts.into_iter().map(|a| (a.id, a)).collect();

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
        let polls_map = batch_status_polls(&state, &enrich_ids, Some(auth.account_id)).await?;
        let cards_map = batch_status_cards(&state, &enrich_ids).await?;
        let viewer_ctxs = super::statuses::batch_viewer_contexts(&state, auth.account_id, &all_ids).await?;

        let mut map = std::collections::HashMap::new();
        for s in &statuses {
            let Some(account) = stat_account_map.get(&s.account_id) else { continue };
            let media = media_map.get(&s.id).cloned().unwrap_or_default();
            let reblog = reblog_map.get(&s.id).cloned();
            let mentions = mentions_map.get(&s.id).cloned().unwrap_or_default();
            let rb_mentions = reblog.as_ref()
                .and_then(|(rs, _, _)| mentions_map.get(&rs.id))
                .cloned()
                .unwrap_or_default();
            let ctx = viewer_ctxs.get(&s.id).cloned();
            let mut api = status_from_db(s, account, media, reblog, ctx, &mentions, &rb_mentions);
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
            map.insert(s.id, api);
        }
        map
    } else {
        std::collections::HashMap::new()
    };

    // Batch-fetch reports for admin.report notifications
    let report_ids: Vec<i64> = notifications.iter()
        .filter_map(|n| if n.notification_type == "admin.report" { n.report_id } else { None })
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let report_map: std::collections::HashMap<i64, super::types::Report> = if !report_ids.is_empty() {
        let mut map = std::collections::HashMap::new();
        if let Ok(rows) = sqlx::query!(
            r#"SELECT r.id, r.comment, COALESCE(r.forwarded, false) AS "forwarded!",
                      r.category, r.action_taken_at, r.created_at, r.status_ids,
                      r.target_account_id
               FROM reports r
               WHERE r.id = ANY($1::bigint[])"#,
            &report_ids,
        )
        .fetch_all(&state.db)
        .await
        {
            let target_ids: Vec<i64> = rows.iter().map(|r| r.target_account_id).collect::<std::collections::HashSet<_>>().into_iter().collect();
            let target_accounts: std::collections::HashMap<i64, Account> = if !target_ids.is_empty() {
                sqlx::query_as!(Account, "SELECT * FROM accounts WHERE id = ANY($1::bigint[])", &target_ids)
                    .fetch_all(&state.db)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|a| (a.id, a))
                    .collect()
            } else {
                std::collections::HashMap::new()
            };
            for r in rows {
                let Some(ta) = target_accounts.get(&r.target_account_id) else { continue };
                map.insert(r.id, super::types::Report {
                    id: r.id.to_string(),
                    action_taken: r.action_taken_at.is_some(),
                    action_taken_at: r.action_taken_at.map(super::convert::mastodon_date),
                    category: r.category,
                    comment: r.comment,
                    forwarded: r.forwarded,
                    created_at: super::convert::mastodon_date(r.created_at),
                    status_ids: r.status_ids.iter().map(|i| i.to_string()).collect(),
                    rule_ids: vec![],
                    collection_ids: vec![],
                    target_account: account_from_db(ta),
                });
            }
        }
        map
    } else {
        std::collections::HashMap::new()
    };

    let mut result = Vec::with_capacity(notifications.len());
    for n in &notifications {
        let Some(account) = from_account_map.get(&n.from_account_id) else { continue };
        let status = n.status_id.and_then(|sid| status_api_map.get(&sid)).cloned();
        let report = n.report_id.and_then(|rid| report_map.get(&rid)).cloned();
        result.push(Notification {
            id: n.id.to_string(),
            notification_type: n.notification_type.clone(),
            created_at: super::convert::mastodon_date(n.created_at),
            group_key: format!("ungrouped-{}", n.id),
            account: account_from_db(account),
            status,
            report,
            filtered: None,
            event: None,
            moderation_warning: None,
            fallback: None,
            collection: None,
        });
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
             AND ($7::bigint IS NULL OR from_account_id = $7)
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

    // Batch-fetch from_accounts
    let from_account_ids: Vec<i64> = notifications.iter()
        .map(|n| n.from_account_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let from_accounts_vec: Vec<Account> = sqlx::query_as!(
        Account,
        "SELECT * FROM accounts WHERE id = ANY($1::bigint[])",
        &from_account_ids,
    )
    .fetch_all(&state.db)
    .await?;
    let from_account_map: std::collections::HashMap<i64, Account> =
        from_accounts_vec.into_iter().map(|a| (a.id, a)).collect();

    // Batch-fetch statuses
    let notif_status_ids: Vec<i64> = notifications.iter()
        .filter_map(|n| n.status_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let status_api_map: std::collections::HashMap<i64, super::types::Status> = if !notif_status_ids.is_empty() {
        let statuses: Vec<crate::db::models::Status> = sqlx::query_as!(
            crate::db::models::Status,
            "SELECT * FROM statuses WHERE id = ANY($1::bigint[]) AND deleted_at IS NULL",
            &notif_status_ids,
        )
        .fetch_all(&state.db)
        .await?;

        let stat_account_ids: Vec<i64> = statuses.iter()
            .map(|s| s.account_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        let stat_accounts: Vec<Account> = sqlx::query_as!(
            Account,
            "SELECT * FROM accounts WHERE id = ANY($1::bigint[])",
            &stat_account_ids,
        )
        .fetch_all(&state.db)
        .await?;
        let stat_account_map: std::collections::HashMap<i64, Account> =
            stat_accounts.into_iter().map(|a| (a.id, a)).collect();

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
        let polls_map = batch_status_polls(&state, &enrich_ids, Some(auth.account_id)).await?;
        let cards_map = batch_status_cards(&state, &enrich_ids).await?;
        let viewer_ctxs = super::statuses::batch_viewer_contexts(&state, auth.account_id, &all_ids).await?;

        let mut map = std::collections::HashMap::new();
        for s in &statuses {
            let Some(account) = stat_account_map.get(&s.account_id) else { continue };
            let media = media_map.get(&s.id).cloned().unwrap_or_default();
            let reblog = reblog_map.get(&s.id).cloned();
            let mentions = mentions_map.get(&s.id).cloned().unwrap_or_default();
            let rb_mentions = reblog.as_ref()
                .and_then(|(rs, _, _)| mentions_map.get(&rs.id))
                .cloned()
                .unwrap_or_default();
            let ctx = viewer_ctxs.get(&s.id).cloned();
            let mut api = status_from_db(s, account, media, reblog, ctx, &mentions, &rb_mentions);
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
            map.insert(s.id, api);
        }
        map
    } else {
        std::collections::HashMap::new()
    };

    // Build accounts and statuses deduplicated maps for the response
    let mut accounts_map: std::collections::HashMap<String, super::types::Account> =
        std::collections::HashMap::new();
    for a in from_account_map.values() {
        accounts_map.insert(a.id.to_string(), account_from_db(a));
    }
    let mut statuses_resp_map: std::collections::HashMap<String, super::types::Status> =
        std::collections::HashMap::new();

    let mut groups = Vec::with_capacity(notifications.len());
    for n in &notifications {
        let status_id = n.status_id.and_then(|sid| {
            if let Some(api) = status_api_map.get(&sid) {
                statuses_resp_map.insert(sid.to_string(), api.clone());
                Some(sid.to_string())
            } else {
                None
            }
        });

        let id_str = n.id.to_string();
        groups.push(NotificationGroup {
            group_key: format!("ungrouped-{}", id_str),
            notifications_count: 1,
            notification_type: n.notification_type.clone(),
            most_recent_notification_id: id_str.clone(),
            page_max_id: id_str.clone(),
            page_min_id: id_str.clone(),
            latest_page_notification_at: super::convert::mastodon_date(n.created_at),
            sample_account_ids: vec![n.from_account_id.to_string()],
            status_id,
        });
    }

    Ok(Json(NotificationGroupsResponse {
        notification_groups: groups,
        accounts: accounts_map.into_values().collect(),
        statuses: statuses_resp_map.into_values().collect(),
    }))
}

// ── GET /api/v2/notifications/:group_key ─────────────────────────────────

pub async fn get_notification_group(
    State(state): State<AppState>,
    Path(group_key): Path<String>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<NotificationGroup>> {
    auth.require_scope("read:notifications")?;
    let notif_id: i64 = group_key
        .strip_prefix("ungrouped-")
        .and_then(|s| s.parse().ok())
        .ok_or(AppError::NotFound)?;

    let n: DbNotification = sqlx::query_as(
        "SELECT * FROM notifications WHERE id = $1 AND account_id = $2",
    )
    .bind(notif_id)
    .bind(auth.account_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let id_str = n.id.to_string();
    Ok(Json(NotificationGroup {
        group_key: format!("ungrouped-{}", id_str),
        notifications_count: 1,
        notification_type: n.notification_type,
        most_recent_notification_id: id_str.clone(),
        page_max_id: id_str.clone(),
        page_min_id: id_str.clone(),
        latest_page_notification_at: super::convert::mastodon_date(n.created_at),
        sample_account_ids: vec![n.from_account_id.to_string()],
        status_id: n.status_id.map(|s| s.to_string()),
    }))
}

// ── POST /api/v2/notifications/:group_key/dismiss ─────────────────────────

pub async fn dismiss_notification_group(
    State(state): State<AppState>,
    Path(group_key): Path<String>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<serde_json::Value>> {
    auth.require_scope("write:notifications")?;
    let notif_id: i64 = group_key
        .strip_prefix("ungrouped-")
        .and_then(|s| s.parse().ok())
        .ok_or(AppError::NotFound)?;

    sqlx::query!(
        "DELETE FROM notifications WHERE id = $1 AND account_id = $2",
        notif_id,
        auth.account_id,
    )
    .execute(&state.db)
    .await?;

    Ok(Json(serde_json::json!({})))
}

// ── GET /api/v2/notifications/:group_key/accounts ────────────────────────

pub async fn get_notification_group_accounts(
    State(state): State<AppState>,
    Path(group_key): Path<String>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<super::types::Account>>> {
    auth.require_scope("read:notifications")?;
    let notif_id: i64 = group_key
        .strip_prefix("ungrouped-")
        .and_then(|s| s.parse().ok())
        .ok_or(AppError::NotFound)?;

    let n: DbNotification = sqlx::query_as(
        "SELECT * FROM notifications WHERE id = $1 AND account_id = $2",
    )
    .bind(notif_id)
    .bind(auth.account_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let account: Account = sqlx::query_as!(
        Account,
        "SELECT * FROM accounts WHERE id = $1",
        n.from_account_id,
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(vec![super::convert::account_from_db(&account)]))
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

fn bool_to_policy(b: bool) -> String {
    if b { "filter".to_string() } else { "accept".to_string() }
}

fn policy_to_bool(s: &str) -> bool {
    matches!(s, "filter" | "drop")
}

pub async fn get_notification_policy(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<NotificationPolicy>> {
    auth.require_scope("read:notifications")?;
    let user = sqlx::query!(
        r#"SELECT notif_filter_not_following, notif_filter_not_followers,
                  notif_filter_new_accounts, notif_filter_private_mentions,
                  notif_filter_limited_accounts
           FROM users WHERE account_id = $1"#,
        auth.account_id,
    )
    .fetch_one(&state.db)
    .await?;

    let any_filter = user.notif_filter_not_following
        || user.notif_filter_not_followers
        || user.notif_filter_new_accounts
        || user.notif_filter_private_mentions
        || user.notif_filter_limited_accounts;

    let (pending_requests, pending_notifs) = if any_filter {
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
        for_not_following: bool_to_policy(user.notif_filter_not_following),
        for_not_followers: bool_to_policy(user.notif_filter_not_followers),
        for_new_accounts: bool_to_policy(user.notif_filter_new_accounts),
        for_private_mentions: bool_to_policy(user.notif_filter_private_mentions),
        for_limited_accounts: bool_to_policy(user.notif_filter_limited_accounts),
        summary: NotificationPolicySummary {
            pending_requests_count: pending_requests,
            pending_notifications_count: pending_notifs,
        },
    }))
}

// ── PATCH /api/v2/notifications/policy ───────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct UpdateNotificationPolicyForm {
    pub for_not_following: Option<String>,
    pub for_not_followers: Option<String>,
    pub for_new_accounts: Option<String>,
    pub for_private_mentions: Option<String>,
    pub for_limited_accounts: Option<String>,
}

pub async fn update_notification_policy(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<UpdateNotificationPolicyForm>,
) -> AppResult<Json<NotificationPolicy>> {
    auth.require_scope("write:notifications")?;
    let filter_not_following    = form.for_not_following.as_deref().map(policy_to_bool);
    let filter_not_followers    = form.for_not_followers.as_deref().map(policy_to_bool);
    let filter_new_accounts     = form.for_new_accounts.as_deref().map(policy_to_bool);
    let filter_private_mentions = form.for_private_mentions.as_deref().map(policy_to_bool);
    let filter_limited_accounts = form.for_limited_accounts.as_deref().map(policy_to_bool);
    sqlx::query!(
        r#"UPDATE users SET
               notif_filter_not_following    = COALESCE($2, notif_filter_not_following),
               notif_filter_not_followers    = COALESCE($3, notif_filter_not_followers),
               notif_filter_new_accounts     = COALESCE($4, notif_filter_new_accounts),
               notif_filter_private_mentions = COALESCE($5, notif_filter_private_mentions),
               notif_filter_limited_accounts = COALESCE($6, notif_filter_limited_accounts),
               updated_at = now()
           WHERE account_id = $1"#,
        auth.account_id,
        filter_not_following,
        filter_not_followers,
        filter_new_accounts,
        filter_private_mentions,
        filter_limited_accounts,
    )
    .execute(&state.db)
    .await?;

    get_notification_policy(State(state), Extension(auth)).await
}

// ── GET /api/v1/notifications/policy ─────────────────────────────────────────

pub async fn get_notification_policy_v1(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<NotificationPolicyV1>> {
    auth.require_scope("read:notifications")?;
    let user = sqlx::query!(
        r#"SELECT notif_filter_not_following, notif_filter_not_followers,
                  notif_filter_new_accounts, notif_filter_private_mentions
           FROM users WHERE account_id = $1"#,
        auth.account_id,
    )
    .fetch_one(&state.db)
    .await?;

    let any_filter = user.notif_filter_not_following
        || user.notif_filter_not_followers
        || user.notif_filter_new_accounts
        || user.notif_filter_private_mentions;

    let (pending_requests, pending_notifs) = if any_filter {
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

    Ok(Json(NotificationPolicyV1 {
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

// ── PATCH /api/v1/notifications/policy ───────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct UpdateNotificationPolicyV1Form {
    pub filter_not_following: Option<bool>,
    pub filter_not_followers: Option<bool>,
    pub filter_new_accounts: Option<bool>,
    pub filter_private_mentions: Option<bool>,
}

pub async fn update_notification_policy_v1(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<UpdateNotificationPolicyV1Form>,
) -> AppResult<Json<NotificationPolicyV1>> {
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

    get_notification_policy_v1(State(state), Extension(auth)).await
}

// ── GET /api/v1/notifications/requests ───────────────────────────────────

pub async fn get_notification_requests(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Query(pagination): Query<NotificationPagination>,
    uri: Uri,
    req_headers: HeaderMap,
) -> AppResult<impl IntoResponse> {
    auth.require_scope("read:notifications")?;
    let limit = pagination.limit.unwrap_or(40).min(80).max(1) as i64;
    let max_id = pagination.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = pagination.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = pagination.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let rows = sqlx::query!(
        r#"SELECT nr.id, nr.from_account_id, nr.last_status_id, nr.notifications_count, nr.created_at, nr.updated_at,
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
             AND ($3::bigint IS NULL OR nr.id < $3)
             AND ($4::bigint IS NULL OR nr.id > $4)
             AND ($5::bigint IS NULL OR nr.id > $5)
           ORDER BY nr.id DESC
           LIMIT $2"#,
        auth.account_id, limit, max_id, since_id, min_id,
    )
    .fetch_all(&state.db)
    .await?;

    // Batch-fetch and enrich all last statuses up front
    let last_status_ids: Vec<i64> = rows.iter()
        .filter_map(|r| r.last_status_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let mut last_status_map: std::collections::HashMap<i64, super::types::Status> =
        std::collections::HashMap::new();

    if !last_status_ids.is_empty() {
        let ls_statuses: Vec<crate::db::models::Status> = sqlx::query_as!(
            crate::db::models::Status,
            "SELECT * FROM statuses WHERE id = ANY($1::bigint[]) AND deleted_at IS NULL",
            &last_status_ids,
        )
        .fetch_all(&state.db)
        .await?;

        let ls_media_map = batch_status_media(&state, &last_status_ids).await?;
        let ls_reblog_map = batch_reblog_data(&state, &ls_statuses).await?;
        let ls_reblog_ids: Vec<i64> = ls_reblog_map.values().map(|(rs, _, _)| rs.id).collect();
        let mut ls_enrich_ids = last_status_ids.clone();
        ls_enrich_ids.extend_from_slice(&ls_reblog_ids);
        let ls_tags_map = batch_status_tags(&state, &ls_enrich_ids).await?;
        let ls_mentions_map = batch_status_mentions(&state, &ls_enrich_ids).await?;
        let ls_all_for_emoji: Vec<crate::db::models::Status> = ls_statuses.iter().cloned()
            .chain(ls_reblog_map.values().map(|(rs, _, _)| rs.clone()))
            .collect();
        let ls_emojis_map = batch_status_emojis(&state, &ls_all_for_emoji).await?;
        let ls_polls_map = batch_status_polls(&state, &ls_enrich_ids, Some(auth.account_id)).await?;
        let ls_cards_map = batch_status_cards(&state, &ls_enrich_ids).await?;

        let ls_account_ids: Vec<i64> = ls_statuses.iter().map(|s| s.account_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        let ls_accounts: Vec<Account> = sqlx::query_as!(
            Account,
            "SELECT * FROM accounts WHERE id = ANY($1::bigint[])",
            &ls_account_ids,
        )
        .fetch_all(&state.db)
        .await?;
        let ls_account_map: std::collections::HashMap<i64, Account> =
            ls_accounts.into_iter().map(|a| (a.id, a)).collect();

        for s in &ls_statuses {
            let Some(account) = ls_account_map.get(&s.account_id) else { continue };
            let media = ls_media_map.get(&s.id).cloned().unwrap_or_default();
            let reblog = ls_reblog_map.get(&s.id).cloned();
            let mentions = ls_mentions_map.get(&s.id).cloned().unwrap_or_default();
            let rb_mentions = reblog.as_ref()
                .and_then(|(rs, _, _)| ls_mentions_map.get(&rs.id))
                .cloned()
                .unwrap_or_default();
            let mut api = status_from_db(s, account, media, reblog, None, &mentions, &rb_mentions);
            api.tags = ls_tags_map.get(&s.id).cloned().unwrap_or_default();
            api.mentions = mentions;
            api.emojis = ls_emojis_map.get(&s.id).cloned().unwrap_or_default();
            api.poll = ls_polls_map.get(&s.id).cloned();
            api.card = ls_cards_map.get(&s.id).cloned();
            if let Some(ref mut rb) = api.reblog {
                let rid: i64 = rb.id.parse().unwrap_or(0);
                rb.tags = ls_tags_map.get(&rid).cloned().unwrap_or_default();
                rb.mentions = rb_mentions;
                rb.emojis = ls_emojis_map.get(&rid).cloned().unwrap_or_default();
                rb.poll = ls_polls_map.get(&rid).cloned();
                rb.card = ls_cards_map.get(&rid).cloned();
            }
            last_status_map.insert(s.id, api);
        }
    }

    let mut result: Vec<NotificationRequest> = Vec::with_capacity(rows.len());
    for r in rows {
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
            sensitized_at: None,
            hide_collections: r.hide_collections,
            last_status_at: r.last_status_at,
            fields: r.fields,
            attribution_domains: vec![],
            created_at: r.account_created_at,
            updated_at: r.account_updated_at,
        };
        let last_status = r.last_status_id.and_then(|id| last_status_map.remove(&id));
        result.push(NotificationRequest {
            id: r.id.to_string(),
            created_at: super::convert::mastodon_date(r.created_at),
            updated_at: super::convert::mastodon_date(r.updated_at),
            notifications_count: r.notifications_count.to_string(),
            last_status,
            account: super::convert::account_from_db(&acc),
        });
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

// ── POST /api/v1/notifications/requests/:id/accept ───────────────────────

pub async fn accept_notification_request(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    auth.require_scope("write:notifications")?;
    let deleted = sqlx::query!(
        "DELETE FROM notification_requests WHERE id = $1 AND account_id = $2",
        id, auth.account_id,
    )
    .execute(&state.db)
    .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
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

// ── POST /api/v1/notifications/requests/accept_all ───────────────────────

pub async fn accept_all_notification_requests(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<serde_json::Value>> {
    auth.require_scope("write:notifications")?;
    sqlx::query!(
        "DELETE FROM notification_requests WHERE account_id = $1 AND NOT dismissed",
        auth.account_id,
    )
    .execute(&state.db)
    .await?;
    Ok(Json(serde_json::json!({})))
}

// ── POST /api/v1/notifications/requests/dismiss_all ──────────────────────

pub async fn dismiss_all_notification_requests(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<serde_json::Value>> {
    auth.require_scope("write:notifications")?;
    sqlx::query!(
        "UPDATE notification_requests SET dismissed = true WHERE account_id = $1",
        auth.account_id,
    )
    .execute(&state.db)
    .await?;
    Ok(Json(serde_json::json!({})))
}

// ── GET /api/v1/notifications/requests/merged ────────────────────────────

pub async fn get_notification_requests_merged(
    State(_state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<serde_json::Value>> {
    auth.require_scope("read:notifications")?;
    Ok(Json(serde_json::json!({ "merged": true })))
}

// ── GET /api/v1/notifications/requests/:id ───────────────────────────────

pub async fn get_notification_request(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<NotificationRequest>> {
    auth.require_scope("read:notifications")?;
    let r = sqlx::query!(
        r#"SELECT nr.id, nr.from_account_id, nr.last_status_id, nr.notifications_count, nr.created_at, nr.updated_at,
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
        sensitized_at: None,
        hide_collections: r.hide_collections,
        last_status_at: r.last_status_at,
        fields: r.fields,
        attribution_domains: vec![],
        created_at: r.account_created_at,
        updated_at: r.account_updated_at,
    };
    let last_status = fetch_last_status(&state, r.last_status_id).await;
    Ok(Json(NotificationRequest {
        id: r.id.to_string(),
        created_at: super::convert::mastodon_date(r.created_at),
        updated_at: super::convert::mastodon_date(r.updated_at),
        notifications_count: r.notifications_count.to_string(),
        last_status,
        account: super::convert::account_from_db(&acc),
    }))
}

// ── Helpers ────────────────────────────────────────────────────────────────

async fn fetch_last_status(
    state: &AppState,
    last_status_id: Option<i64>,
) -> Option<super::types::Status> {
    let status_id = last_status_id?;
    let s = sqlx::query_as!(
        crate::db::models::Status,
        "SELECT * FROM statuses WHERE id = $1 AND deleted_at IS NULL",
        status_id,
    )
    .fetch_optional(&state.db)
    .await.ok()??;
    let account = sqlx::query_as!(
        crate::db::models::Account,
        "SELECT * FROM accounts WHERE id = $1",
        s.account_id,
    )
    .fetch_one(&state.db)
    .await.ok()?;
    let media = fetch_status_media(state, s.id).await.ok()?;
    let reblog = fetch_reblog_data(state, &s).await.ok()?;
    build_status(state, &s, &account, media, reblog, None).await.ok()
}

/// Parse `types[]=x`, `types=x`, `exclude_types[]=x`, `exclude_types=x`, and
/// `account_id=x` from the raw query string.  Mastodon clients use bracket
/// array notation (`foo[]=val`) which serde_urlencoded does not normalise.
fn parse_notif_filters(
    qs: Option<&str>,
) -> (Option<Vec<String>>, Option<Vec<String>>, Option<i64>) {
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
        .and_then(|(_, v)| v.parse::<i64>().ok());

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

    let report = if n.notification_type == "admin.report" {
        if let Some(rid) = n.report_id {
            sqlx::query!(
                r#"SELECT r.id, r.comment, COALESCE(r.forwarded, false) AS "forwarded!",
                          r.category, r.action_taken_at, r.created_at, r.status_ids,
                          r.target_account_id
                   FROM reports r WHERE r.id = $1"#,
                rid,
            )
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten()
            .and_then(|r| {
                // We'll fetch target_account synchronously in a blocking manner — acceptable here
                // since this is the single-notification endpoint (not batch).
                // Store report data without target_account for now; caller must handle.
                Some((r.id, r.comment, r.forwarded, r.category, r.action_taken_at, r.created_at, r.status_ids, r.target_account_id))
            })
        } else { None }
    } else { None };

    let report = if let Some((rid, comment, forwarded, category, action_taken_at, created_at_r, status_ids, ta_id)) = report {
        let ta = sqlx::query_as!(Account, "SELECT * FROM accounts WHERE id = $1", ta_id)
            .fetch_optional(&state.db).await.ok().flatten();
        ta.as_ref().map(|ta| super::types::Report {
            id: rid.to_string(),
            action_taken: action_taken_at.is_some(),
            action_taken_at: action_taken_at.map(super::convert::mastodon_date),
            category,
            comment,
            forwarded,
            created_at: super::convert::mastodon_date(created_at_r),
            status_ids: status_ids.iter().map(|i| i.to_string()).collect(),
            rule_ids: vec![],
            collection_ids: vec![],
            target_account: account_from_db(ta),
        })
    } else { None };

    Ok(Notification {
        id: n.id.to_string(),
        notification_type: n.notification_type.clone(),
        created_at: super::convert::mastodon_date(n.created_at),
        group_key: format!("ungrouped-{}", n.id),
        account: account_from_db(&from_account),
        status,
        report,
        filtered: None,
        event: None,
        moderation_warning: None,
        fallback: None,
        collection: None,
    })
}
