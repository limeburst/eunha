use axum::{
    extract::{Extension, Multipart, Path, Query, State},
    http::{header, HeaderMap, StatusCode, Uri},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use crate::{
    db::models,
    error::{AppError, AppResult},
    middleware::{AuthenticatedUser, ResolvedInstance},
    state::AppState,
};
use super::accounts::{batch_account_roles, fetch_account_emojis};
use super::convert::account_from_db;
use super::types::Account as ApiAccount;

// ── Admin auth guard ──────────────────────────────────────────────────────

async fn require_admin(state: &AppState, account_id: i64) -> AppResult<()> {
    let role = sqlx::query_scalar!(
        "SELECT role FROM users WHERE account_id = $1",
        account_id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Unauthorized)?;

    if role != "admin" && role != "moderator" {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

// ── Admin Account type ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AdminAccount {
    pub id: String,
    pub username: String,
    pub domain: Option<String>,
    pub created_at: String,
    pub email: String,
    pub ip: Option<String>,
    pub ips: Vec<serde_json::Value>,
    pub role: AdminRole,
    pub confirmed: bool,
    pub suspended: bool,
    pub silenced: bool,
    pub sensitized: bool,
    pub disabled: bool,
    pub approved: bool,
    pub locale: Option<String>,
    pub invite_request: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by_application_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invited_by_account_id: Option<String>,
    pub account: ApiAccount,
}

#[derive(Debug, Serialize)]
pub struct AdminRole {
    pub id: String,
    pub name: String,
    pub color: String,
    pub position: i32,
    pub permissions: i64,
    pub highlighted: bool,
    pub created_at: String,
    pub updated_at: String,
}

fn role_for(role_str: &str) -> AdminRole {
    match role_str {
        "admin" => AdminRole {
            id: "1".into(),
            name: "Admin".into(),
            color: "#6364ff".into(),
            position: 100,
            permissions: 2031612,
            highlighted: true,
            created_at: "2022-09-08T22:48:07.983Z".into(),
            updated_at: "2022-09-08T22:48:07.983Z".into(),
        },
        "moderator" => AdminRole {
            id: "2".into(),
            name: "Moderator".into(),
            color: "#6364ff".into(),
            position: 10,
            permissions: 1049884,
            highlighted: true,
            created_at: "2022-09-08T22:48:07.983Z".into(),
            updated_at: "2022-09-08T22:48:07.983Z".into(),
        },
        _ => AdminRole {
            id: "0".into(),
            name: "".into(),
            color: "".into(),
            position: 0,
            permissions: 0,
            highlighted: false,
            created_at: "2022-09-08T22:48:07.983Z".into(),
            updated_at: "2022-09-08T22:48:07.983Z".into(),
        },
    }
}

async fn build_admin_account(state: &AppState, account: &models::Account) -> AppResult<AdminAccount> {
    let user = sqlx::query!(
        "SELECT id, email, confirmed_at, approved_at, reason, role FROM users WHERE account_id = $1",
        account.id,
    )
    .fetch_optional(&state.db)
    .await?;

    let (user_id, email, confirmed, approved, reason, role_str) = match user {
        Some(u) => (
            Some(u.id),
            u.email,
            u.confirmed_at.is_some(),
            u.approved_at.is_some(),
            u.reason,
            u.role,
        ),
        None => (None, String::new(), true, true, None, "user".to_string()),
    };

    // Fetch IP addresses from user_ips view for local accounts
    let ips: Vec<serde_json::Value> = if let Some(uid) = user_id {
        sqlx::query!(
            "SELECT ip::text AS ip, used_at FROM user_ips WHERE user_id = $1 ORDER BY used_at DESC LIMIT 20",
            uid,
        )
        .fetch_all(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|r| serde_json::json!({
            "ip": r.ip.unwrap_or_default(),
            "used_at": r.used_at.map(super::convert::mastodon_date),
        }))
        .collect()
    } else {
        vec![]
    };
    let first_ip = ips.first().and_then(|v| v["ip"].as_str().map(str::to_string));

    Ok(AdminAccount {
        id: account.id.to_string(),
        username: account.username.clone(),
        domain: account.domain.clone(),
        created_at: super::convert::mastodon_date(account.created_at),
        email,
        ip: first_ip,
        ips,
        role: role_for(&role_str),
        confirmed,
        suspended: account.suspended_at.is_some(),
        silenced: account.silenced_at.is_some(),
        sensitized: account.sensitized_at.is_some(),
        disabled: false,
        approved,
        locale: None,
        invite_request: reason,
        created_by_application_id: None,
        invited_by_account_id: None,
        account: {
            let mut api = account_from_db(account);
            api.emojis = fetch_account_emojis(state, account).await;
            api.roles = {
                let m = batch_account_roles(state, std::slice::from_ref(account)).await;
                m.get(&account.id).cloned().unwrap_or_default()
            };
            api
        },
    })
}

// ── GET /api/v1/admin/accounts ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AdminAccountsParams {
    pub status: Option<String>,
    pub username: Option<String>,
    pub by_domain: Option<String>,
    pub role_ids: Option<Vec<String>>,
    pub limit: Option<i64>,
    pub max_id: Option<String>,
    pub min_id: Option<String>,
    pub since_id: Option<String>,
}

pub async fn list_admin_accounts(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Query(params): Query<AdminAccountsParams>,
    uri: Uri,
    req_headers: HeaderMap,
) -> AppResult<impl IntoResponse> {
    require_admin(&state, auth.account_id).await?;

    let limit = params.limit.unwrap_or(40).min(80).max(1);
    let max_id = params.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = params.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = params.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let accounts = sqlx::query_as!(
        models::Account,
        r#"SELECT a.* FROM accounts a
           WHERE a.domain IS NULL
             AND ($2::text IS NULL OR a.username = $2)
             AND ($3::text IS NULL OR
                  ($3 = 'suspended' AND a.suspended_at IS NOT NULL) OR
                  ($3 = 'silenced' AND a.silenced_at IS NOT NULL) OR
                  ($3 = 'sensitized' AND a.sensitized_at IS NOT NULL) OR
                  ($3 = 'disabled' AND a.suspended_at IS NOT NULL) OR
                  ($3 = 'pending' AND EXISTS (
                      SELECT 1 FROM users u WHERE u.account_id = a.id AND u.approved_at IS NULL
                  )) OR
                  ($3 = 'active' AND a.suspended_at IS NULL AND a.silenced_at IS NULL
                      AND NOT EXISTS (SELECT 1 FROM users u WHERE u.account_id = a.id AND u.approved_at IS NULL))
             )
             AND ($4::bigint IS NULL OR a.id < $4)
             AND ($5::bigint IS NULL OR a.id > $5)
             AND ($6::bigint IS NULL OR a.id > $6)
           ORDER BY a.id DESC
           LIMIT $1"#,
        limit, params.username.as_deref(), params.status.as_deref(),
        max_id, since_id, min_id,
    )
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::with_capacity(accounts.len());
    for a in &accounts {
        result.push(build_admin_account(&state, a).await?);
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

// ── GET /api/v2/admin/accounts ────────────────────────────────────────────
// Adds origin (local/remote) and display_name filters on top of v1.

#[derive(Debug, Deserialize)]
pub struct AdminAccountsV2Params {
    pub origin: Option<String>,      // "local" | "remote"
    pub status: Option<String>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub by_domain: Option<String>,
    pub email: Option<String>,
    pub role_ids: Option<Vec<String>>,
    pub limit: Option<i64>,
    pub max_id: Option<String>,
    pub min_id: Option<String>,
    pub since_id: Option<String>,
}

pub async fn list_admin_accounts_v2(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Query(params): Query<AdminAccountsV2Params>,
    uri: Uri,
    req_headers: HeaderMap,
) -> AppResult<impl IntoResponse> {
    require_admin(&state, auth.account_id).await?;

    let limit = params.limit.unwrap_or(40).min(80).max(1);
    let max_id = params.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = params.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = params.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    // origin filter: "local" = domain IS NULL, "remote" = domain IS NOT NULL
    let local_only = params.origin.as_deref() == Some("local");
    let remote_only = params.origin.as_deref() == Some("remote");

    let display_name_pattern = params.display_name.as_ref().map(|s| format!("%{}%", s));
    let accounts = sqlx::query_as!(
        models::Account,
        r#"SELECT a.* FROM accounts a
           LEFT JOIN users u ON u.account_id = a.id
           WHERE ($2::boolean IS NOT TRUE OR a.domain IS NULL)
             AND ($3::boolean IS NOT TRUE OR a.domain IS NOT NULL)
             AND ($4::text IS NULL OR a.username ILIKE $4)
             AND ($5::text IS NULL OR a.display_name ILIKE $5)
             AND ($6::text IS NULL OR
                  ($6 = 'suspended' AND a.suspended_at IS NOT NULL) OR
                  ($6 = 'silenced' AND a.silenced_at IS NOT NULL) OR
                  ($6 = 'sensitized' AND a.sensitized_at IS NOT NULL) OR
                  ($6 = 'disabled' AND a.suspended_at IS NOT NULL) OR
                  ($6 = 'pending' AND u.approved_at IS NULL AND u.id IS NOT NULL) OR
                  ($6 = 'active' AND a.suspended_at IS NULL AND a.silenced_at IS NULL
                      AND (u.id IS NULL OR u.approved_at IS NOT NULL))
             )
             AND ($7::text IS NULL OR (lower(u.email) = lower($7) AND a.domain IS NULL))
             AND ($8::bigint IS NULL OR a.id < $8)
             AND ($9::bigint IS NULL OR a.id > $9)
             AND ($10::bigint IS NULL OR a.id > $10)
           ORDER BY a.id DESC
           LIMIT $1"#,
        limit,
        local_only, remote_only,
        params.username.as_deref(),
        display_name_pattern.as_deref(),
        params.status.as_deref(),
        params.email.as_deref(),
        max_id, since_id, min_id,
    ).fetch_all(&state.db).await?;

    let mut result = Vec::with_capacity(accounts.len());
    for a in &accounts {
        result.push(build_admin_account(&state, a).await?);
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

// ── GET /api/v1/admin/accounts/:id ───────────────────────────────────────

pub async fn get_admin_account(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminAccount>> {
    require_admin(&state, auth.account_id).await?;
    let account = sqlx::query_as!(models::Account, "SELECT * FROM accounts WHERE id = $1", id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(build_admin_account(&state, &account).await?))
}

// ── POST /api/v1/admin/accounts/:id/approve ──────────────────────────────

pub async fn approve_account(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminAccount>> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!(
        "UPDATE users SET approved_at = now() WHERE account_id = $1",
        id,
    )
    .execute(&state.db)
    .await?;
    let account = sqlx::query_as!(models::Account, "SELECT * FROM accounts WHERE id = $1", id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(build_admin_account(&state, &account).await?))
}

// ── POST /api/v1/admin/accounts/:id/reject ───────────────────────────────

pub async fn reject_account(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!(
        "UPDATE users SET approved_at = NULL WHERE account_id = $1 AND approved_at IS NULL",
        id,
    )
    .execute(&state.db)
    .await?;
    sqlx::query!(
        "UPDATE accounts SET suspended_at = now() WHERE id = $1",
        id,
    )
    .execute(&state.db)
    .await?;
    Ok(StatusCode::OK)
}

// ── POST /api/v1/admin/accounts/:id/enable ───────────────────────────────

pub async fn enable_account(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminAccount>> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!(
        "UPDATE accounts SET suspended_at = NULL WHERE id = $1",
        id,
    )
    .execute(&state.db)
    .await?;
    let account = sqlx::query_as!(models::Account, "SELECT * FROM accounts WHERE id = $1", id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(build_admin_account(&state, &account).await?))
}

// ── POST /api/v1/admin/accounts/:id/silence ──────────────────────────────

pub async fn silence_account(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminAccount>> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!(
        "UPDATE accounts SET silenced_at = now() WHERE id = $1 AND silenced_at IS NULL",
        id,
    )
    .execute(&state.db)
    .await?;
    let account = sqlx::query_as!(models::Account, "SELECT * FROM accounts WHERE id = $1", id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(build_admin_account(&state, &account).await?))
}

// ── POST /api/v1/admin/accounts/:id/unsilence ────────────────────────────

pub async fn unsilence_account(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminAccount>> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!(
        "UPDATE accounts SET silenced_at = NULL WHERE id = $1",
        id,
    )
    .execute(&state.db)
    .await?;
    let account = sqlx::query_as!(models::Account, "SELECT * FROM accounts WHERE id = $1", id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(build_admin_account(&state, &account).await?))
}

// ── POST /api/v1/admin/accounts/:id/suspend ──────────────────────────────

pub async fn suspend_account(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminAccount>> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!(
        "UPDATE accounts SET suspended_at = now() WHERE id = $1 AND suspended_at IS NULL",
        id,
    )
    .execute(&state.db)
    .await?;
    sqlx::query!(
        "UPDATE statuses SET deleted_at = now() WHERE account_id = $1 AND deleted_at IS NULL",
        id,
    )
    .execute(&state.db)
    .await?;
    let account = sqlx::query_as!(models::Account, "SELECT * FROM accounts WHERE id = $1", id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(build_admin_account(&state, &account).await?))
}

// ── POST /api/v1/admin/accounts/:id/unsuspend ────────────────────────────

pub async fn unsuspend_account(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminAccount>> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!(
        "UPDATE accounts SET suspended_at = NULL WHERE id = $1",
        id,
    )
    .execute(&state.db)
    .await?;
    let account = sqlx::query_as!(models::Account, "SELECT * FROM accounts WHERE id = $1", id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(build_admin_account(&state, &account).await?))
}

// ── Admin Report type ─────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AdminReport {
    pub id: String,
    pub action_taken: bool,
    pub action_taken_at: Option<String>,
    pub category: String,
    pub comment: String,
    pub forwarded: bool,
    pub created_at: String,
    pub updated_at: String,
    pub account: ApiAccount,
    pub target_account: ApiAccount,
    pub assigned_account: Option<ApiAccount>,
    pub action_taken_by_account: Option<ApiAccount>,
    pub statuses: Vec<serde_json::Value>,
    pub rules: Vec<serde_json::Value>,
}

async fn build_admin_report(
    state: &AppState,
    report: &AdminReportRow,
) -> AppResult<AdminReport> {
    let account = sqlx::query_as!(
        models::Account,
        "SELECT * FROM accounts WHERE id = $1",
        report.account_id,
    )
    .fetch_one(&state.db)
    .await?;
    let target = sqlx::query_as!(
        models::Account,
        "SELECT * FROM accounts WHERE id = $1",
        report.target_account_id,
    )
    .fetch_one(&state.db)
    .await?;

    let mut account_api = account_from_db(&account);
    account_api.emojis = fetch_account_emojis(state, &account).await;
    account_api.roles = {
        let m = batch_account_roles(state, std::slice::from_ref(&account)).await;
        m.get(&account.id).cloned().unwrap_or_default()
    };
    let mut target_api = account_from_db(&target);
    target_api.emojis = fetch_account_emojis(state, &target).await;
    target_api.roles = {
        let m = batch_account_roles(state, std::slice::from_ref(&target)).await;
        m.get(&target.id).cloned().unwrap_or_default()
    };
    Ok(AdminReport {
        id: report.id.to_string(),
        action_taken: report.action_taken_at.is_some(),
        action_taken_at: report.action_taken_at.map(super::convert::mastodon_date),
        category: report.category.clone(),
        comment: report.comment.clone(),
        forwarded: report.forwarded.unwrap_or(false),
        created_at: super::convert::mastodon_date(report.created_at),
        updated_at: super::convert::mastodon_date(report.updated_at),
        account: account_api,
        target_account: target_api,
        assigned_account: None,
        action_taken_by_account: None,
        statuses: vec![],
        rules: vec![],
    })
}

struct AdminReportRow {
    id: i64,
    account_id: i64,
    target_account_id: i64,
    comment: String,
    forwarded: Option<bool>,
    category: String,
    action_taken_at: Option<chrono::DateTime<chrono::Utc>>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

// ── GET /api/v1/admin/reports ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AdminReportsParams {
    pub resolved: Option<bool>,
    pub limit: Option<i64>,
    pub max_id: Option<String>,
    pub min_id: Option<String>,
    pub since_id: Option<String>,
}

pub async fn list_admin_reports(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Query(params): Query<AdminReportsParams>,
    uri: Uri,
    req_headers: HeaderMap,
) -> AppResult<impl IntoResponse> {
    require_admin(&state, auth.account_id).await?;

    let limit = params.limit.unwrap_or(20).min(40).max(1);
    let resolved = params.resolved.unwrap_or(false);
    let max_id = params.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = params.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = params.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let rows = sqlx::query!(
        r#"SELECT r.id, r.account_id, r.target_account_id,
                  r.comment, r.forwarded, r.action_taken_at,
                  r.created_at, r.updated_at,
                  CASE r.category WHEN 0 THEN 'other' WHEN 1 THEN 'spam' WHEN 2 THEN 'violation' ELSE 'other' END AS "category!"
           FROM reports r
           WHERE ($1 = (r.action_taken_at IS NOT NULL))
             AND ($3::bigint IS NULL OR r.id < $3)
             AND ($4::bigint IS NULL OR r.id > $4)
             AND ($5::bigint IS NULL OR r.id > $5)
           ORDER BY r.id DESC
           LIMIT $2"#,
        resolved, limit, max_id, since_id, min_id,
    )
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::with_capacity(rows.len());
    for r in &rows {
        let row = AdminReportRow {
            id: r.id,
            account_id: r.account_id,
            target_account_id: r.target_account_id,
            comment: r.comment.clone(),
            forwarded: r.forwarded,
            category: r.category.clone(),
            action_taken_at: r.action_taken_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        };
        result.push(build_admin_report(&state, &row).await?);
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

// ── GET /api/v1/admin/reports/:id ────────────────────────────────────────

pub async fn get_admin_report(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminReport>> {
    require_admin(&state, auth.account_id).await?;
    let r = sqlx::query!(
        r#"SELECT r.id, r.account_id, r.target_account_id,
                  r.comment, r.forwarded, r.action_taken_at,
                  r.created_at, r.updated_at,
                  CASE r.category WHEN 0 THEN 'other' WHEN 1 THEN 'spam' WHEN 2 THEN 'violation' ELSE 'other' END AS "category!"
           FROM reports r
           WHERE r.id = $1"#,
        id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    let row = AdminReportRow {
        id: r.id,
        account_id: r.account_id,
        target_account_id: r.target_account_id,
        comment: r.comment.clone(),
        forwarded: r.forwarded,
        category: r.category.clone(),
        action_taken_at: r.action_taken_at,
        created_at: r.created_at,
        updated_at: r.updated_at,
    };
    Ok(Json(build_admin_report(&state, &row).await?))
}

// ── POST /api/v1/admin/reports/:id/resolve ───────────────────────────────

pub async fn resolve_report(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminReport>> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!(
        "UPDATE reports SET action_taken_at = now(), action_taken_by_account_id = $1 WHERE id = $2",
        auth.account_id, id,
    )
    .execute(&state.db)
    .await?;
    let r = sqlx::query!(
        r#"SELECT r.id, r.account_id, r.target_account_id,
                  r.comment, r.forwarded, r.action_taken_at,
                  r.created_at, r.updated_at,
                  CASE r.category WHEN 0 THEN 'other' WHEN 1 THEN 'spam' WHEN 2 THEN 'violation' ELSE 'other' END AS "category!"
           FROM reports r
           WHERE r.id = $1"#,
        id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    let row = AdminReportRow {
        id: r.id,
        account_id: r.account_id,
        target_account_id: r.target_account_id,
        comment: r.comment.clone(),
        forwarded: r.forwarded,
        category: r.category.clone(),
        action_taken_at: r.action_taken_at,
        created_at: r.created_at,
        updated_at: r.updated_at,
    };
    Ok(Json(build_admin_report(&state, &row).await?))
}

// ── POST /api/v1/admin/reports/:id/reopen ────────────────────────────────

pub async fn reopen_report(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminReport>> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!(
        "UPDATE reports SET action_taken_at = NULL, action_taken_by_account_id = NULL WHERE id = $1",
        id,
    )
    .execute(&state.db)
    .await?;
    get_admin_report(State(state), Extension(auth), Path(id)).await
}

// ── GET /api/v1/admin/roles ───────────────────────────────────────────────

pub async fn list_admin_roles(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<AdminRole>>> {
    require_admin(&state, auth.account_id).await?;
    Ok(Json(vec![
        role_for("admin"),
        role_for("moderator"),
        role_for("user"),
    ]))
}

pub async fn get_admin_role(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> AppResult<Json<AdminRole>> {
    require_admin(&state, auth.account_id).await?;
    let role = match id.as_str() {
        "1" => role_for("admin"),
        "2" => role_for("moderator"),
        _ => role_for("user"),
    };
    Ok(Json(role))
}

// ── POST /api/v1/admin/measures ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct MeasuresRequest {
    pub keys: Vec<String>,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
}

pub async fn get_measures(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(body): Json<MeasuresRequest>,
) -> AppResult<Json<Vec<serde_json::Value>>> {
    require_admin(&state, auth.account_id).await?;

    let start: chrono::DateTime<chrono::Utc> = body.start_at.as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|| chrono::Utc::now() - chrono::Duration::days(7));
    let end: chrono::DateTime<chrono::Utc> = body.end_at.as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(chrono::Utc::now);
    let prev_start = start - (end - start);

    let mut result = Vec::new();

    for key in &body.keys {
        let measure = match key.as_str() {
            "new_users" => {
                let total = sqlx::query_scalar!(
                    "SELECT COUNT(*) FROM accounts WHERE domain IS NULL AND created_at BETWEEN $1 AND $2",
                    start, end,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                let previous_total = sqlx::query_scalar!(
                    "SELECT COUNT(*) FROM accounts WHERE domain IS NULL AND created_at BETWEEN $1 AND $2",
                    prev_start, start,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                let data = sqlx::query!(
                    r#"SELECT date_trunc('day', created_at)::timestamptz AS day, COUNT(*) AS n
                       FROM accounts WHERE domain IS NULL AND created_at BETWEEN $1 AND $2
                       GROUP BY day ORDER BY day"#,
                    start, end,
                ).fetch_all(&state.db).await?;
                serde_json::json!({
                    "key": key,
                    "unit": null,
                    "total": total.to_string(),
                    "human_value": total.to_string(),
                    "previous_total": previous_total.to_string(),
                    "data": data.iter().map(|r| serde_json::json!({
                        "date": r.day.map(super::convert::mastodon_date).unwrap_or_default(),
                        "value": r.n.unwrap_or(0).to_string(),
                    })).collect::<Vec<_>>(),
                })
            }
            "active_users" => {
                let total = sqlx::query_scalar!(
                    r#"SELECT COUNT(DISTINCT s.account_id) FROM statuses s
                       JOIN accounts a ON a.id = s.account_id
                       WHERE a.domain IS NULL AND s.created_at BETWEEN $1 AND $2 AND s.deleted_at IS NULL"#,
                    start, end,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                let previous_total = sqlx::query_scalar!(
                    r#"SELECT COUNT(DISTINCT s.account_id) FROM statuses s
                       JOIN accounts a ON a.id = s.account_id
                       WHERE a.domain IS NULL AND s.created_at BETWEEN $1 AND $2 AND s.deleted_at IS NULL"#,
                    prev_start, start,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                let data = sqlx::query!(
                    r#"SELECT date_trunc('day', s.created_at)::timestamptz AS day, COUNT(DISTINCT s.account_id) AS n
                       FROM statuses s JOIN accounts a ON a.id = s.account_id
                       WHERE a.domain IS NULL AND s.created_at BETWEEN $1 AND $2 AND s.deleted_at IS NULL
                       GROUP BY day ORDER BY day"#,
                    start, end,
                ).fetch_all(&state.db).await?;
                serde_json::json!({
                    "key": key,
                    "unit": null,
                    "total": total.to_string(),
                    "human_value": total.to_string(),
                    "previous_total": previous_total.to_string(),
                    "data": data.iter().map(|r| serde_json::json!({
                        "date": r.day.map(super::convert::mastodon_date).unwrap_or_default(),
                        "value": r.n.unwrap_or(0).to_string(),
                    })).collect::<Vec<_>>(),
                })
            }
            "new_statuses" => {
                let total = sqlx::query_scalar!(
                    r#"SELECT COUNT(*) FROM statuses s JOIN accounts a ON a.id = s.account_id
                       WHERE a.domain IS NULL AND s.created_at BETWEEN $1 AND $2 AND s.deleted_at IS NULL"#,
                    start, end,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                let previous_total = sqlx::query_scalar!(
                    r#"SELECT COUNT(*) FROM statuses s JOIN accounts a ON a.id = s.account_id
                       WHERE a.domain IS NULL AND s.created_at BETWEEN $1 AND $2 AND s.deleted_at IS NULL"#,
                    prev_start, start,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                let data = sqlx::query!(
                    r#"SELECT date_trunc('day', s.created_at)::timestamptz AS day, COUNT(*) AS n
                       FROM statuses s JOIN accounts a ON a.id = s.account_id
                       WHERE a.domain IS NULL AND s.created_at BETWEEN $1 AND $2 AND s.deleted_at IS NULL
                       GROUP BY day ORDER BY day"#,
                    start, end,
                ).fetch_all(&state.db).await?;
                serde_json::json!({
                    "key": key,
                    "unit": null,
                    "total": total.to_string(),
                    "human_value": total.to_string(),
                    "previous_total": previous_total.to_string(),
                    "data": data.iter().map(|r| serde_json::json!({
                        "date": r.day.map(super::convert::mastodon_date).unwrap_or_default(),
                        "value": r.n.unwrap_or(0).to_string(),
                    })).collect::<Vec<_>>(),
                })
            }
            "opened_reports" => {
                let total = sqlx::query_scalar!(
                    "SELECT COUNT(*) FROM reports r WHERE r.created_at BETWEEN $1 AND $2",
                    start, end,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                let previous_total = sqlx::query_scalar!(
                    "SELECT COUNT(*) FROM reports r WHERE r.created_at BETWEEN $1 AND $2",
                    prev_start, start,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                serde_json::json!({
                    "key": key, "unit": null,
                    "total": total.to_string(), "human_value": total.to_string(),
                    "previous_total": previous_total.to_string(), "data": [],
                })
            }
            "resolved_reports" => {
                let total = sqlx::query_scalar!(
                    "SELECT COUNT(*) FROM reports r WHERE r.action_taken_at BETWEEN $1 AND $2",
                    start, end,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                let previous_total = sqlx::query_scalar!(
                    "SELECT COUNT(*) FROM reports r WHERE r.action_taken_at BETWEEN $1 AND $2",
                    prev_start, start,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                serde_json::json!({
                    "key": key, "unit": null,
                    "total": total.to_string(), "human_value": total.to_string(),
                    "previous_total": previous_total.to_string(), "data": [],
                })
            }
            _ => serde_json::json!({
                "key": key, "unit": null, "total": "0",
                "human_value": "0", "previous_total": "0", "data": [],
            }),
        };
        result.push(measure);
    }

    Ok(Json(result))
}

// ── POST /api/v1/admin/dimensions ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DimensionsRequest {
    pub keys: Vec<String>,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub limit: Option<i64>,
}

pub async fn get_dimensions(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(body): Json<DimensionsRequest>,
) -> AppResult<Json<Vec<serde_json::Value>>> {
    require_admin(&state, auth.account_id).await?;

    let start: chrono::DateTime<chrono::Utc> = body.start_at.as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|| chrono::Utc::now() - chrono::Duration::days(7));
    let end: chrono::DateTime<chrono::Utc> = body.end_at.as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(chrono::Utc::now);
    let limit = body.limit.unwrap_or(10).min(50).max(1);

    let mut result = Vec::new();

    for key in &body.keys {
        let dimension = match key.as_str() {
            "servers" => {
                let rows = sqlx::query!(
                    r#"SELECT COALESCE(a.domain, 'local') AS server, COUNT(*) AS n
                       FROM statuses s JOIN accounts a ON a.id = s.account_id
                       WHERE s.created_at BETWEEN $1 AND $2 AND s.deleted_at IS NULL
                       GROUP BY server ORDER BY n DESC LIMIT $3"#,
                    start, end, limit,
                ).fetch_all(&state.db).await?;
                serde_json::json!({
                    "key": key,
                    "data": rows.iter().map(|r| {
                        let v = r.n.unwrap_or(0).to_string();
                        serde_json::json!({
                            "key": r.server,
                            "human_key": r.server,
                            "value": v,
                            "unit": null,
                            "human_value": v,
                        })
                    }).collect::<Vec<_>>(),
                })
            }
            "sources" => {
                // Statuses don't store the originating OAuth application — not trackable.
                serde_json::json!({"key": key, "data": []})
            }
            _ => serde_json::json!({"key": key, "data": []}),
        };
        result.push(dimension);
    }

    Ok(Json(result))
}

// ── POST /api/v1/admin/retention ─────────────────────────────────────────
//
// Returns cohort retention: for each signup period, how many users were still
// posting in each subsequent period.

#[derive(Debug, Deserialize)]
pub struct RetentionRequest {
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub frequency: Option<String>, // "day" or "week"
}

pub async fn get_retention(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(body): Json<RetentionRequest>,
) -> AppResult<Json<Vec<serde_json::Value>>> {
    require_admin(&state, auth.account_id).await?;

    let start: chrono::DateTime<chrono::Utc> = body.start_at.as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|| chrono::Utc::now() - chrono::Duration::days(30));
    let end: chrono::DateTime<chrono::Utc> = body.end_at.as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(chrono::Utc::now);
    // frequency must be a valid date_trunc unit: "day", "week", or "month"
    let frequency = match body.frequency.as_deref().unwrap_or("day") {
        "week" => "week",
        "month" => "month",
        _ => "day",
    };

    let cohort_rows = sqlx::query!(
        r#"SELECT
               date_trunc($2, a.created_at)::timestamptz AS period,
               a.id AS account_id
           FROM accounts a
           WHERE a.domain IS NULL
             AND a.created_at BETWEEN $1 AND $3
           ORDER BY period"#,
        start,
        frequency,
        end,
    )
    .fetch_all(&state.db)
    .await?;

    let mut cohorts: std::collections::BTreeMap<
        chrono::DateTime<chrono::Utc>,
        Vec<i64>,
    > = std::collections::BTreeMap::new();
    for row in cohort_rows {
        if let Some(period) = row.period {
            cohorts.entry(period).or_default().push(row.account_id);
        }
    }

    // Cap iterations to avoid very long loops
    let max_periods: usize = 100;

    let mut data = Vec::new();
    for (period, account_ids) in &cohorts {
        let cohort_size = account_ids.len() as i64;
        let mut retention_data = Vec::new();
        let mut check_period = *period;
        let mut count = 0;
        while check_period <= end && count < max_periods {
            let next_period = advance_period(check_period, frequency);
            let active_count = sqlx::query_scalar!(
                r#"SELECT COUNT(DISTINCT s.account_id)
                   FROM statuses s
                   WHERE s.account_id = ANY($1::bigint[])
                     AND s.deleted_at IS NULL
                     AND s.created_at >= $2
                     AND s.created_at < $3"#,
                account_ids,
                check_period,
                next_period,
            )
            .fetch_one(&state.db)
            .await?
            .unwrap_or(0);

            let rate = if cohort_size > 0 {
                active_count as f64 / cohort_size as f64
            } else {
                0.0
            };
            retention_data.push(serde_json::json!({
                "date": super::convert::mastodon_date(check_period),
                "rate": rate,
                "value": active_count,
            }));
            check_period = next_period;
            count += 1;
        }

        data.push(serde_json::json!({
            "period": super::convert::mastodon_date(*period),
            "frequency": frequency,
            "cohort_size": cohort_size,
            "data": retention_data,
        }));
    }

    Ok(Json(data))
}

fn advance_period(dt: chrono::DateTime<chrono::Utc>, frequency: &str) -> chrono::DateTime<chrono::Utc> {
    use chrono::Datelike;
    match frequency {
        "month" => {
            let (year, month) = if dt.month() == 12 {
                (dt.year() + 1, 1u32)
            } else {
                (dt.year(), dt.month() + 1)
            };
            dt.with_year(year).and_then(|d| d.with_month(month)).unwrap_or(dt + chrono::Duration::days(30))
        }
        "week" => dt + chrono::Duration::days(7),
        _ => dt + chrono::Duration::days(1),
    }
}

// ── Admin CustomEmoji type ────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AdminCustomEmoji {
    pub id: String,
    pub shortcode: String,
    pub url: String,
    pub static_url: String,
    pub visible_in_picker: bool,
    pub disabled: bool,
    pub category: Option<String>,
}

// ── GET /api/v1/admin/custom_emojis ──────────────────────────────────────

pub async fn list_admin_custom_emojis(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<AdminCustomEmoji>>> {
    require_admin(&state, auth.account_id).await?;

    let rows = sqlx::query!(
        "SELECT id, shortcode, image_remote_url, visible_in_picker, disabled
         FROM custom_emojis WHERE domain IS NULL ORDER BY shortcode",
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows.into_iter().map(|r| {
        let url = r.image_remote_url.unwrap_or_default();
        AdminCustomEmoji {
            id: r.id.to_string(),
            shortcode: r.shortcode,
            url: url.clone(),
            static_url: url,
            visible_in_picker: r.visible_in_picker,
            disabled: r.disabled,
            category: None,
        }
    }).collect()))
}

// ── POST /api/v1/admin/custom_emojis ─────────────────────────────────────

pub async fn create_admin_custom_emoji(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    mut multipart: Multipart,
) -> AppResult<Json<AdminCustomEmoji>> {
    require_admin(&state, auth.account_id).await?;

    let mut shortcode = String::new();
    let mut image_bytes: Option<Vec<u8>> = None;
    let mut content_type = "image/png".to_string();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "shortcode" => {
                shortcode = field.text().await.unwrap_or_default();
            }
            "image" => {
                content_type = field.content_type().unwrap_or("image/png").to_string();
                image_bytes = field.bytes().await.ok().map(|b| b.to_vec());
            }
            _ => {}
        }
    }

    if shortcode.is_empty() {
        return Err(AppError::Unprocessable("shortcode is required".into()));
    }
    let image_data = image_bytes.ok_or_else(|| AppError::Unprocessable("image is required".into()))?;

    // Upload to storage
    let ext = match content_type.as_str() {
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "png",
    };
    let key = format!("emoji/{}.{}", shortcode, ext);
    state.storage.store(&image_data, &key, &content_type).await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("storage: {e}")))?;
    let url = state.storage.public_url(&key);

    let row = sqlx::query!(
        r#"INSERT INTO custom_emojis (shortcode, image_remote_url, visible_in_picker)
           VALUES ($1, $2, true)
           ON CONFLICT (shortcode) WHERE domain IS NULL
           DO UPDATE SET image_remote_url = $2, disabled = false
           RETURNING id, shortcode, image_remote_url, visible_in_picker, disabled"#,
        shortcode, url,
    )
    .fetch_one(&state.db)
    .await?;

    let url = row.image_remote_url.unwrap_or_default();
    Ok(Json(AdminCustomEmoji {
        id: row.id.to_string(),
        shortcode: row.shortcode,
        url: url.clone(),
        static_url: url,
        visible_in_picker: row.visible_in_picker,
        disabled: row.disabled,
        category: None,
    }))
}

// ── DELETE /api/v1/admin/custom_emojis/:id ───────────────────────────────

pub async fn delete_admin_custom_emoji(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!(
        "DELETE FROM custom_emojis WHERE id = $1",
        id,
    )
    .execute(&state.db)
    .await?;
    Ok(StatusCode::OK)
}

// ── PATCH /api/v1/admin/custom_emojis/:id ────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PatchEmojiForm {
    pub shortcode: Option<String>,
    pub visible_in_picker: Option<bool>,
    pub disabled: Option<bool>,
}

pub async fn update_admin_custom_emoji(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
    Json(form): Json<PatchEmojiForm>,
) -> AppResult<Json<AdminCustomEmoji>> {
    require_admin(&state, auth.account_id).await?;
    if let Some(sc) = &form.shortcode {
        sqlx::query!("UPDATE custom_emojis SET shortcode = $1 WHERE id = $2", sc, id)
            .execute(&state.db).await?;
    }
    if let Some(v) = form.visible_in_picker {
        sqlx::query!("UPDATE custom_emojis SET visible_in_picker = $1 WHERE id = $2", v, id)
            .execute(&state.db).await?;
    }
    if let Some(d) = form.disabled {
        sqlx::query!("UPDATE custom_emojis SET disabled = $1 WHERE id = $2", d, id)
            .execute(&state.db).await?;
    }
    let row = sqlx::query!(
        "SELECT id, shortcode, image_remote_url, visible_in_picker, disabled FROM custom_emojis WHERE id = $1",
        id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    let url = row.image_remote_url.unwrap_or_default();
    Ok(Json(AdminCustomEmoji {
        id: row.id.to_string(),
        shortcode: row.shortcode,
        url: url.clone(),
        static_url: url,
        visible_in_picker: row.visible_in_picker,
        disabled: row.disabled,
        category: None,
    }))
}

// ── Admin DomainBlock / DomainAllow types ─────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AdminDomainBlock {
    pub id: String,
    pub domain: String,
    pub digest: String,
    pub created_at: String,
    pub severity: String,
    pub reject_media: bool,
    pub reject_reports: bool,
    pub private_comment: Option<String>,
    pub public_comment: Option<String>,
    pub obfuscate: bool,
}

#[derive(Debug, Serialize)]
pub struct AdminDomainAllow {
    pub id: String,
    pub domain: String,
    pub created_at: String,
}

// ── GET /api/v1/admin/domain_blocks ──────────────────────────────────────

pub async fn list_domain_blocks(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<AdminDomainBlock>>> {
    require_admin(&state, auth.account_id).await?;
    let rows = sqlx::query!(
        r#"SELECT id, domain, reject_media, reject_reports, private_comment, public_comment, obfuscate, created_at,
                  CASE severity WHEN 0 THEN 'noop' WHEN 1 THEN 'silence' WHEN 2 THEN 'suspend' ELSE 'silence' END AS "severity!"
           FROM domain_blocks ORDER BY domain"#,
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(|r| AdminDomainBlock {
        id: r.id.to_string(),
        digest: sha256_hex(&r.domain),
        domain: r.domain,
        created_at: super::convert::mastodon_date(r.created_at),
        severity: r.severity,
        reject_media: r.reject_media,
        reject_reports: r.reject_reports,
        private_comment: r.private_comment,
        public_comment: r.public_comment,
        obfuscate: r.obfuscate,
    }).collect()))
}

// ── POST /api/v1/admin/domain_blocks ─────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateDomainBlockForm {
    pub domain: String,
    pub severity: Option<String>,
    pub reject_media: Option<bool>,
    pub reject_reports: Option<bool>,
    pub private_comment: Option<String>,
    pub public_comment: Option<String>,
    pub obfuscate: Option<bool>,
}

pub async fn create_domain_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<CreateDomainBlockForm>,
) -> AppResult<Json<AdminDomainBlock>> {
    require_admin(&state, auth.account_id).await?;
    let severity = crate::db::models::domain_severity::from_str(form.severity.as_deref().unwrap_or("silence"));
    let row = sqlx::query!(
        r#"INSERT INTO domain_blocks (domain, severity, reject_media, reject_reports, private_comment, public_comment, obfuscate)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           ON CONFLICT (domain) DO UPDATE SET severity = $2, reject_media = $3, reject_reports = $4,
             private_comment = $5, public_comment = $6, obfuscate = $7, updated_at = now()
           RETURNING id, domain, reject_media, reject_reports, private_comment, public_comment, obfuscate, created_at,
                     CASE severity WHEN 0 THEN 'noop' WHEN 1 THEN 'silence' WHEN 2 THEN 'suspend' ELSE 'silence' END AS "severity!""#,
        form.domain, severity,
        form.reject_media.unwrap_or(false),
        form.reject_reports.unwrap_or(false),
        form.private_comment,
        form.public_comment,
        form.obfuscate.unwrap_or(false),
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(AdminDomainBlock {
        id: row.id.to_string(),
        digest: sha256_hex(&row.domain),
        domain: row.domain,
        created_at: super::convert::mastodon_date(row.created_at),
        severity: row.severity,
        reject_media: row.reject_media,
        reject_reports: row.reject_reports,
        private_comment: row.private_comment,
        public_comment: row.public_comment,
        obfuscate: row.obfuscate,
    }))
}

// ── GET /api/v1/admin/domain_blocks/:id ──────────────────────────────────

pub async fn get_admin_domain_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminDomainBlock>> {
    require_admin(&state, auth.account_id).await?;
    let r = sqlx::query!(
        r#"SELECT id, domain, reject_media, reject_reports, private_comment, public_comment, obfuscate, created_at,
                  CASE severity WHEN 0 THEN 'noop' WHEN 1 THEN 'silence' WHEN 2 THEN 'suspend' ELSE 'silence' END AS "severity!"
           FROM domain_blocks WHERE id = $1"#,
        id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(AdminDomainBlock {
        id: r.id.to_string(),
        digest: sha256_hex(&r.domain),
        domain: r.domain,
        created_at: super::convert::mastodon_date(r.created_at),
        severity: r.severity,
        reject_media: r.reject_media,
        reject_reports: r.reject_reports,
        private_comment: r.private_comment,
        public_comment: r.public_comment,
        obfuscate: r.obfuscate,
    }))
}

// ── PATCH /api/v1/admin/domain_blocks/:id ────────────────────────────────

pub async fn update_admin_domain_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
    Json(form): Json<CreateDomainBlockForm>,
) -> AppResult<Json<AdminDomainBlock>> {
    require_admin(&state, auth.account_id).await?;
    let severity_int: Option<i32> = form.severity.as_deref().map(crate::db::models::domain_severity::from_str);
    let r = sqlx::query!(
        r#"UPDATE domain_blocks SET
               severity       = COALESCE($2, severity),
               reject_media   = COALESCE($3, reject_media),
               reject_reports = COALESCE($4, reject_reports),
               private_comment = $5,
               public_comment  = $6,
               obfuscate      = COALESCE($7, obfuscate),
               updated_at     = now()
           WHERE id = $1
           RETURNING id, domain, reject_media, reject_reports, private_comment, public_comment, obfuscate, created_at,
                     CASE severity WHEN 0 THEN 'noop' WHEN 1 THEN 'silence' WHEN 2 THEN 'suspend' ELSE 'silence' END AS "severity!""#,
        id,
        severity_int,
        form.reject_media,
        form.reject_reports,
        form.private_comment,
        form.public_comment,
        form.obfuscate,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(AdminDomainBlock {
        id: r.id.to_string(),
        digest: sha256_hex(&r.domain),
        domain: r.domain,
        created_at: super::convert::mastodon_date(r.created_at),
        severity: r.severity,
        reject_media: r.reject_media,
        reject_reports: r.reject_reports,
        private_comment: r.private_comment,
        public_comment: r.public_comment,
        obfuscate: r.obfuscate,
    }))
}

// ── DELETE /api/v1/admin/domain_blocks/:id ───────────────────────────────

pub async fn delete_domain_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!("DELETE FROM domain_blocks WHERE id = $1", id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::OK)
}

// ── GET /api/v1/admin/domain_allows ──────────────────────────────────────

pub async fn list_domain_allows(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<AdminDomainAllow>>> {
    require_admin(&state, auth.account_id).await?;
    let rows = sqlx::query!(
        "SELECT id, domain, created_at FROM domain_allows ORDER BY domain",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(|r| AdminDomainAllow {
        id: r.id.to_string(),
        domain: r.domain,
        created_at: super::convert::mastodon_date(r.created_at),
    }).collect()))
}

// ── POST /api/v1/admin/domain_allows ─────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateDomainAllowForm {
    pub domain: String,
}

pub async fn create_domain_allow(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<CreateDomainAllowForm>,
) -> AppResult<Json<AdminDomainAllow>> {
    require_admin(&state, auth.account_id).await?;
    let row = sqlx::query!(
        r#"INSERT INTO domain_allows (domain) VALUES ($1)
           ON CONFLICT (domain) DO UPDATE SET updated_at = now()
           RETURNING id, domain, created_at"#,
        form.domain,
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(AdminDomainAllow {
        id: row.id.to_string(),
        domain: row.domain,
        created_at: super::convert::mastodon_date(row.created_at),
    }))
}

// ── DELETE /api/v1/admin/domain_allows/:id ───────────────────────────────

pub async fn delete_domain_allow(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!("DELETE FROM domain_allows WHERE id = $1", id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::OK)
}

// ── Admin IP blocks ───────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AdminIpBlock {
    pub id: String,
    pub ip: String,
    pub severity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateIpBlockForm {
    pub ip: String,
    pub severity: Option<String>,
    pub comment: Option<String>,
    pub expires_in: Option<i64>,
}

pub async fn list_ip_blocks(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<AdminIpBlock>>> {
    require_admin(&state, auth.account_id).await?;
    let rows = sqlx::query!(
        r#"SELECT id, host(ip) as "ip!", comment, expires_at, created_at,
                  CASE severity WHEN 0 THEN 'noop' WHEN 1 THEN 'sign_up_requires_approval' WHEN 2 THEN 'sign_up_block' WHEN 3 THEN 'block' ELSE 'noop' END AS "severity!"
           FROM ip_blocks ORDER BY created_at DESC"#
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(|r| AdminIpBlock {
        id: r.id.to_string(),
        ip: r.ip,
        severity: r.severity,
        comment: Some(r.comment),
        expires_at: r.expires_at.map(super::convert::mastodon_date),
        created_at: super::convert::mastodon_date(r.created_at),
    }).collect()))
}

pub async fn get_ip_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminIpBlock>> {
    require_admin(&state, auth.account_id).await?;
    let r = sqlx::query!(
        r#"SELECT id, host(ip) as "ip!", comment, expires_at, created_at,
                  CASE severity WHEN 0 THEN 'noop' WHEN 1 THEN 'sign_up_requires_approval' WHEN 2 THEN 'sign_up_block' WHEN 3 THEN 'block' ELSE 'noop' END AS "severity!"
           FROM ip_blocks WHERE id = $1"#,
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(AdminIpBlock {
        id: r.id.to_string(),
        ip: r.ip,
        severity: r.severity,
        comment: Some(r.comment),
        expires_at: r.expires_at.map(super::convert::mastodon_date),
        created_at: super::convert::mastodon_date(r.created_at),
    }))
}

pub async fn create_ip_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<CreateIpBlockForm>,
) -> AppResult<Json<AdminIpBlock>> {
    require_admin(&state, auth.account_id).await?;
    let severity = crate::db::models::ip_severity::from_str(form.severity.as_deref().unwrap_or("sign_up_block"));
    let expires_at = form.expires_in
        .map(|secs| chrono::Utc::now() + chrono::Duration::seconds(secs));
    let r = sqlx::query!(
        r#"INSERT INTO ip_blocks (ip, severity, comment, expires_at)
           VALUES ($1::text::inet, $2, $3, $4)
           ON CONFLICT (ip) DO UPDATE SET severity = $2, comment = $3, expires_at = $4, updated_at = now()
           RETURNING id, host(ip) as "ip!", comment, expires_at, created_at,
                     CASE severity WHEN 0 THEN 'noop' WHEN 1 THEN 'sign_up_requires_approval' WHEN 2 THEN 'sign_up_block' WHEN 3 THEN 'block' ELSE 'noop' END AS "severity!""#,
        form.ip, severity, form.comment.unwrap_or_default(), expires_at,
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(AdminIpBlock {
        id: r.id.to_string(),
        ip: r.ip,
        severity: r.severity,
        comment: Some(r.comment),
        expires_at: r.expires_at.map(super::convert::mastodon_date),
        created_at: super::convert::mastodon_date(r.created_at),
    }))
}

pub async fn update_ip_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
    Json(form): Json<CreateIpBlockForm>,
) -> AppResult<Json<AdminIpBlock>> {
    require_admin(&state, auth.account_id).await?;
    let severity = crate::db::models::ip_severity::from_str(form.severity.as_deref().unwrap_or("sign_up_block"));
    let expires_at = form.expires_in
        .map(|secs| chrono::Utc::now() + chrono::Duration::seconds(secs));
    let r = sqlx::query!(
        r#"UPDATE ip_blocks SET severity = $2, comment = $3, expires_at = $4, updated_at = now()
           WHERE id = $1
           RETURNING id, host(ip) as "ip!", comment, expires_at, created_at,
                     CASE severity WHEN 0 THEN 'noop' WHEN 1 THEN 'sign_up_requires_approval' WHEN 2 THEN 'sign_up_block' WHEN 3 THEN 'block' ELSE 'noop' END AS "severity!""#,
        id, severity, form.comment.unwrap_or_default(), expires_at,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(AdminIpBlock {
        id: r.id.to_string(),
        ip: r.ip,
        severity: r.severity,
        comment: Some(r.comment),
        expires_at: r.expires_at.map(super::convert::mastodon_date),
        created_at: super::convert::mastodon_date(r.created_at),
    }))
}

pub async fn delete_ip_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!("DELETE FROM ip_blocks WHERE id = $1", id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::OK)
}

// ── Admin Email Domain blocks ─────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AdminEmailDomainBlock {
    pub id: String,
    pub domain: String,
    pub created_at: String,
    pub history: Vec<AdminEmailDomainBlockHistory>,
    pub allow_with_approval: bool,
}

#[derive(Debug, Serialize)]
pub struct AdminEmailDomainBlockHistory {
    pub day: String,
    pub accounts: String,
    pub uses: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateEmailDomainBlockForm {
    pub domain: String,
}

pub async fn list_email_domain_blocks(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<AdminEmailDomainBlock>>> {
    require_admin(&state, auth.account_id).await?;
    let rows = sqlx::query!(
        "SELECT id, domain, created_at, allow_with_approval FROM email_domain_blocks ORDER BY domain"
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(|r| AdminEmailDomainBlock {
        id: r.id.to_string(),
        domain: r.domain,
        created_at: super::convert::mastodon_date(r.created_at),
        history: vec![],
        allow_with_approval: r.allow_with_approval,
    }).collect()))
}

pub async fn get_email_domain_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminEmailDomainBlock>> {
    require_admin(&state, auth.account_id).await?;
    let r = sqlx::query!(
        "SELECT id, domain, created_at, allow_with_approval FROM email_domain_blocks WHERE id = $1",
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(AdminEmailDomainBlock {
        id: r.id.to_string(),
        domain: r.domain,
        created_at: super::convert::mastodon_date(r.created_at),
        history: vec![],
        allow_with_approval: r.allow_with_approval,
    }))
}

pub async fn create_email_domain_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<CreateEmailDomainBlockForm>,
) -> AppResult<Json<AdminEmailDomainBlock>> {
    require_admin(&state, auth.account_id).await?;
    let r = sqlx::query!(
        r#"INSERT INTO email_domain_blocks (domain) VALUES ($1)
           ON CONFLICT (domain) DO UPDATE SET updated_at = now()
           RETURNING id, domain, created_at, allow_with_approval"#,
        form.domain,
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(AdminEmailDomainBlock {
        id: r.id.to_string(),
        domain: r.domain,
        created_at: super::convert::mastodon_date(r.created_at),
        history: vec![],
        allow_with_approval: r.allow_with_approval,
    }))
}

pub async fn delete_email_domain_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!("DELETE FROM email_domain_blocks WHERE id = $1", id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::OK)
}

// ── POST /api/v1/admin/reports/:id/assign_to_self ────────────────────────

pub async fn assign_report_to_self(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminReport>> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!(
        "UPDATE reports SET assigned_account_id = $1 WHERE id = $2",
        auth.account_id, id,
    )
    .execute(&state.db)
    .await?;
    get_admin_report(State(state), Extension(auth), Path(id)).await
}

// ── POST /api/v1/admin/reports/:id/unassign ──────────────────────────────

pub async fn unassign_report(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminReport>> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!(
        "UPDATE reports SET assigned_account_id = NULL WHERE id = $1",
        id,
    )
    .execute(&state.db)
    .await?;
    get_admin_report(State(state), Extension(auth), Path(id)).await
}

// ── POST /api/v1/admin/accounts/:id/sensitive ────────────────────────────

pub async fn sensitive_account(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminAccount>> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!(
        "UPDATE accounts SET sensitized_at = now() WHERE id = $1 AND sensitized_at IS NULL",
        id,
    )
    .execute(&state.db)
    .await?;
    let account = sqlx::query_as!(models::Account, "SELECT * FROM accounts WHERE id = $1", id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(build_admin_account(&state, &account).await?))
}

// ── POST /api/v1/admin/accounts/:id/unsensitive ──────────────────────────

pub async fn unsensitive_account(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminAccount>> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!(
        "UPDATE accounts SET sensitized_at = NULL WHERE id = $1",
        id,
    )
    .execute(&state.db)
    .await?;
    let account = sqlx::query_as!(models::Account, "SELECT * FROM accounts WHERE id = $1", id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(build_admin_account(&state, &account).await?))
}

// ── POST /api/v1/admin/accounts/:id/action ───────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AccountActionForm {
    #[serde(rename = "type")]
    pub action_type: Option<String>,
    pub text: Option<String>,
    pub report_id: Option<String>,
    pub warning_preset_id: Option<String>,
    pub send_email_notification: Option<bool>,
}

