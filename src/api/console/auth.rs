use axum::{
    extract::State,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    db::models::ConsoleUser,
    error::{AppError, AppResult},
    state::AppState,
};
use super::ConsoleAuth;

// ── Response types ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum SignupResponse {
    NeedsConfirmation { needs_confirmation: bool },
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserResponse,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: String,
    pub email: String,
    pub locale: String,
    pub created_at: String,
}

impl From<&ConsoleUser> for UserResponse {
    fn from(u: &ConsoleUser) -> Self {
        UserResponse {
            id: u.id.to_string(),
            email: u.email.clone(),
            locale: u.locale.clone(),
            created_at: u.created_at.to_rfc3339(),
        }
    }
}

// ── POST /api/console/auth/signup ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SignupRequest {
    pub email: String,
    pub locale: Option<String>,
}

pub async fn signup(
    State(state): State<AppState>,
    Json(body): Json<SignupRequest>,
) -> AppResult<Json<SignupResponse>> {
    let email = body.email.trim().to_string();
    let email_normalized = email.to_lowercase();
    let locale_str = body.locale.clone().unwrap_or_else(|| "en".into());
    let confirmation_token = generate_token(64);

    let existing = sqlx::query_as!(
        ConsoleUser,
        "SELECT * FROM console_users WHERE email_normalized = $1",
        email_normalized,
    )
    .fetch_optional(&state.db)
    .await?;

    let user = if let Some(u) = existing {
        if u.confirmed_at.is_some() {
            // Confirmed account — don't reveal it exists; just no-op and return needs_confirmation
            // so the form shows the same message regardless (prevents enumeration).
            return Ok(Json(SignupResponse::NeedsConfirmation { needs_confirmation: true }));
        }
        // Unconfirmed: regenerate token and resend
        sqlx::query_as!(
            ConsoleUser,
            "UPDATE console_users SET confirmation_token = $1 WHERE id = $2 RETURNING *",
            confirmation_token,
            u.id,
        )
        .fetch_one(&state.db)
        .await?
    } else {
        sqlx::query_as!(
            ConsoleUser,
            r#"INSERT INTO console_users (email, email_normalized, confirmation_token)
               VALUES ($1, $2, $3)
               RETURNING *"#,
            email,
            email_normalized,
            confirmation_token,
        )
        .fetch_one(&state.db)
        .await?
    };

    if let Some(ref resend) = state.config.resend {
        let confirm_url = format!(
            "https://{}/confirm-account?token={}",
            state.config.console_domain, confirmation_token
        );
        let http = state.http.clone();
        let api_key = resend.api_key.clone();
        let from = resend.from.clone();
        let to = user.email.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::email::send_confirmation(
                &http, &api_key, &from, &to, &to, &confirm_url, &locale_str,
            )
            .await
            {
                tracing::error!(error = %e, "failed to send console confirmation email");
            }
        });
    }

    Ok(Json(SignupResponse::NeedsConfirmation { needs_confirmation: true }))
}

// ── POST /api/console/auth/confirm ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ConfirmRequest {
    pub token: String,
    pub password: String,
}

pub async fn confirm(
    State(state): State<AppState>,
    Json(body): Json<ConfirmRequest>,
) -> AppResult<Json<AuthResponse>> {
    if body.password.len() < 8 {
        return Err(AppError::Unprocessable("Password must be at least 8 characters".into()));
    }

    let password_hash = hash_password(&body.password)?;

    let user = sqlx::query_as!(
        ConsoleUser,
        r#"UPDATE console_users
           SET confirmed_at = now(), confirmation_token = NULL, password_hash = $2
           WHERE confirmation_token = $1 AND confirmed_at IS NULL
           RETURNING *"#,
        body.token,
        password_hash,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let token = issue_session(&state, user.id).await?;
    Ok(Json(AuthResponse { token, user: UserResponse::from(&user) }))
}

// ── POST /api/console/auth/login ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> AppResult<Json<AuthResponse>> {
    let email_normalized = body.email.trim().to_lowercase();

    let user = sqlx::query_as!(
        ConsoleUser,
        "SELECT * FROM console_users WHERE email_normalized = $1 AND confirmed_at IS NOT NULL",
        email_normalized,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Unauthorized)?;

    let hash = user.password_hash.as_deref().ok_or(AppError::Unauthorized)?;
    verify_password(&body.password, hash)?;

    let token = issue_session(&state, user.id).await?;
    Ok(Json(AuthResponse { token, user: UserResponse::from(&user) }))
}

// ── GET /api/console/auth/me ───────────────────────────────────────────────

pub async fn me(ConsoleAuth(user): ConsoleAuth) -> AppResult<Json<UserResponse>> {
    Ok(Json(UserResponse::from(&user)))
}

// ── PATCH /api/console/auth/password ──────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

pub async fn change_password(
    State(state): State<AppState>,
    ConsoleAuth(user): ConsoleAuth,
    Json(body): Json<ChangePasswordRequest>,
) -> AppResult<axum::http::StatusCode> {
    let hash = user.password_hash.as_deref().ok_or(AppError::Unauthorized)?;
    verify_password(&body.current_password, hash)?;

    if body.new_password.len() < 8 {
        return Err(AppError::Unprocessable("Password must be at least 8 characters".into()));
    }

    let new_hash = hash_password(&body.new_password)?;

    sqlx::query!(
        "UPDATE console_users SET password_hash = $1, updated_at = now() WHERE id = $2",
        new_hash,
        user.id,
    )
    .execute(&state.db)
    .await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ── PATCH /api/console/auth/locale ────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UpdateLocaleRequest {
    pub locale: String,
}

pub async fn update_locale(
    State(state): State<AppState>,
    ConsoleAuth(user): ConsoleAuth,
    Json(body): Json<UpdateLocaleRequest>,
) -> AppResult<axum::http::StatusCode> {
    let allowed = ["en", "ko"];
    if !allowed.contains(&body.locale.as_str()) {
        return Err(AppError::Unprocessable("Unsupported locale".into()));
    }

    sqlx::query!(
        "UPDATE console_users SET locale = $1, updated_at = now() WHERE id = $2",
        body.locale,
        user.id,
    )
    .execute(&state.db)
    .await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ── helpers ────────────────────────────────────────────────────────────────

async fn issue_session(state: &AppState, user_id: uuid::Uuid) -> AppResult<String> {
    let token = generate_token(64);
    sqlx::query!(
        "INSERT INTO console_sessions (console_user_id, token) VALUES ($1, $2)",
        user_id,
        token,
    )
    .execute(&state.db)
    .await?;
    Ok(token)
}

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
