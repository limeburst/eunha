use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    db::models::ConsoleUser,
    error::{AppError, AppResult},
    state::AppState,
};
use super::ConsoleAuth;

#[derive(Debug, Deserialize)]
pub struct AuthRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum AuthResponse {
    Confirmed { token: String, user: UserResponse },
    NeedsConfirmation { needs_confirmation: bool },
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

pub async fn signup(
    State(state): State<AppState>,
    Json(body): Json<AuthRequest>,
) -> AppResult<Json<AuthResponse>> {
    if body.password.len() < 8 {
        return Err(AppError::Unprocessable("Password must be at least 8 characters".into()));
    }

    let email = body.email.trim();
    let email_normalized = email.to_lowercase();

    let exists = sqlx::query_scalar!(
        "SELECT 1 FROM console_users WHERE email_normalized = $1",
        email_normalized
    )
    .fetch_optional(&state.db)
    .await?;

    if exists.is_some() {
        return Err(AppError::Conflict);
    }

    let password_hash = hash_password(&body.password)?;

    if let Some(ref resend) = state.config.resend {
        let confirmation_token = generate_token(64);
        let user = sqlx::query_as!(
            ConsoleUser,
            r#"INSERT INTO console_users (email, email_normalized, password_hash, confirmation_token)
               VALUES ($1, $2, $3, $4)
               RETURNING *"#,
            email,
            email_normalized,
            password_hash,
            confirmation_token,
        )
        .fetch_one(&state.db)
        .await?;

        let confirm_url = format!(
            "https://{}/api/console/auth/confirm?token={}",
            state.config.console_domain, confirmation_token
        );
        let http = state.http.clone();
        let api_key = resend.api_key.clone();
        let from = resend.from.clone();
        let to = user.email.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::email::send_confirmation(
                &http, &api_key, &from, &to, &to, &confirm_url,
            )
            .await
            {
                tracing::error!(error = %e, "failed to send console confirmation email");
            }
        });

        return Ok(Json(AuthResponse::NeedsConfirmation { needs_confirmation: true }));
    }

    let user = sqlx::query_as!(
        ConsoleUser,
        r#"INSERT INTO console_users (email, email_normalized, password_hash, confirmed_at)
           VALUES ($1, $2, $3, now())
           RETURNING *"#,
        email,
        email_normalized,
        password_hash,
    )
    .fetch_one(&state.db)
    .await?;

    let token = issue_session(&state, user.id).await?;
    Ok(Json(AuthResponse::Confirmed { token, user: UserResponse::from(&user) }))
}

pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<AuthRequest>,
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

    verify_password(&body.password, &user.password_hash)?;

    let token = issue_session(&state, user.id).await?;
    Ok(Json(AuthResponse::Confirmed { token, user: UserResponse::from(&user) }))
}

// ── GET /api/console/auth/confirm ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ConfirmQuery {
    pub token: String,
}

pub async fn confirm(
    State(state): State<AppState>,
    Query(q): Query<ConfirmQuery>,
) -> Response {
    let user = sqlx::query_as!(
        ConsoleUser,
        r#"UPDATE console_users
           SET confirmed_at = now(), confirmation_token = NULL
           WHERE confirmation_token = $1 AND confirmed_at IS NULL
           RETURNING *"#,
        q.token,
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    let Some(user) = user else {
        return (
            StatusCode::NOT_FOUND,
            Html("<h1>Invalid confirmation link</h1><p>This link may have already been used or is invalid.</p>".to_string()),
        )
            .into_response();
    };

    let token = match issue_session(&state, user.id).await {
        Ok(t) => t,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Session creation failed").into_response();
        }
    };

    let user_json = serde_json::to_string(&UserResponse::from(&user)).unwrap_or_default();
    let escaped_user = user_json.replace('\\', "\\\\").replace('\'', "\\'");

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head><title>Email confirmed</title></head>
<body>
<p>Confirming your account…</p>
<script>
  localStorage.setItem('console_token', '{token}');
  localStorage.setItem('console_user', '{escaped_user}');
  window.location.replace('/');
</script>
</body>
</html>"#
    );
    Html(html).into_response()
}

pub async fn me(ConsoleAuth(user): ConsoleAuth) -> AppResult<Json<UserResponse>> {
    Ok(Json(UserResponse::from(&user)))
}

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
    verify_password(&body.current_password, &user.password_hash)?;

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
