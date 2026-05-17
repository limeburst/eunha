use axum::{
    extract::{Extension, Multipart, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use crate::{
    db::models,
    error::{AppError, AppResult},
    middleware::AuthenticatedUser,
    state::AppState,
};
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
        sensitized: account.sensitized_at.is_some(),
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
             AND ($3::text IS NULL OR a.username = $3)
             AND ($4::text IS NULL OR
                  ($4 = 'suspended' AND a.suspended_at IS NOT NULL) OR
                  ($4 = 'silenced' AND a.silenced_at IS NOT NULL) OR
                  ($4 = 'pending' AND EXISTS (
                      SELECT 1 FROM users u WHERE u.account_id = a.id AND u.approved_at IS NULL
                  )) OR
                  ($4 = 'active' AND a.suspended_at IS NULL AND a.silenced_at IS NULL
                      AND NOT EXISTS (SELECT 1 FROM users u WHERE u.account_id = a.id AND u.approved_at IS NULL))
             )
           ORDER BY a.created_at DESC
           LIMIT $2"#,
        instance_id, limit, params.username.as_deref(), params.status.as_deref(),
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
    account_id: i64,
    target_account_id: i64,
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
    let instance_id = sqlx::query_scalar!(
        "SELECT instance_id FROM accounts WHERE id = $1",
        auth.account_id,
    )
    .fetch_one(&state.db)
    .await?;

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
                    "SELECT COUNT(*) FROM accounts WHERE instance_id = $1 AND domain IS NULL AND created_at BETWEEN $2 AND $3",
                    instance_id, start, end,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                let previous_total = sqlx::query_scalar!(
                    "SELECT COUNT(*) FROM accounts WHERE instance_id = $1 AND domain IS NULL AND created_at BETWEEN $2 AND $3",
                    instance_id, prev_start, start,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                let data = sqlx::query!(
                    r#"SELECT date_trunc('day', created_at)::timestamptz AS day, COUNT(*) AS n
                       FROM accounts WHERE instance_id = $1 AND domain IS NULL AND created_at BETWEEN $2 AND $3
                       GROUP BY day ORDER BY day"#,
                    instance_id, start, end,
                ).fetch_all(&state.db).await?;
                serde_json::json!({
                    "key": key,
                    "unit": null,
                    "total": total.to_string(),
                    "human_value": total.to_string(),
                    "previous_total": previous_total.to_string(),
                    "data": data.iter().map(|r| serde_json::json!({
                        "date": r.day.map(|d| d.to_rfc3339()).unwrap_or_default(),
                        "value": r.n.unwrap_or(0).to_string(),
                    })).collect::<Vec<_>>(),
                })
            }
            "active_users" => {
                let total = sqlx::query_scalar!(
                    r#"SELECT COUNT(DISTINCT s.account_id) FROM statuses s
                       JOIN accounts a ON a.id = s.account_id
                       WHERE a.instance_id = $1 AND a.domain IS NULL AND s.created_at BETWEEN $2 AND $3 AND s.deleted_at IS NULL"#,
                    instance_id, start, end,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                let previous_total = sqlx::query_scalar!(
                    r#"SELECT COUNT(DISTINCT s.account_id) FROM statuses s
                       JOIN accounts a ON a.id = s.account_id
                       WHERE a.instance_id = $1 AND a.domain IS NULL AND s.created_at BETWEEN $2 AND $3 AND s.deleted_at IS NULL"#,
                    instance_id, prev_start, start,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                let data = sqlx::query!(
                    r#"SELECT date_trunc('day', s.created_at)::timestamptz AS day, COUNT(DISTINCT s.account_id) AS n
                       FROM statuses s JOIN accounts a ON a.id = s.account_id
                       WHERE a.instance_id = $1 AND a.domain IS NULL AND s.created_at BETWEEN $2 AND $3 AND s.deleted_at IS NULL
                       GROUP BY day ORDER BY day"#,
                    instance_id, start, end,
                ).fetch_all(&state.db).await?;
                serde_json::json!({
                    "key": key,
                    "unit": null,
                    "total": total.to_string(),
                    "human_value": total.to_string(),
                    "previous_total": previous_total.to_string(),
                    "data": data.iter().map(|r| serde_json::json!({
                        "date": r.day.map(|d| d.to_rfc3339()).unwrap_or_default(),
                        "value": r.n.unwrap_or(0).to_string(),
                    })).collect::<Vec<_>>(),
                })
            }
            "new_statuses" => {
                let total = sqlx::query_scalar!(
                    r#"SELECT COUNT(*) FROM statuses s JOIN accounts a ON a.id = s.account_id
                       WHERE a.instance_id = $1 AND a.domain IS NULL AND s.created_at BETWEEN $2 AND $3 AND s.deleted_at IS NULL"#,
                    instance_id, start, end,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                let previous_total = sqlx::query_scalar!(
                    r#"SELECT COUNT(*) FROM statuses s JOIN accounts a ON a.id = s.account_id
                       WHERE a.instance_id = $1 AND a.domain IS NULL AND s.created_at BETWEEN $2 AND $3 AND s.deleted_at IS NULL"#,
                    instance_id, prev_start, start,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                let data = sqlx::query!(
                    r#"SELECT date_trunc('day', s.created_at)::timestamptz AS day, COUNT(*) AS n
                       FROM statuses s JOIN accounts a ON a.id = s.account_id
                       WHERE a.instance_id = $1 AND a.domain IS NULL AND s.created_at BETWEEN $2 AND $3 AND s.deleted_at IS NULL
                       GROUP BY day ORDER BY day"#,
                    instance_id, start, end,
                ).fetch_all(&state.db).await?;
                serde_json::json!({
                    "key": key,
                    "unit": null,
                    "total": total.to_string(),
                    "human_value": total.to_string(),
                    "previous_total": previous_total.to_string(),
                    "data": data.iter().map(|r| serde_json::json!({
                        "date": r.day.map(|d| d.to_rfc3339()).unwrap_or_default(),
                        "value": r.n.unwrap_or(0).to_string(),
                    })).collect::<Vec<_>>(),
                })
            }
            "opened_reports" => {
                let total = sqlx::query_scalar!(
                    r#"SELECT COUNT(*) FROM reports r JOIN accounts a ON a.id = r.account_id
                       WHERE a.instance_id = $1 AND r.created_at BETWEEN $2 AND $3"#,
                    instance_id, start, end,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                let previous_total = sqlx::query_scalar!(
                    r#"SELECT COUNT(*) FROM reports r JOIN accounts a ON a.id = r.account_id
                       WHERE a.instance_id = $1 AND r.created_at BETWEEN $2 AND $3"#,
                    instance_id, prev_start, start,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                serde_json::json!({
                    "key": key, "unit": null,
                    "total": total.to_string(), "human_value": total.to_string(),
                    "previous_total": previous_total.to_string(), "data": [],
                })
            }
            "resolved_reports" => {
                let total = sqlx::query_scalar!(
                    r#"SELECT COUNT(*) FROM reports r JOIN accounts a ON a.id = r.account_id
                       WHERE a.instance_id = $1 AND r.action_taken_at BETWEEN $2 AND $3"#,
                    instance_id, start, end,
                ).fetch_one(&state.db).await?.unwrap_or(0);
                let previous_total = sqlx::query_scalar!(
                    r#"SELECT COUNT(*) FROM reports r JOIN accounts a ON a.id = r.account_id
                       WHERE a.instance_id = $1 AND r.action_taken_at BETWEEN $2 AND $3"#,
                    instance_id, prev_start, start,
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
    let instance_id = sqlx::query_scalar!(
        "SELECT instance_id FROM accounts WHERE id = $1",
        auth.account_id,
    )
    .fetch_one(&state.db)
    .await?;

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
                       WHERE a.instance_id = $1 AND s.created_at BETWEEN $2 AND $3 AND s.deleted_at IS NULL
                       GROUP BY server ORDER BY n DESC LIMIT $4"#,
                    instance_id, start, end, limit,
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
    let instance_id = sqlx::query_scalar!(
        "SELECT instance_id FROM accounts WHERE id = $1",
        auth.account_id,
    )
    .fetch_one(&state.db)
    .await?;

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
               date_trunc($3, a.created_at)::timestamptz AS period,
               a.id AS account_id
           FROM accounts a
           WHERE a.instance_id = $1
             AND a.domain IS NULL
             AND a.created_at BETWEEN $2 AND $4
           ORDER BY period"#,
        instance_id,
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
                "date": check_period.to_rfc3339(),
                "rate": rate,
                "value": active_count,
            }));
            check_period = next_period;
            count += 1;
        }

        data.push(serde_json::json!({
            "period": period.to_rfc3339(),
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
    let instance_id = sqlx::query_scalar!(
        "SELECT instance_id FROM accounts WHERE id = $1",
        auth.account_id,
    )
    .fetch_one(&state.db)
    .await?;

    let rows = sqlx::query!(
        "SELECT id, shortcode, image_url, static_image_url, visible_in_picker, disabled
         FROM custom_emojis WHERE instance_id = $1 AND domain IS NULL ORDER BY shortcode",
        instance_id,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows.into_iter().map(|r| AdminCustomEmoji {
        id: r.id.to_string(),
        shortcode: r.shortcode.clone(),
        url: r.image_url.clone(),
        static_url: r.static_image_url.unwrap_or_else(|| r.image_url.clone()),
        visible_in_picker: r.visible_in_picker,
        disabled: r.disabled,
        category: None,
    }).collect()))
}

// ── POST /api/v1/admin/custom_emojis ─────────────────────────────────────

pub async fn create_admin_custom_emoji(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    mut multipart: Multipart,
) -> AppResult<Json<AdminCustomEmoji>> {
    require_admin(&state, auth.account_id).await?;

    let instance_id = sqlx::query_scalar!(
        "SELECT instance_id FROM accounts WHERE id = $1",
        auth.account_id,
    )
    .fetch_one(&state.db)
    .await?;

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
    let key = format!("{}/emoji/{}.{}", instance_id, shortcode, ext);
    state.storage.store(&image_data, &key, &content_type).await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("storage: {e}")))?;
    let url = state.storage.public_url(&key);

    let row = sqlx::query!(
        r#"INSERT INTO custom_emojis (instance_id, shortcode, image_url, static_image_url, visible_in_picker)
           VALUES ($1, $2, $3, $3, true)
           ON CONFLICT (instance_id, shortcode)
           DO UPDATE SET image_url = $3, static_image_url = $3, disabled = false
           RETURNING id, shortcode, image_url, static_image_url, visible_in_picker, disabled"#,
        instance_id, shortcode, url,
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(AdminCustomEmoji {
        id: row.id.to_string(),
        shortcode: row.shortcode.clone(),
        url: row.image_url.clone(),
        static_url: row.static_image_url.unwrap_or_else(|| row.image_url.clone()),
        visible_in_picker: row.visible_in_picker,
        disabled: row.disabled,
        category: None,
    }))
}

// ── DELETE /api/v1/admin/custom_emojis/:id ───────────────────────────────

pub async fn delete_admin_custom_emoji(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<uuid::Uuid>,
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
    Path(id): Path<uuid::Uuid>,
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
        "SELECT id, shortcode, image_url, static_image_url, visible_in_picker, disabled FROM custom_emojis WHERE id = $1",
        id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(AdminCustomEmoji {
        id: row.id.to_string(),
        shortcode: row.shortcode.clone(),
        url: row.image_url.clone(),
        static_url: row.static_image_url.unwrap_or_else(|| row.image_url.clone()),
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
        "SELECT id, domain, severity, reject_media, reject_reports, private_comment, public_comment, obfuscate, created_at
         FROM domain_blocks ORDER BY domain",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(|r| AdminDomainBlock {
        id: r.id.to_string(),
        digest: hex::encode(md5_bytes(&r.domain)),
        domain: r.domain,
        created_at: r.created_at.to_rfc3339(),
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
    let severity = form.severity.as_deref().unwrap_or("silence");
    let row = sqlx::query!(
        r#"INSERT INTO domain_blocks (domain, severity, reject_media, reject_reports, private_comment, public_comment, obfuscate)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           ON CONFLICT (domain) DO UPDATE SET severity = $2, reject_media = $3, reject_reports = $4,
             private_comment = $5, public_comment = $6, obfuscate = $7, updated_at = now()
           RETURNING id, domain, severity, reject_media, reject_reports, private_comment, public_comment, obfuscate, created_at"#,
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
        digest: hex::encode(md5_bytes(&row.domain)),
        domain: row.domain,
        created_at: row.created_at.to_rfc3339(),
        severity: row.severity,
        reject_media: row.reject_media,
        reject_reports: row.reject_reports,
        private_comment: row.private_comment,
        public_comment: row.public_comment,
        obfuscate: row.obfuscate,
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
        created_at: r.created_at.to_rfc3339(),
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
        created_at: row.created_at.to_rfc3339(),
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
        "SELECT id, ip, severity, comment, expires_at, created_at FROM admin_ip_blocks ORDER BY created_at DESC"
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(|r| AdminIpBlock {
        id: r.id.to_string(),
        ip: r.ip,
        severity: r.severity,
        comment: r.comment,
        expires_at: r.expires_at.map(|t| t.to_rfc3339()),
        created_at: r.created_at.to_rfc3339(),
    }).collect()))
}

pub async fn get_ip_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminIpBlock>> {
    require_admin(&state, auth.account_id).await?;
    let r = sqlx::query!(
        "SELECT id, ip, severity, comment, expires_at, created_at FROM admin_ip_blocks WHERE id = $1",
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(AdminIpBlock {
        id: r.id.to_string(),
        ip: r.ip,
        severity: r.severity,
        comment: r.comment,
        expires_at: r.expires_at.map(|t| t.to_rfc3339()),
        created_at: r.created_at.to_rfc3339(),
    }))
}

pub async fn create_ip_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<CreateIpBlockForm>,
) -> AppResult<Json<AdminIpBlock>> {
    require_admin(&state, auth.account_id).await?;
    let severity = form.severity.as_deref().unwrap_or("sign_up_block");
    let expires_at = form.expires_in
        .map(|secs| chrono::Utc::now() + chrono::Duration::seconds(secs));
    let r = sqlx::query!(
        r#"INSERT INTO admin_ip_blocks (ip, severity, comment, expires_at)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (ip) DO UPDATE SET severity = $2, comment = $3, expires_at = $4, updated_at = now()
           RETURNING id, ip, severity, comment, expires_at, created_at"#,
        form.ip, severity, form.comment, expires_at,
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(AdminIpBlock {
        id: r.id.to_string(),
        ip: r.ip,
        severity: r.severity,
        comment: r.comment,
        expires_at: r.expires_at.map(|t| t.to_rfc3339()),
        created_at: r.created_at.to_rfc3339(),
    }))
}

pub async fn update_ip_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
    Json(form): Json<CreateIpBlockForm>,
) -> AppResult<Json<AdminIpBlock>> {
    require_admin(&state, auth.account_id).await?;
    let severity = form.severity.as_deref().unwrap_or("sign_up_block");
    let expires_at = form.expires_in
        .map(|secs| chrono::Utc::now() + chrono::Duration::seconds(secs));
    let r = sqlx::query!(
        r#"UPDATE admin_ip_blocks SET severity = $2, comment = $3, expires_at = $4, updated_at = now()
           WHERE id = $1
           RETURNING id, ip, severity, comment, expires_at, created_at"#,
        id, severity, form.comment, expires_at,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(AdminIpBlock {
        id: r.id.to_string(),
        ip: r.ip,
        severity: r.severity,
        comment: r.comment,
        expires_at: r.expires_at.map(|t| t.to_rfc3339()),
        created_at: r.created_at.to_rfc3339(),
    }))
}

pub async fn delete_ip_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!("DELETE FROM admin_ip_blocks WHERE id = $1", id)
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
        "SELECT id, domain, created_at FROM admin_email_domain_blocks ORDER BY domain"
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(|r| AdminEmailDomainBlock {
        id: r.id.to_string(),
        domain: r.domain,
        created_at: r.created_at.to_rfc3339(),
        history: vec![],
    }).collect()))
}

pub async fn get_email_domain_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<Json<AdminEmailDomainBlock>> {
    require_admin(&state, auth.account_id).await?;
    let r = sqlx::query!(
        "SELECT id, domain, created_at FROM admin_email_domain_blocks WHERE id = $1",
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(AdminEmailDomainBlock {
        id: r.id.to_string(),
        domain: r.domain,
        created_at: r.created_at.to_rfc3339(),
        history: vec![],
    }))
}

pub async fn create_email_domain_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<CreateEmailDomainBlockForm>,
) -> AppResult<Json<AdminEmailDomainBlock>> {
    require_admin(&state, auth.account_id).await?;
    let r = sqlx::query!(
        r#"INSERT INTO admin_email_domain_blocks (domain) VALUES ($1)
           ON CONFLICT (domain) DO UPDATE SET updated_at = now()
           RETURNING id, domain, created_at"#,
        form.domain,
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(AdminEmailDomainBlock {
        id: r.id.to_string(),
        domain: r.domain,
        created_at: r.created_at.to_rfc3339(),
        history: vec![],
    }))
}

