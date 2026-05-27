use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::{AppError, AppResult},
    media,
    state::AppState,
};
use super::ConsoleAuth;

#[derive(Debug, Serialize)]
pub struct InstanceResponse {
    pub domain: String,
    pub title: String,
    pub registrations_open: bool,
    pub approval_required: bool,
    pub icon_url: Option<String>,
    pub privacy_policy: String,
}

pub async fn list(
    State(state): State<AppState>,
    ConsoleAuth(_user): ConsoleAuth,
) -> AppResult<Json<Vec<InstanceResponse>>> {
    Ok(Json(vec![InstanceResponse {
        domain: state.instance.domain.clone(),
        title: state.instance.title.clone(),
        registrations_open: state.instance.registrations_open,
        approval_required: state.instance.approval_required,
        icon_url: state.instance.icon_url.clone(),
        privacy_policy: state.instance.privacy_policy.clone(),
    }]))
}

pub async fn get_one(
    State(state): State<AppState>,
    ConsoleAuth(_user): ConsoleAuth,
    Path(_domain): Path<String>,
) -> AppResult<Json<InstanceResponse>> {
    Ok(Json(InstanceResponse {
        domain: state.instance.domain.clone(),
        title: state.instance.title.clone(),
        registrations_open: state.instance.registrations_open,
        approval_required: state.instance.approval_required,
        icon_url: state.instance.icon_url.clone(),
        privacy_policy: state.instance.privacy_policy.clone(),
    }))
}

pub async fn create(
    ConsoleAuth(_user): ConsoleAuth,
    Json(_body): Json<serde_json::Value>,
) -> AppResult<StatusCode> {
    Err(AppError::Unprocessable("Multi-instance creation is not supported in single-tenant mode".into()))
}

pub async fn update(
    ConsoleAuth(_user): ConsoleAuth,
    Path(_domain): Path<String>,
    Json(_body): Json<serde_json::Value>,
) -> AppResult<StatusCode> {
    Err(AppError::Unprocessable("Instance settings must be updated via config file".into()))
}

pub async fn upload_icon(
    State(state): State<AppState>,
    ConsoleAuth(_user): ConsoleAuth,
    Path(_domain): Path<String>,
    mut multipart: Multipart,
) -> AppResult<Json<InstanceResponse>> {
    let storage = &state.storage;

    let mut icon_data: Option<(Vec<u8>, String)> = None;
    while let Some(field) = multipart.next_field().await.map_err(|e| AppError::Unprocessable(e.to_string()))? {
        if field.name() == Some("icon") {
            let content_type = field.content_type().unwrap_or("application/octet-stream").to_string();
            let data = field.bytes().await.map_err(|e| AppError::Unprocessable(e.to_string()))?;
            icon_data = Some((data.to_vec(), content_type));
        }
    }

    let (data, content_type) = icon_data.ok_or_else(|| AppError::Unprocessable("missing icon field".into()))?;
    let key = media::singleton_icon_key(&content_type);
    storage.store(&data, &key, &content_type).await?;
    let _url = storage.public_url(&key);

    Ok(Json(InstanceResponse {
        domain: state.instance.domain.clone(),
        title: state.instance.title.clone(),
        registrations_open: state.instance.registrations_open,
        approval_required: state.instance.approval_required,
        icon_url: state.instance.icon_url.clone(),
        privacy_policy: state.instance.privacy_policy.clone(),
    }))
}

pub async fn delete(
    ConsoleAuth(_user): ConsoleAuth,
    Path(_domain): Path<String>,
) -> AppResult<StatusCode> {
    Err(AppError::Unprocessable("Instance deletion is not supported in single-tenant mode".into()))
}