pub async fn account_action(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
    Json(form): Json<AccountActionForm>,
) -> AppResult<StatusCode> {
    require_admin(&state, auth.account_id).await?;

    match form.action_type.as_deref().unwrap_or("none") {
        "disable" => {
            sqlx::query!(
                "UPDATE accounts SET suspended_at = now() WHERE id = $1 AND suspended_at IS NULL",
                id,
            )
            .execute(&state.db)
            .await?;
        }
        "sensitive" => {
            sqlx::query!(
                "UPDATE accounts SET sensitized_at = now() WHERE id = $1 AND sensitized_at IS NULL",
                id,
            )
            .execute(&state.db)
            .await?;
        }
        "silence" => {
            sqlx::query!(
                "UPDATE accounts SET silenced_at = now() WHERE id = $1 AND silenced_at IS NULL",
                id,
            )
            .execute(&state.db)
            .await?;
        }
        "suspend" => {
            sqlx::query!(
                "UPDATE accounts SET suspended_at = now() WHERE id = $1 AND suspended_at IS NULL",
                id,
            )
            .execute(&state.db)
            .await?;
            sqlx::query!(
                "UPDATE statuses SET deleted_at = now() WHERE account_id = $1 AND deleted_at IS NULL",
                id,
            )
            .execute(&state.db)
            .await?;
        }
        _ => {}
    }

    if let Some(report_id_str) = &form.report_id {
        if let Ok(report_id) = report_id_str.parse::<i64>() {
            sqlx::query!(
                "UPDATE reports SET action_taken_at = now(), action_taken_by_account_id = $1 WHERE id = $2",
                auth.account_id, report_id,
            )
            .execute(&state.db)
            .await?;
        }
    }

    Ok(StatusCode::OK)
}

