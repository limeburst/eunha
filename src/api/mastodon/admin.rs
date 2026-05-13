use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    db::models,
    error::{AppError, AppResult},
    middleware::AuthenticatedUser,
    state::AppState,
};
use super::convert::account_from_db;
use super::types::Account as ApiAccount;

// ── Admin auth guard ──────────────────────────────────────────────────────

async fn require_admin(state: &AppState, account_id: Uuid) -> AppResult<()> {
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
    pub role: AdminRole,
    pub confirmed: bool,
    pub suspended: bool,
    pub silenced: bool,
    pub sensitized: bool,
    pub disabled: bool,
    pub approved: bool,
    pub locale: Option<String>,
    pub invite_request: Option<String>,
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
            position: 10,
            permissions: 1048575,
            highlighted: true,
            created_at: "2022-09-08T22:48:07.983Z".into(),
            updated_at: "2022-09-08T22:48:07.983Z".into(),
        },
        "moderator" => AdminRole {
            id: "2".into(),
            name: "Moderator".into(),
            color: "#6364ff".into(),
            position: 5,
            permissions: 65536,
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
        "SELECT email, confirmed_at, approved_at, reason, role FROM users WHERE account_id = $1",
        account.id,
    )
    .fetch_optional(&state.db)
    .await?;

    let (email, confirmed, approved, reason, role_str) = match user {
        Some(u) => (
            u.email,
            u.confirmed_at.is_some(),
            u.approved_at.is_some(),
            u.reason,
            u.role,
        ),
        None => (String::new(), true, true, None, "user".to_string()),
    };

    Ok(AdminAccount {
        id: account.id.to_string(),
        username: account.username.clone(),
        domain: account.domain.clone(),
        created_at: account.created_at.to_rfc3339(),
        email,
        ip: None,
        role: role_for(&role_str),
        confirmed,
        suspended: account.suspended_at.is_some(),
        silenced: account.silenced_at.is_some(),
        sensitized: false,
        disabled: false,
        approved,
        locale: None,
        invite_request: reason,
        account: account_from_db(account),
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
) -> AppResult<Json<Vec<AdminAccount>>> {
    require_admin(&state, auth.account_id).await?;

    let instance_id = sqlx::query_scalar!(
        "SELECT instance_id FROM accounts WHERE id = $1",
        auth.account_id,
    )
    .fetch_one(&state.db)
    .await?;

    let limit = params.limit.unwrap_or(40).min(80).max(1);

    let accounts = sqlx::query_as!(
        models::Account,
        r#"SELECT a.* FROM accounts a
           WHERE a.instance_id = $1
             AND a.domain IS NULL
           ORDER BY a.created_at DESC
           LIMIT $2"#,
        instance_id, limit,
    )
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::with_capacity(accounts.len());
    for a in &accounts {
        result.push(build_admin_account(&state, a).await?);
    }
    Ok(Json(result))
}

// ── GET /api/v1/admin/accounts/:id ───────────────────────────────────────

pub async fn get_admin_account(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<Uuid>,
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
    Path(id): Path<Uuid>,
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
    Path(id): Path<Uuid>,
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
    Path(id): Path<Uuid>,
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
    Path(id): Path<Uuid>,
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
    Path(id): Path<Uuid>,
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
    Path(id): Path<Uuid>,
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
    Path(id): Path<Uuid>,
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
    pub status_ids: Vec<String>,
    pub rules_violated: Vec<serde_json::Value>,
    pub statuses: Vec<serde_json::Value>,
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

    Ok(AdminReport {
        id: report.id.to_string(),
        action_taken: report.action_taken_at.is_some(),
        action_taken_at: report.action_taken_at.map(|t| t.to_rfc3339()),
        category: report.category.clone(),
        comment: report.comment.clone(),
        forwarded: report.forwarded.unwrap_or(false),
        created_at: report.created_at.to_rfc3339(),
        updated_at: report.updated_at.to_rfc3339(),
        account: account_from_db(&account),
        target_account: account_from_db(&target),
        status_ids: report.status_ids.iter().map(|id| id.to_string()).collect(),
        rules_violated: vec![],
        statuses: vec![],
    })
}

struct AdminReportRow {
    id: i64,
    account_id: Uuid,
    target_account_id: Uuid,
    status_ids: Vec<i64>,
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
}

pub async fn list_admin_reports(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Query(params): Query<AdminReportsParams>,
) -> AppResult<Json<Vec<AdminReport>>> {
    require_admin(&state, auth.account_id).await?;

    let instance_id = sqlx::query_scalar!(
        "SELECT instance_id FROM accounts WHERE id = $1",
        auth.account_id,
    )
    .fetch_one(&state.db)
    .await?;

    let limit = params.limit.unwrap_or(20).min(40).max(1);
    let resolved = params.resolved.unwrap_or(false);

    let rows = sqlx::query!(
        r#"SELECT r.id, r.account_id, r.target_account_id, r.status_ids,
                  r.comment, r.forwarded, r.category, r.action_taken_at,
                  r.created_at, r.updated_at
           FROM reports r
           JOIN accounts a ON a.id = r.account_id
           WHERE a.instance_id = $1
             AND ($2 = (r.action_taken_at IS NOT NULL))
           ORDER BY r.created_at DESC
           LIMIT $3"#,
        instance_id, resolved, limit,
    )
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::with_capacity(rows.len());
    for r in &rows {
        let row = AdminReportRow {
            id: r.id,
            account_id: r.account_id,
            target_account_id: r.target_account_id,
            status_ids: r.status_ids.clone(),
            comment: r.comment.clone(),
            forwarded: r.forwarded,
            category: r.category.clone(),
            action_taken_at: r.action_taken_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        };
        result.push(build_admin_report(&state, &row).await?);
    }
    Ok(Json(result))
}

// ── GET /api/v1/admin/reports/:id ────────────────────────────────────────

pub async fn get_admin_report(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminReport>> {
    require_admin(&state, auth.account_id).await?;
    let r = sqlx::query!(
        r#"SELECT r.id, r.account_id, r.target_account_id, r.status_ids,
                  r.comment, r.forwarded, r.category, r.action_taken_at,
                  r.created_at, r.updated_at
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
        status_ids: r.status_ids.clone(),
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
        r#"SELECT r.id, r.account_id, r.target_account_id, r.status_ids,
                  r.comment, r.forwarded, r.category, r.action_taken_at,
                  r.created_at, r.updated_at
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
        status_ids: r.status_ids.clone(),
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

// ── GET /api/v1/admin/dimension / measures / retention (stubs) ───────────

pub async fn get_dimensions() -> Json<Vec<serde_json::Value>> { Json(vec![]) }
pub async fn get_measures() -> Json<Vec<serde_json::Value>> { Json(vec![]) }
pub async fn get_retention() -> Json<serde_json::Value> { Json(serde_json::json!({"data": []})) }
