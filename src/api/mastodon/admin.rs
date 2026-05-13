use axum::{
    extract::{Extension, Multipart, Path, Query, State},
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

// ── GET /api/v1/admin/dimension / measures / retention (stubs) ───────────

pub async fn get_dimensions() -> Json<Vec<serde_json::Value>> { Json(vec![]) }
pub async fn get_measures() -> Json<Vec<serde_json::Value>> { Json(vec![]) }
pub async fn get_retention() -> Json<serde_json::Value> { Json(serde_json::json!({"data": []})) }

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

fn md5_bytes(s: &str) -> [u8; 16] {
    use std::hash::Hasher;
    // Simple deterministic digest (not security-sensitive — Mastodon uses it for obfuscation display)
    let mut h: u128 = 0x9e3779b97f4a7c15;
    for b in s.bytes() {
        h = h.wrapping_mul(0x6c62272e07bb0142).wrapping_add(b as u128);
    }
    h.to_le_bytes()
}
