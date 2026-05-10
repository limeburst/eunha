use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    db::models::Instance,
    error::{AppError, AppResult},
    state::AppState,
};
use super::ConsoleAuth;

#[derive(Debug, Serialize)]
pub struct InstanceResponse {
    pub id: String,
    pub domain: String,
    pub custom_domain: Option<String>,
    pub title: String,
    pub status: String,
    pub plan: String,
    pub region: String,
    pub registrations_open: bool,
    pub approval_required: bool,
    pub created_at: String,
    pub admin_account: Option<String>,
}

impl InstanceResponse {
    fn from_instance(instance: &Instance, admin: Option<String>) -> Self {
        InstanceResponse {
            id: instance.id.to_string(),
            domain: instance.domain.clone(),
            custom_domain: instance.custom_domain.clone(),
            title: instance.title.clone(),
            status: "running".to_string(),
            plan: "free".to_string(),
            region: "default".to_string(),
            registrations_open: instance.registrations_open,
            approval_required: instance.approval_required,
            created_at: instance.created_at.to_rfc3339(),
            admin_account: admin,
        }
    }
}

pub async fn list(
    State(state): State<AppState>,
    ConsoleAuth(user): ConsoleAuth,
) -> AppResult<Json<Vec<InstanceResponse>>> {
    let instances = sqlx::query_as!(
        Instance,
        "SELECT * FROM instances WHERE console_user_id = $1 ORDER BY created_at DESC",
        user.id,
    )
    .fetch_all(&state.db)
    .await?;

    let mut responses = Vec::with_capacity(instances.len());
    for instance in &instances {
        let admin = first_admin_username(&state, instance.id).await?;
        responses.push(InstanceResponse::from_instance(instance, admin));
    }

    Ok(Json(responses))
}

pub async fn get_one(
    State(state): State<AppState>,
    ConsoleAuth(user): ConsoleAuth,
    Path(domain): Path<String>,
) -> AppResult<Json<InstanceResponse>> {
    let instance = instance_for_user(&state, &domain, user.id).await?;
    let admin = first_admin_username(&state, instance.id).await?;
    Ok(Json(InstanceResponse::from_instance(&instance, admin)))
}

#[derive(Debug, Deserialize)]
pub struct CreateInstanceRequest {
    pub domain: String,
    pub custom_domain: Option<String>,
    pub title: String,
    pub admin_username: String,
    pub admin_email: String,
    pub admin_password: String,
}

