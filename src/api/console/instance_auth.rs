use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    state::AppState,
};
use super::InstanceUserAuth;

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub domain: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: InstanceUserResponse,
}

#[derive(Debug, Serialize)]
pub struct InstanceUserResponse {
    pub username: String,
    pub instance_domain: String,
}

pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> AppResult<Json<LoginResponse>> {
    let domain = body.domain.trim().to_lowercase();
    let email_normalized = body.email.trim().to_lowercase();

    let row = sqlx::query!(
        r#"SELECT u.id, u.password_hash, a.username
           FROM users u
           JOIN accounts a ON a.id = u.account_id
           JOIN instances inst ON inst.id = u.instance_id
           WHERE (inst.domain = $1 OR inst.custom_domain = $1)
             AND u.email_normalized = $2
             AND u.confirmed_at IS NOT NULL
             AND a.domain IS NULL"#,
        domain,
        email_normalized,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Unauthorized)?;

    verify_password(&body.password, &row.password_hash)?;

    let token = generate_token(64);
    sqlx::query!(
        "INSERT INTO instance_user_sessions (user_id, token) VALUES ($1, $2)",
        row.id,
        token,
    )
    .execute(&state.db)
    .await?;

    Ok(Json(LoginResponse {
        token,
        user: InstanceUserResponse {
            username: row.username,
            instance_domain: domain,
        },
    }))
}

pub async fn me(
    InstanceUserAuth(auth): InstanceUserAuth,
) -> AppResult<Json<InstanceUserResponse>> {
    Ok(Json(InstanceUserResponse {
        username: auth.username,
        instance_domain: auth.instance_domain,
    }))
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

pub async fn change_password(
    State(state): State<AppState>,
    InstanceUserAuth(auth): InstanceUserAuth,
    Json(body): Json<ChangePasswordRequest>,
) -> AppResult<axum::http::StatusCode> {
    let row = sqlx::query!(
        "SELECT password_hash FROM users WHERE id = $1",
        auth.user_id,
    )
    .fetch_one(&state.db)
    .await?;

    verify_password(&body.current_password, &row.password_hash)?;

    if body.new_password.len() < 8 {
        return Err(AppError::Unprocessable("Password must be at least 8 characters".into()));
    }

    let new_hash = hash_password(&body.new_password)?;
    sqlx::query!(
        "UPDATE users SET password_hash = $1, updated_at = now() WHERE id = $2",
        new_hash,
        auth.user_id,
    )
    .execute(&state.db)
    .await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ── Invite tree ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct InviteTreeResponse {
    pub members: Vec<InviteTreeMember>,
    pub invites: Vec<InviteResponse>,
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
pub struct InviteResponse {
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

pub async fn invite_tree(
    State(state): State<AppState>,
    InstanceUserAuth(auth): InstanceUserAuth,
) -> AppResult<Json<InviteTreeResponse>> {
    let members = sqlx::query!(
        r#"SELECT
             a.id           AS account_id,
             a.username,
             u.invite_id    AS "invite_id?: Uuid",
             inv_a.id       AS "invited_by_account_id?: Uuid",
             inv_a.username AS "invited_by_username?: String",
             u.created_at
           FROM users u
           JOIN accounts a ON a.id = u.account_id
           LEFT JOIN invites i ON i.id = u.invite_id
           LEFT JOIN accounts inv_a ON inv_a.id = i.created_by
           WHERE u.instance_id = $1 AND a.domain IS NULL
           ORDER BY u.created_at ASC"#,
        auth.instance_id,
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
        auth.instance_id,
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|r| InviteResponse {
        url: crate::api::mastodon::invites::invite_url(&auth.instance_domain, &r.code),
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

// ── Helpers ────────────────────────────────────────────────────────────────

fn hash_password(password: &str) -> AppResult<String> {
    use argon2::{Argon2, PasswordHasher};
    use argon2::password_hash::{rand_core::OsRng, SaltString};
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(anyhow::anyhow!("password hashing failed: {e}")))
}

fn verify_password(password: &str, hash: &str) -> AppResult<()> {
    use argon2::PasswordVerifier;
    let parsed = argon2::PasswordHash::new(hash)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("invalid password hash")))?;
    argon2::Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| AppError::Unauthorized)
}

fn generate_token(len: usize) -> String {
    use rand::RngCore;
    let mut rng = rand::rng();
    (0..len)
        .map(|_| format!("{:02x}", rng.next_u32() as u8))
        .collect()
}
