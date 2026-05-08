use axum::{
    extract::{Extension, Form, State},
    Json,
};
use serde::Deserialize;

use crate::{
    db::models::{OauthApplication, OauthAccessToken},
    error::{AppError, AppResult},
    middleware::ResolvedInstance,
    state::AppState,
};
use super::types::{CredentialApplication, Token};

// ── POST /api/v1/apps ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RegisterAppForm {
    pub client_name: String,
    pub redirect_uris: Option<String>,
    pub scopes: Option<String>,
    pub website: Option<String>,
}

pub async fn register_app(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Form(form): Form<RegisterAppForm>,
) -> AppResult<Json<CredentialApplication>> {
    let client_id = generate_token(32);
    let client_secret = generate_token(64);
    let redirect_uris = form.redirect_uris.unwrap_or_else(|| "urn:ietf:wg:oauth:2.0:oob".into());
    let scopes = form.scopes.unwrap_or_else(|| "read".into());

    let app = sqlx::query_as!(
        OauthApplication,
        r#"INSERT INTO oauth_applications
             (instance_id, name, client_id, client_secret, redirect_uris, scopes, website)
           VALUES ($1,$2,$3,$4,$5,$6,$7)
           RETURNING *"#,
        instance.id,
        form.client_name,
        client_id,
        client_secret,
        redirect_uris,
        scopes,
        form.website,
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(app_to_credential(&app)))
}

fn app_to_credential(app: &OauthApplication) -> CredentialApplication {
    CredentialApplication {
        id: app.id.to_string(),
        name: app.name.clone(),
        website: app.website.clone(),
        scopes: app.scopes.split_whitespace().map(str::to_owned).collect(),
        redirect_uris: app.redirect_uris.lines().map(str::to_owned).collect(),
        client_id: app.client_id.clone(),
        client_secret: app.client_secret.clone(),
        vapid_key: None,
    }
}

// ── POST /oauth/token ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct TokenRequest {
    pub grant_type: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: Option<String>,
    pub code: Option<String>,
    pub scope: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

pub async fn issue_token(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Form(form): Form<TokenRequest>,
) -> AppResult<Json<Token>> {
    // Verify client credentials
    let app = sqlx::query_as!(
        OauthApplication,
        "SELECT * FROM oauth_applications WHERE client_id = $1 AND instance_id = $2",
        form.client_id,
        instance.id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Unauthorized)?;

    if app.client_secret != form.client_secret {
        return Err(AppError::Unauthorized);
    }

    let (account_id, scopes) = match form.grant_type.as_str() {
        "client_credentials" => (None, app.scopes.clone()),

        "authorization_code" => {
            let code_str = form.code.as_deref().ok_or(AppError::Unprocessable("missing code".into()))?;
            let code = sqlx::query!(
                r#"DELETE FROM oauth_authorization_codes
                   WHERE code = $1 AND application_id = $2 AND expires_at > now()
                   RETURNING account_id, scopes"#,
                code_str,
                app.id,
            )
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::Unauthorized)?;
            (code.account_id, code.scopes)
        }

        "password" => {
            let username = form.username.as_deref().ok_or(AppError::Unprocessable("missing username".into()))?;
            let password = form.password.as_deref().ok_or(AppError::Unprocessable("missing password".into()))?;
            let user = sqlx::query!(
                r#"SELECT u.id, u.password_hash, u.account_id
                   FROM users u
                   JOIN accounts a ON a.id = u.account_id
                   WHERE u.email_normalized = lower($1)
                     AND u.instance_id = $2
                     AND u.confirmed_at IS NOT NULL"#,
                username,
                instance.id,
            )
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::Unauthorized)?;

            verify_password(password, &user.password_hash)?;

            (Some(user.account_id), form.scope.unwrap_or_else(|| app.scopes.clone()))
        }

        _ => return Err(AppError::Unprocessable("unsupported grant_type".into())),
    };

    let token_str = generate_token(64);
    let created_at = chrono::Utc::now();

    sqlx::query!(
        r#"INSERT INTO oauth_access_tokens (application_id, account_id, token, scopes)
           VALUES ($1, $2, $3, $4)"#,
        app.id,
        account_id,
        token_str,
        scopes,
    )
    .execute(&state.db)
    .await?;

    Ok(Json(Token {
        access_token: token_str,
        token_type: "Bearer".to_string(),
        scope: scopes,
        created_at: created_at.timestamp(),
    }))
}

// ── POST /oauth/revoke ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RevokeRequest {
    pub client_id: String,
    pub client_secret: String,
    pub token: String,
}

pub async fn revoke_token(
    State(state): State<AppState>,
    Form(form): Form<RevokeRequest>,
) -> AppResult<Json<serde_json::Value>> {
    sqlx::query!(
        r#"UPDATE oauth_access_tokens SET revoked_at = now()
           WHERE token = $1 AND revoked_at IS NULL"#,
        form.token,
    )
    .execute(&state.db)
    .await?;
    Ok(Json(serde_json::json!({})))
}

fn verify_password(password: &str, hash: &str) -> Result<(), AppError> {
    if hash.starts_with("$2a$") || hash.starts_with("$2b$") || hash.starts_with("$2y$") {
        bcrypt::verify(password, hash)
            .map_err(|_| AppError::Internal(anyhow::anyhow!("bcrypt error")))?
            .then_some(())
            .ok_or(AppError::Unauthorized)
    } else {
        let parsed = argon2::PasswordHash::new(hash)
            .map_err(|_| AppError::Internal(anyhow::anyhow!("invalid password hash")))?;
        use argon2::PasswordVerifier;
        argon2::Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .map_err(|_| AppError::Unauthorized)
    }
}

fn generate_token(len: usize) -> String {
    use rand::RngCore;
    let mut rng = rand::rng();
    (0..len)
        .map(|_| format!("{:02x}", rng.next_u32() as u8))
        .collect()
}