// ── Invites ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ConsoleInviteResponse {
    pub id: String,
    pub code: String,
    pub url: String,
    pub created_by_account_id: Option<String>,
    pub created_by_username: Option<String>,
    pub max_uses: Option<i32>,
    pub uses: i32,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct InviteTreeMember {
    pub account_id: String,
    pub username: String,
    pub invite_id: Option<String>,
    pub invited_by_account_id: Option<String>,
    pub invited_by_username: Option<String>,
    pub joined_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct RejectedMember {
    pub account_id: String,
    pub username: String,
    pub email: String,
    pub reason: Option<String>,
    pub applied_at: chrono::DateTime<chrono::Utc>,
    pub rejected_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct InviteTreeResponse {
    pub members: Vec<InviteTreeMember>,
    pub invites: Vec<ConsoleInviteResponse>,
    pub rejected: Vec<RejectedMember>,
}

pub async fn invite_tree(
    State(state): State<AppState>,
    ConsoleAuth(_user): ConsoleAuth,
    Path(_domain): Path<String>,
) -> AppResult<Json<InviteTreeResponse>> {
    let members = sqlx::query!(
        r#"SELECT
             a.id        AS account_id,
             a.username,
             u.invite_id,
             inv_a.id    AS "invited_by_account_id?: i64",
             inv_a.username AS "invited_by_username?: String",
             u.created_at
           FROM users u
           JOIN accounts a ON a.id = u.account_id
           LEFT JOIN invites i ON i.id = u.invite_id
           LEFT JOIN users inv_u ON inv_u.id = i.user_id
           LEFT JOIN accounts inv_a ON inv_a.id = inv_u.account_id
           WHERE a.domain IS NULL
           ORDER BY u.created_at ASC"#,
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|r| InviteTreeMember {
        account_id: r.account_id.to_string(),
        username: r.username,
        invite_id: r.invite_id.map(|id| id.to_string()),
        invited_by_account_id: r.invited_by_account_id.map(|id| id.to_string()),
        invited_by_username: r.invited_by_username,
        joined_at: r.created_at,
    })
    .collect();

    let invites = sqlx::query!(
        r#"SELECT
             i.id, i.code, i.max_uses, i.uses, i.expires_at, i.created_at,
             a.id       AS "created_by_account_id?: i64",
             a.username AS "created_by_username?: String"
           FROM invites i
           LEFT JOIN users inv_u ON inv_u.id = i.user_id
           LEFT JOIN accounts a ON a.id = inv_u.account_id
           ORDER BY i.created_at DESC"#,
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|r| ConsoleInviteResponse {
        url: crate::api::mastodon::invites::invite_url(&state.instance.domain, &r.code),
        id: r.id.to_string(),
        code: r.code,
        created_by_account_id: r.created_by_account_id.map(|id| id.to_string()),
        created_by_username: r.created_by_username,
        max_uses: r.max_uses,
        uses: r.uses,
        expires_at: r.expires_at,
        created_at: r.created_at,
    })
    .collect();

    // Mastodon schema: users.approved=false for non-approved; no rejected_at
    let rejected: Vec<RejectedMember> = vec![];

    Ok(Json(InviteTreeResponse { members, invites, rejected }))
}

#[derive(Debug, Deserialize)]
pub struct CreateConsoleInviteRequest {
    pub max_uses: Option<i32>,
}

pub async fn create_console_invite(
    State(state): State<AppState>,
    ConsoleAuth(_user): ConsoleAuth,
    Path(_domain): Path<String>,
    body: Option<Json<CreateConsoleInviteRequest>>,
) -> AppResult<Json<ConsoleInviteResponse>> {
    let max_uses = body.and_then(|Json(b)| b.max_uses);

    if let Some(n) = max_uses {
        if n < 1 {
            return Err(AppError::Unprocessable("max_uses must be at least 1".into()));
        }
    }

    // user_id in invites references users.id
    let admin_user_id = sqlx::query_scalar!(
        "SELECT u.id FROM users u JOIN accounts a ON a.id = u.account_id WHERE a.domain IS NULL ORDER BY u.created_at ASC LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await?;

    let admin_account_id = sqlx::query_scalar!(
        "SELECT account_id FROM users WHERE id = $1",
        admin_user_id,
    )
    .fetch_optional(&state.db)
    .await?;

    let code = crate::api::mastodon::invites::generate_code();

    let row = sqlx::query!(
        r#"INSERT INTO invites (code, user_id, max_uses)
           VALUES ($1, $2, $3)
           RETURNING id, code, max_uses, uses, expires_at, created_at"#,
        code,
        admin_user_id,
        max_uses,
    )
    .fetch_one(&state.db)
    .await?;

    let creator_username = if let Some(aid) = admin_account_id {
        sqlx::query_scalar!("SELECT username FROM accounts WHERE id = $1", aid)
            .fetch_optional(&state.db)
            .await?
    } else {
        None
    };

    Ok(Json(ConsoleInviteResponse {
        url: crate::api::mastodon::invites::invite_url(&state.instance.domain, &row.code),
        id: row.id.to_string(),
        code: row.code,
        created_by_account_id: admin_account_id.map(|id| id.to_string()),
        created_by_username: creator_username,
        max_uses: row.max_uses,
        uses: row.uses,
        expires_at: row.expires_at,
        created_at: row.created_at,
    }))
}

// ── Applications ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ApplicationResponse {
    pub account_id: String,
    pub username: String,
    pub email: String,
    pub reason: Option<String>,
    pub applied_at: chrono::DateTime<chrono::Utc>,
}

pub async fn list_applications(
    State(state): State<AppState>,
    ConsoleAuth(_user): ConsoleAuth,
    Path(_domain): Path<String>,
) -> AppResult<Json<Vec<ApplicationResponse>>> {
    let rows = sqlx::query!(
        r#"SELECT a.id AS account_id, a.username, u.email, u.created_at
           FROM users u
           JOIN accounts a ON a.id = u.account_id
           WHERE a.domain IS NULL
             AND u.approved = false
           ORDER BY u.created_at ASC"#,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows.into_iter().map(|r| ApplicationResponse {
        account_id: r.account_id.to_string(),
        username: r.username,
        email: r.email,
        reason: None,
        applied_at: r.created_at,
    }).collect()))
}

