use axum::{
    extract::{Extension, Path, Query, RawQuery, State},
    Json,
};

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
        Notification, NotificationGroup, NotificationGroupsResponse, NotificationPolicy,
        NotificationPolicySummary, PaginationParams,
    },
};

// ── GET /api/v1/notifications ─────────────────────────────────────────────

pub async fn get_notifications(
    State(state): State<AppState>,
    Query(pagination): Query<PaginationParams>,
    RawQuery(qs): RawQuery,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<Notification>>> {
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
    sqlx::query!(
        "DELETE FROM notifications WHERE id = $1 AND account_id = $2",
        id, auth.account_id,
    )
    .execute(&state.db)
    .await?;
    Ok(Json(serde_json::json!({})))
}

// ── GET /api/v2/notifications ─────────────────────────────────────────────

pub async fn get_notifications_v2(
    State(state): State<AppState>,
    Query(pagination): Query<PaginationParams>,
    RawQuery(qs): RawQuery,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<NotificationGroupsResponse>> {
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

// ── GET /api/v2/notifications/policy ─────────────────────────────────────

pub async fn get_notification_policy(
    Extension(_auth): Extension<AuthenticatedUser>,
) -> Json<NotificationPolicy> {
    Json(NotificationPolicy {
        filter_not_following: false,
        filter_not_followers: false,
        filter_new_accounts: false,
        filter_private_mentions: false,
        summary: NotificationPolicySummary {
            pending_requests_count: 0,
            pending_notifications_count: 0,
        },
    })
}

// ── PATCH /api/v2/notifications/policy ───────────────────────────────────

pub async fn update_notification_policy(
    Extension(_auth): Extension<AuthenticatedUser>,
) -> Json<NotificationPolicy> {
    Json(NotificationPolicy::default())
}

// ── GET /api/v1/notifications/requests ───────────────────────────────────

pub async fn get_notification_requests(
    Extension(_auth): Extension<AuthenticatedUser>,
) -> Json<Vec<serde_json::Value>> {
    Json(vec![])
}

// ── POST /api/v1/notifications/requests/:id/accept ───────────────────────

pub async fn accept_notification_request(
    Extension(_auth): Extension<AuthenticatedUser>,
    axum::extract::Path(_id): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({}))
}

// ── POST /api/v1/notifications/requests/:id/dismiss ──────────────────────

pub async fn dismiss_notification_request(
    Extension(_auth): Extension<AuthenticatedUser>,
    axum::extract::Path(_id): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({}))
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