// ── DELETE /api/v1/admin/accounts/:id ────────────────────────────────────

pub async fn delete_admin_account(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    require_admin(&state, auth.account_id).await?;

    let mut tx = state.db.begin().await?;
    sqlx::query!(
        "UPDATE statuses SET deleted_at = now() WHERE account_id = $1 AND deleted_at IS NULL",
        id,
    ).execute(&mut *tx).await?;
    sqlx::query!(
        "UPDATE oauth_access_tokens SET revoked_at = now() WHERE account_id = $1 AND revoked_at IS NULL",
        id,
    ).execute(&mut *tx).await?;
    sqlx::query!(
        "UPDATE accounts SET suspended_at = now() WHERE id = $1",
        id,
    ).execute(&mut *tx).await?;
    sqlx::query!(
        "DELETE FROM users WHERE account_id = $1",
        id,
    ).execute(&mut *tx).await?;
    tx.commit().await?;

    Ok(StatusCode::OK)
}

// ── GET /api/v1/admin/trends/* ────────────────────────────────────────────

pub async fn admin_trending_tags(
    state: State<AppState>,
    instance: axum::extract::Extension<crate::middleware::ResolvedInstance>,
    query: axum::extract::Query<super::trends::TrendParams>,
    auth: axum::extract::Extension<AuthenticatedUser>,
) -> AppResult<axum::Json<Vec<super::types::Tag>>> {
    require_admin(&state, auth.account_id).await?;
    super::trends::trending_tags(state, instance, query, None).await
}