pub async fn approve_application(
    State(state): State<AppState>,
    ConsoleAuth(_user): ConsoleAuth,
    Path((_domain, account_id)): Path<(String, i64)>,
) -> AppResult<StatusCode> {
    let rows_affected = sqlx::query!(
        "UPDATE users SET approved = true, updated_at = now() WHERE account_id = $1 AND approved = false",
        account_id,
    )
    .execute(&state.db)
    .await?
    .rows_affected();

    if rows_affected == 0 {
        return Err(AppError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn reject_application(
    State(state): State<AppState>,
    ConsoleAuth(_user): ConsoleAuth,
    Path((_domain, account_id)): Path<(String, i64)>,
) -> AppResult<StatusCode> {
    // In Mastodon schema, rejection means deleting the user account
    let rows_affected = sqlx::query!(
        "DELETE FROM users WHERE account_id = $1 AND approved = false",
        account_id,
    )
    .execute(&state.db)
    .await?
    .rows_affected();

    if rows_affected == 0 {
        return Err(AppError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

// ── Announcements ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AnnouncementResponse {
    pub id: String,
    pub text: String,
    pub published: bool,
    pub all_day: bool,
    pub starts_at: Option<String>,
    pub ends_at: Option<String>,
    pub published_at: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct AnnouncementForm {
    pub text: String,
    pub published: Option<bool>,
    pub all_day: Option<bool>,
    pub starts_at: Option<String>,
    pub ends_at: Option<String>,
}

pub async fn list_announcements(
    State(state): State<AppState>,
    ConsoleAuth(_user): ConsoleAuth,
    Path(_domain): Path<String>,
) -> AppResult<Json<Vec<AnnouncementResponse>>> {
    let rows = sqlx::query!(
        "SELECT id, text, published, all_day, starts_at, ends_at, published_at, created_at, updated_at
         FROM announcements ORDER BY published_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows.into_iter().map(|r| AnnouncementResponse {
        id: r.id.to_string(),
        text: r.text,
        published: r.published,
        all_day: r.all_day,
        starts_at: r.starts_at.map(|t| t.to_rfc3339()),
        ends_at: r.ends_at.map(|t| t.to_rfc3339()),
        published_at: r.published_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
        created_at: r.created_at.to_rfc3339(),
        updated_at: r.updated_at.to_rfc3339(),
    }).collect()))
}

pub async fn create_announcement(
    State(state): State<AppState>,
    ConsoleAuth(_user): ConsoleAuth,
    Path(_domain): Path<String>,
    Json(form): Json<AnnouncementForm>,
) -> AppResult<Json<AnnouncementResponse>> {
    let starts_at = form.starts_at.as_deref()
        .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok());
    let ends_at = form.ends_at.as_deref()
        .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok());

    let r = sqlx::query!(
        r#"INSERT INTO announcements (text, published, all_day, starts_at, ends_at)
           VALUES ($1, $2, $3, $4, $5)
           RETURNING id, text, published, all_day, starts_at, ends_at, published_at, created_at, updated_at"#,
        form.text,
        form.published.unwrap_or(true),
        form.all_day.unwrap_or(false),
        starts_at,
        ends_at,
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(AnnouncementResponse {
        id: r.id.to_string(),
        text: r.text,
        published: r.published,
        all_day: r.all_day,
        starts_at: r.starts_at.map(|t| t.to_rfc3339()),
        ends_at: r.ends_at.map(|t| t.to_rfc3339()),
        published_at: r.published_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
        created_at: r.created_at.to_rfc3339(),
        updated_at: r.updated_at.to_rfc3339(),
    }))
}

pub async fn update_announcement(
    State(state): State<AppState>,
    ConsoleAuth(_user): ConsoleAuth,
    Path((_domain, id)): Path<(String, i64)>,
    Json(form): Json<AnnouncementForm>,
) -> AppResult<Json<AnnouncementResponse>> {
    let starts_at = form.starts_at.as_deref()
        .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok());
    let ends_at = form.ends_at.as_deref()
        .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok());

    let r = sqlx::query!(
        r#"UPDATE announcements
           SET text = $2, published = $3, all_day = $4, starts_at = $5, ends_at = $6, updated_at = now()
           WHERE id = $1
           RETURNING id, text, published, all_day, starts_at, ends_at, published_at, created_at, updated_at"#,
        id,
        form.text,
        form.published.unwrap_or(true),
        form.all_day.unwrap_or(false),
        starts_at,
        ends_at,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(Json(AnnouncementResponse {
        id: r.id.to_string(),
        text: r.text,
        published: r.published,
        all_day: r.all_day,
        starts_at: r.starts_at.map(|t| t.to_rfc3339()),
        ends_at: r.ends_at.map(|t| t.to_rfc3339()),
        published_at: r.published_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
        created_at: r.created_at.to_rfc3339(),
        updated_at: r.updated_at.to_rfc3339(),
    }))
}

pub async fn delete_announcement(
    State(state): State<AppState>,
    ConsoleAuth(_user): ConsoleAuth,
    Path((_domain, id)): Path<(String, i64)>,
) -> AppResult<StatusCode> {
    let deleted = sqlx::query_scalar!(
        "DELETE FROM announcements WHERE id = $1 RETURNING id",
        id,
    )
    .fetch_optional(&state.db)
    .await?;

    if deleted.is_none() {
        return Err(AppError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}