pub async fn delete_email_domain_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    require_admin(&state, auth.account_id).await?;
    sqlx::query!("DELETE FROM admin_email_domain_blocks WHERE id = $1", id)
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
    super::trends::trending_tags(state, instance, query).await
}

pub async fn admin_trending_statuses(
    state: State<AppState>,
    instance: axum::extract::Extension<crate::middleware::ResolvedInstance>,
    query: axum::extract::Query<super::trends::TrendParams>,
    auth: axum::extract::Extension<AuthenticatedUser>,
) -> AppResult<axum::Json<Vec<super::types::Status>>> {
    require_admin(&state, auth.account_id).await?;
    super::trends::trending_statuses(state, instance, query, Some(axum::extract::Extension(crate::middleware::AuthenticatedUser { account_id: auth.account_id, token_id: auth.token_id, scopes: auth.scopes.clone(), application_id: auth.application_id }))).await
}

pub async fn admin_trending_links(
    state: State<AppState>,
    query: axum::extract::Query<super::trends::TrendParams>,
    auth: axum::extract::Extension<AuthenticatedUser>,
) -> AppResult<axum::Json<Vec<super::types::PreviewCard>>> {
    require_admin(&state, auth.account_id).await?;
    super::trends::trending_links(state, query).await
}

fn md5_bytes(s: &str) -> [u8; 16] {
    // Simple deterministic digest (not security-sensitive — Mastodon uses it for obfuscation display)
    let mut h: u128 = 0x9e3779b97f4a7c15;
    for b in s.bytes() {
        h = h.wrapping_mul(0x6c62272e07bb0142).wrapping_add(b as u128);
    }
    h.to_le_bytes()
}