pub async fn admin_trending_statuses(
    state: State<AppState>,
    query: axum::extract::Query<super::trends::TrendParams>,
    auth: axum::extract::Extension<AuthenticatedUser>,
) -> AppResult<axum::Json<Vec<super::types::Status>>> {
    require_admin(&state, auth.account_id).await?;
    super::trends::trending_statuses(state, query, Some(axum::extract::Extension(crate::middleware::AuthenticatedUser { account_id: auth.account_id, user_id: auth.user_id, token_id: auth.token_id, scopes: auth.scopes.clone(), application_id: auth.application_id }))).await
}

pub async fn admin_trending_links(
    state: State<AppState>,
    query: axum::extract::Query<super::trends::TrendParams>,
    auth: axum::extract::Extension<AuthenticatedUser>,
) -> AppResult<axum::Json<Vec<super::types::PreviewCard>>> {
    require_admin(&state, auth.account_id).await?;
    super::trends::trending_links(state, query).await
}

// ── Admin Trends Approve / Reject (stubs — eunha computes trends dynamically) ──

pub async fn admin_approve_trending_tag(
    Extension(auth): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Path(_id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    require_admin(&state, auth.account_id).await?;
    Ok(Json(serde_json::json!({})))
}

pub async fn admin_reject_trending_tag(
    Extension(auth): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Path(_id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    require_admin(&state, auth.account_id).await?;
    Ok(Json(serde_json::json!({})))
}

pub async fn admin_approve_trending_status(
    Extension(auth): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Path(_id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    require_admin(&state, auth.account_id).await?;
    Ok(Json(serde_json::json!({})))
}

pub async fn admin_reject_trending_status(
    Extension(auth): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Path(_id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    require_admin(&state, auth.account_id).await?;
    Ok(Json(serde_json::json!({})))
}

pub async fn admin_approve_trending_link(
    Extension(auth): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Path(_id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    require_admin(&state, auth.account_id).await?;
    Ok(Json(serde_json::json!({})))
}

pub async fn admin_reject_trending_link(
    Extension(auth): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Path(_id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    require_admin(&state, auth.account_id).await?;
    Ok(Json(serde_json::json!({})))
}

// ── Admin Canonical Email Blocks ──────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CanonicalEmailBlock {
    pub id: String,
    pub canonical_email_hash: String,
    pub created_at: String,
}

pub async fn list_canonical_email_blocks(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<CanonicalEmailBlock>>> {
    require_admin(&state, auth.account_id).await?;
    let rows = sqlx::query!(
        "SELECT id, canonical_email_hash, created_at FROM canonical_email_blocks ORDER BY id DESC LIMIT 100",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(|r| CanonicalEmailBlock {
        id: r.id.to_string(),
        canonical_email_hash: r.canonical_email_hash,
        created_at: super::convert::mastodon_date(r.created_at),
    }).collect()))
}

#[derive(Debug, Deserialize)]
pub struct CreateCanonicalEmailBlockForm {
    pub email: Option<String>,
    pub canonical_email_hash: Option<String>,
}

pub async fn create_canonical_email_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<CreateCanonicalEmailBlockForm>,
) -> AppResult<Json<CanonicalEmailBlock>> {
    require_admin(&state, auth.account_id).await?;
    let hash = if let Some(h) = form.canonical_email_hash {
        h
    } else if let Some(email) = form.email {
        let normalized = email.trim().to_lowercase();
        sha256_hex(&normalized)
    } else {
        return Err(AppError::Unprocessable("email or canonical_email_hash required".into()));
    };
    let row = sqlx::query!(
        "INSERT INTO canonical_email_blocks (canonical_email_hash) VALUES ($1) ON CONFLICT (canonical_email_hash) DO UPDATE SET canonical_email_hash = EXCLUDED.canonical_email_hash RETURNING id, canonical_email_hash, created_at",
        hash,
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(CanonicalEmailBlock {
        id: row.id.to_string(),
        canonical_email_hash: row.canonical_email_hash,
        created_at: super::convert::mastodon_date(row.created_at),
    }))
}

pub async fn get_canonical_email_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<CanonicalEmailBlock>> {
    require_admin(&state, auth.account_id).await?;
    let row = sqlx::query!(
        "SELECT id, canonical_email_hash, created_at FROM canonical_email_blocks WHERE id = $1",
        id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(CanonicalEmailBlock {
        id: row.id.to_string(),
        canonical_email_hash: row.canonical_email_hash,
        created_at: super::convert::mastodon_date(row.created_at),
    }))
}

pub async fn delete_canonical_email_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!("DELETE FROM canonical_email_blocks WHERE id = $1", id)
        .execute(&state.db)
        .await?;
    Ok(Json(serde_json::json!({})))
}

pub async fn test_canonical_email_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<CreateCanonicalEmailBlockForm>,
) -> AppResult<Json<Vec<CanonicalEmailBlock>>> {
    require_admin(&state, auth.account_id).await?;
    let hash = if let Some(h) = form.canonical_email_hash {
        h
    } else if let Some(email) = form.email {
        let normalized = email.trim().to_lowercase();
        sha256_hex(&normalized)
    } else {
        return Err(AppError::Unprocessable("email or canonical_email_hash required".into()));
    };
    let rows = sqlx::query!(
        "SELECT id, canonical_email_hash, created_at FROM canonical_email_blocks WHERE canonical_email_hash = $1",
        hash,
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(|r| CanonicalEmailBlock {
        id: r.id.to_string(),
        canonical_email_hash: r.canonical_email_hash,
        created_at: super::convert::mastodon_date(r.created_at),
    }).collect()))
}

// ── Admin Tags ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AdminTag {
    pub id: String,
    pub name: String,
    pub url: String,
    pub trendable: bool,
    pub usable: bool,
    pub requires_review: bool,
    pub listable: bool,
}