pub async fn create(
    State(state): State<AppState>,
    ConsoleAuth(user): ConsoleAuth,
    Json(body): Json<CreateInstanceRequest>,
) -> AppResult<(StatusCode, Json<InstanceResponse>)> {
    let domain = body.domain.trim().to_lowercase();
    if domain.is_empty() {
        return Err(AppError::Unprocessable("domain is required".into()));
    }

    let username = body.admin_username.trim().to_lowercase();
    if username.is_empty() {
        return Err(AppError::Unprocessable("admin_username is required".into()));
    }
    if body.admin_password.len() < 8 {
        return Err(AppError::Unprocessable("admin_password must be at least 8 characters".into()));
    }

    // Normalise custom domain
    let custom_domain: Option<String> = body.custom_domain
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase());

    // Check domain not already taken
    let exists = sqlx::query_scalar!(
        "SELECT 1 FROM instances WHERE domain = $1 OR custom_domain = $1",
        domain
    )
    .fetch_optional(&state.db)
    .await?;
    if exists.is_some() {
        return Err(AppError::Conflict);
    }

    if let Some(ref cd) = custom_domain {
        let taken = sqlx::query_scalar!(
            "SELECT 1 FROM instances WHERE domain = $1 OR custom_domain = $1",
            cd
        )
        .fetch_optional(&state.db)
        .await?;
        if taken.is_some() {
            return Err(AppError::Conflict);
        }
    }

    // Generate instance keypair (for ActivityPub instance actor)
    let (instance_private_key, instance_public_key) = generate_rsa_keypair()?;

    let instance = sqlx::query_as!(
        Instance,
        r#"INSERT INTO instances
             (domain, custom_domain, title, private_key, public_key, console_user_id)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING *"#,
        domain,
        custom_domain,
        body.title.trim(),
        instance_private_key,
        instance_public_key,
        user.id,
    )
    .fetch_one(&state.db)
    .await?;

    // Generate admin account keypair
    let (account_private_key, account_public_key) = generate_rsa_keypair()?;

    let base_url = format!("https://{}", domain);
    let uri = format!("{}/users/{}", base_url, username);
    let inbox_url = format!("{}/inbox", uri);
    let outbox_url = format!("{}/outbox", uri);
    let url = format!("{}/{}", base_url, username);

    let account = sqlx::query!(
        r#"INSERT INTO accounts
             (instance_id, username, display_name, note, note_text, url, uri,
              private_key, public_key, inbox_url, outbox_url, shared_inbox_url)
           VALUES ($1, $2, $2, '', '', $3, $4, $5, $6, $7, $8, $9)
           RETURNING id"#,
        instance.id,
        username,
        url,
        uri,
        account_private_key,
        account_public_key,
        inbox_url,
        outbox_url,
        format!("https://{}/inbox", domain),
    )
    .fetch_one(&state.db)
    .await?;

    let admin_email = body.admin_email.trim();
    let password_hash = hash_password(&body.admin_password)?;

    sqlx::query!(
        r#"INSERT INTO users
             (account_id, instance_id, email, email_normalized, password_hash, confirmed_at)
           VALUES ($1, $2, $3, $4, $5, now())"#,
        account.id,
        instance.id,
        admin_email,
        admin_email.to_lowercase(),
        password_hash,
    )
    .execute(&state.db)
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(InstanceResponse::from_instance(&instance, Some(username))),
    ))
}

#[derive(Debug, Deserialize)]
pub struct UpdateInstanceRequest {
    pub title: Option<String>,
    /// `null` clears the custom domain; omitting the field leaves it unchanged.
    /// We use a double-Option to distinguish "not provided" from "set to null".
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub custom_domain: MaybeAbsent<Option<String>>,
    pub registrations_open: Option<bool>,
    pub approval_required: Option<bool>,
}

/// Represents a JSON field that may be absent vs explicitly null/present.
#[derive(Debug, Default)]
pub enum MaybeAbsent<T> {
    #[default]
    Absent,
    Present(T),
}

fn deserialize_optional_field<'de, D, T>(d: D) -> Result<MaybeAbsent<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Ok(MaybeAbsent::Present(T::deserialize(d)?))
}

pub async fn update(
    State(state): State<AppState>,
    ConsoleAuth(user): ConsoleAuth,
    Path(domain): Path<String>,
    Json(body): Json<UpdateInstanceRequest>,
) -> AppResult<Json<InstanceResponse>> {
    let instance = instance_for_user(&state, &domain, user.id).await?;

    if let Some(title) = &body.title {
        sqlx::query!(
            "UPDATE instances SET title = $1, updated_at = now() WHERE id = $2",
            title.trim(),
            instance.id,
        )
        .execute(&state.db)
        .await?;
    }

    if let Some(registrations_open) = body.registrations_open {
        sqlx::query!(
            "UPDATE instances SET registrations_open = $1, updated_at = now() WHERE id = $2",
            registrations_open,
            instance.id,
        )
        .execute(&state.db)
        .await?;
    }

    if let Some(approval_required) = body.approval_required {
        sqlx::query!(
            "UPDATE instances SET approval_required = $1, updated_at = now() WHERE id = $2",
            approval_required,
            instance.id,
        )
        .execute(&state.db)
        .await?;

        // Turning approval off: approve everyone who was waiting
        if !approval_required {
            sqlx::query!(
                "UPDATE users SET approved_at = now(), updated_at = now() WHERE instance_id = $1 AND approved_at IS NULL",
                instance.id,
            )
            .execute(&state.db)
            .await?;
        }
    }

    if let MaybeAbsent::Present(ref custom_domain) = body.custom_domain {
        let cd: Option<String> = custom_domain
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase());

        if let Some(ref cd_val) = cd {
            let taken = sqlx::query_scalar!(
                "SELECT 1 FROM instances WHERE (domain = $1 OR custom_domain = $1) AND id != $2",
                cd_val,
                instance.id,
            )
            .fetch_optional(&state.db)
            .await?;
            if taken.is_some() {
                return Err(AppError::Conflict);
            }
        }

        sqlx::query!(
            "UPDATE instances SET custom_domain = $1, updated_at = now() WHERE id = $2",
            cd,
            instance.id,
        )
        .execute(&state.db)
        .await?;
    }

    let updated = sqlx::query_as!(Instance, "SELECT * FROM instances WHERE id = $1", instance.id)
        .fetch_one(&state.db)
        .await?;

    let admin = first_admin_username(&state, updated.id).await?;
    Ok(Json(InstanceResponse::from_instance(&updated, admin)))
}

