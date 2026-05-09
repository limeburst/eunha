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
    use rsa::RsaPrivateKey;
    use rsa::pkcs8::EncodePrivateKey;
    use pkcs8::spki::EncodePublicKey;
    use pkcs8::LineEnding;
    use rsa::rand_core::OsRng;

    let priv_key = RsaPrivateKey::new(&mut OsRng, 2048)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("RSA keygen failed: {e}")))?;

    let priv_doc = priv_key
        .to_pkcs8_pem(LineEnding::LF)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("PKCS8 encode failed: {e}")))?;
    let priv_pem = std::str::from_utf8(priv_doc.as_bytes())
        .map_err(|e| AppError::Internal(anyhow::anyhow!("PEM UTF-8: {e}")))?
        .to_string();

    let pub_pem = priv_key
        .to_public_key()
        .to_public_key_pem(LineEnding::LF)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("SPKI encode failed: {e}")))?;

    Ok((priv_pem, pub_pem))
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