fn admin_tag_url(domain: &str, name: &str) -> String {
    format!("https://{domain}/tags/{name}")
}

#[derive(Debug, Deserialize)]
pub struct UpdateAdminTagForm {
    pub trendable: Option<bool>,
    pub usable: Option<bool>,
    pub listable: Option<bool>,
}

#[derive(serde::Deserialize)]
pub struct AdminTagsParams {
    #[serde(flatten)]
    pub pagination: super::types::PaginationParams,
    pub name: Option<String>,
}

pub async fn list_admin_tags(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Query(params): Query<AdminTagsParams>,
) -> AppResult<Json<Vec<AdminTag>>> {
    require_admin(&state, auth.account_id).await?;
    let domain = &instance.domain;
    let limit = params.pagination.limit_clamped(100, 100);
    let max_id = params.pagination.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = params.pagination.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = params.pagination.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let name_filter = params.name.as_deref().map(|s| s.to_lowercase());

    let rows = sqlx::query!(
        r#"SELECT id, name, trendable, usable, listable, reviewed_at
           FROM tags
           WHERE ($2::bigint IS NULL OR id < $2)
             AND ($3::bigint IS NULL OR id > $3)
             AND ($4::bigint IS NULL OR id > $4)
             AND ($5::text IS NULL OR name = $5)
           ORDER BY id DESC
           LIMIT $1"#,
        limit,
        max_id,
        since_id,
        min_id,
        name_filter,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows.into_iter().map(|r| AdminTag {
        id: r.id.to_string(),
        name: r.name.clone(),
        url: admin_tag_url(domain, &r.name),
        trendable: r.trendable.unwrap_or(false),
        usable: r.usable.unwrap_or(true),
        listable: r.listable.unwrap_or(true),
        requires_review: r.reviewed_at.is_none(),
    }).collect()))
}