pub async fn delete(
    State(state): State<AppState>,
    ConsoleAuth(user): ConsoleAuth,
    Path(domain): Path<String>,
) -> AppResult<StatusCode> {
    let instance = instance_for_user(&state, &domain, user.id).await?;

    sqlx::query!("DELETE FROM instances WHERE id = $1", instance.id)
        .execute(&state.db)
        .await?;

    Ok(StatusCode::NO_CONTENT)
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
    /// Invite ID used at signup; null if no invite was used.
    pub invite_id: Option<String>,
    /// Account ID of the person whose invite was used; null for root members.
    pub invited_by_account_id: Option<String>,
    pub invited_by_username: Option<String>,
    pub joined_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct InviteTreeResponse {
    pub members: Vec<InviteTreeMember>,
    pub invites: Vec<ConsoleInviteResponse>,
}

pub async fn invite_tree(
    State(state): State<AppState>,
    ConsoleAuth(user): ConsoleAuth,
    Path(domain): Path<String>,
) -> AppResult<Json<InviteTreeResponse>> {
    let instance = instance_for_user(&state, &domain, user.id).await?;

    let members = sqlx::query!(
        r#"SELECT
             a.id        AS account_id,
             a.username,
             u.invite_id AS "invite_id?: Uuid",
             inv_a.id    AS "invited_by_account_id?: Uuid",
             inv_a.username AS "invited_by_username?: String",
             u.created_at
           FROM users u
           JOIN accounts a ON a.id = u.account_id
           LEFT JOIN invites i ON i.id = u.invite_id
           LEFT JOIN accounts inv_a ON inv_a.id = i.created_by
           WHERE u.instance_id = $1 AND a.domain IS NULL
           ORDER BY u.created_at ASC"#,
        instance.id,
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
             a.id       AS "created_by_account_id?: Uuid",
             a.username AS "created_by_username?: String"
           FROM invites i
           LEFT JOIN accounts a ON a.id = i.created_by
           WHERE i.instance_id = $1
           ORDER BY i.created_at DESC"#,
        instance.id,
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|r| ConsoleInviteResponse {
        url: crate::api::mastodon::invites::invite_url(&instance.domain, &r.code),
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

    Ok(Json(InviteTreeResponse { members, invites }))
}

pub async fn create_console_invite(
    State(state): State<AppState>,
    ConsoleAuth(user): ConsoleAuth,
    Path(domain): Path<String>,
) -> AppResult<Json<ConsoleInviteResponse>> {
    let instance = instance_for_user(&state, &domain, user.id).await?;

    // Create the invite on behalf of the first admin account (if one exists),
    // so it appears in the tree rooted at the admin.
    let admin_id = sqlx::query_scalar!(
        r#"SELECT a.id FROM accounts a
           JOIN users u ON u.account_id = a.id
           WHERE a.instance_id = $1 AND a.domain IS NULL
           ORDER BY u.created_at ASC LIMIT 1"#,
        instance.id,
    )
    .fetch_optional(&state.db)
    .await?;

    let code = crate::api::mastodon::invites::generate_code();

    let row = sqlx::query!(
        r#"INSERT INTO invites (instance_id, code, created_by)
           VALUES ($1, $2, $3)
           RETURNING id, code, max_uses, uses, expires_at, created_at"#,
        instance.id,
        code,
        admin_id,
    )
    .fetch_one(&state.db)
    .await?;

    let creator_username = if let Some(aid) = admin_id {
        sqlx::query_scalar!("SELECT username FROM accounts WHERE id = $1", aid)
            .fetch_optional(&state.db)
            .await?
    } else {
        None
    };

    Ok(Json(ConsoleInviteResponse {
        url: crate::api::mastodon::invites::invite_url(&instance.domain, &row.code),
        id: row.id.to_string(),
        code: row.code,
        created_by_account_id: admin_id.map(|id| id.to_string()),
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
    ConsoleAuth(user): ConsoleAuth,
    Path(domain): Path<String>,
) -> AppResult<Json<Vec<ApplicationResponse>>> {
    let instance = instance_for_user(&state, &domain, user.id).await?;

    let rows = sqlx::query!(
        r#"SELECT a.id AS account_id, a.username, u.email, u.reason, u.created_at
           FROM users u
           JOIN accounts a ON a.id = u.account_id
           WHERE u.instance_id = $1
             AND a.domain IS NULL
             AND u.approved_at IS NULL
           ORDER BY u.created_at ASC"#,
        instance.id,
    )
    .fetch_all(&state.db)
    .await?;

    let apps = rows.into_iter().map(|r| ApplicationResponse {
        account_id: r.account_id.to_string(),
        username: r.username,
        email: r.email,
        reason: r.reason,
        applied_at: r.created_at,
    }).collect();

    Ok(Json(apps))
}

pub async fn approve_application(
    State(state): State<AppState>,
    ConsoleAuth(user): ConsoleAuth,
    Path((domain, account_id)): Path<(String, Uuid)>,
) -> AppResult<StatusCode> {
    let instance = instance_for_user(&state, &domain, user.id).await?;

    let rows_affected = sqlx::query!(
        r#"UPDATE users SET approved_at = now(), updated_at = now()
           WHERE account_id = $1 AND instance_id = $2 AND approved_at IS NULL"#,
        account_id,
        instance.id,
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
    ConsoleAuth(user): ConsoleAuth,
    Path((domain, account_id)): Path<(String, Uuid)>,
) -> AppResult<StatusCode> {
    let instance = instance_for_user(&state, &domain, user.id).await?;

    // Verify the account exists and is pending for this instance
    let exists = sqlx::query_scalar!(
        r#"SELECT 1 FROM users
           WHERE account_id = $1 AND instance_id = $2 AND approved_at IS NULL"#,
        account_id,
        instance.id,
    )
    .fetch_optional(&state.db)
    .await?;

    if exists.is_none() {
        return Err(AppError::NotFound);
    }

    // Delete the account (cascades to users via FK)
    sqlx::query!(
        "DELETE FROM accounts WHERE id = $1 AND instance_id = $2",
        account_id,
        instance.id,
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

// ── helpers ────────────────────────────────────────────────────────────────

async fn instance_for_user(state: &AppState, domain: &str, user_id: Uuid) -> AppResult<Instance> {
    sqlx::query_as!(
        Instance,
        "SELECT * FROM instances WHERE domain = $1 AND console_user_id = $2",
        domain,
        user_id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

async fn first_admin_username(state: &AppState, instance_id: Uuid) -> AppResult<Option<String>> {
    let row = sqlx::query!(
        r#"SELECT a.username FROM accounts a
           JOIN users u ON u.account_id = a.id
           WHERE a.instance_id = $1
           ORDER BY u.created_at ASC
           LIMIT 1"#,
        instance_id,
    )
    .fetch_optional(&state.db)
    .await?;

    Ok(row.map(|r| r.username))
}

fn generate_rsa_keypair() -> AppResult<(String, String)> {
    crate::crypto::generate_rsa_keypair()
}

fn hash_password(password: &str) -> AppResult<String> {
    crate::crypto::hash_password(password)
}