pub async fn get_admin_tag(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminTag>> {
    require_admin(&state, auth.account_id).await?;
    let domain = &instance.domain;
    let r = sqlx::query!(
        "SELECT id, name, trendable, usable, listable, reviewed_at FROM tags WHERE id = $1",
        id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(AdminTag {
        id: r.id.to_string(),
        name: r.name.clone(),
        url: admin_tag_url(domain, &r.name),
        trendable: r.trendable.unwrap_or(false),
        usable: r.usable.unwrap_or(true),
        listable: r.listable.unwrap_or(true),
        requires_review: r.reviewed_at.is_none(),
    }))
}

pub async fn update_admin_tag(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Path(id): Path<i64>,
    Json(form): Json<UpdateAdminTagForm>,
) -> AppResult<Json<AdminTag>> {
    require_admin(&state, auth.account_id).await?;
    let domain = &instance.domain;
    let r = sqlx::query!(
        r#"UPDATE tags SET
               trendable   = COALESCE($2, trendable),
               usable      = COALESCE($3, usable),
               listable    = COALESCE($4, listable),
               reviewed_at = now(),
               updated_at  = now()
           WHERE id = $1
           RETURNING id, name, trendable, usable, listable, reviewed_at"#,
        id,
        form.trendable,
        form.usable,
        form.listable,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(AdminTag {
        id: r.id.to_string(),
        name: r.name.clone(),
        url: admin_tag_url(domain, &r.name),
        trendable: r.trendable.unwrap_or(false),
        usable: r.usable.unwrap_or(true),
        listable: r.listable.unwrap_or(true),
        requires_review: r.reviewed_at.is_none(),
    }))
}

fn sha256_hex(s: &str) -> String {
    use sha2::{Sha256, Digest};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex::encode(h.finalize())
}
