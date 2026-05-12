use axum::{
    extract::{Extension, Form, FromRequest, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    Json,
};
use serde::Deserialize;

/// Extractor that accepts both JSON and form-encoded bodies.
pub struct FormOrJson<T>(pub T);

impl<T, S> FromRequest<S> for FormOrJson<T>
where
    T: serde::de::DeserializeOwned + Send + 'static,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        let is_json = req
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|ct| ct.contains("application/json"))
            .unwrap_or(false);

        if is_json {
            Json::<T>::from_request(req, state)
                .await
                .map(|Json(v)| FormOrJson(v))
                .map_err(IntoResponse::into_response)
        } else {
            Form::<T>::from_request(req, state)
                .await
                .map(|Form(v)| FormOrJson(v))
                .map_err(IntoResponse::into_response)
        }
    }
}

use crate::{
    db::models::OauthApplication,
    error::{AppError, AppResult},
    middleware::ResolvedInstance,
    state::AppState,
};
use super::types::{AppCredentials, CredentialApplication, Token};

// ── GET /api/v1/apps/verify_credentials ───────────────────────────────────

pub async fn verify_app_credentials(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> AppResult<Json<AppCredentials>> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthorized)?;

    let row = sqlx::query!(
        r#"SELECT a.name, a.website
           FROM oauth_access_tokens t
           JOIN oauth_applications a ON a.id = t.application_id
           WHERE t.token = $1 AND t.revoked_at IS NULL
             AND (t.expires_at IS NULL OR t.expires_at > now())"#,
        token,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Unauthorized)?;

    Ok(Json(AppCredentials {
        name: row.name,
        website: row.website,
        vapid_key: None,
    }))
}

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
    FormOrJson(form): FormOrJson<RegisterAppForm>,
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
    let uris: Vec<String> = app.redirect_uris.lines().map(str::to_owned).collect();
    let redirect_uri = uris.first().cloned().unwrap_or_else(|| app.redirect_uris.clone());
    CredentialApplication {
        id: app.id.to_string(),
        name: app.name.clone(),
        website: app.website.clone(),
        scopes: app.scopes.split_whitespace().map(str::to_owned).collect(),
        redirect_uri,
        redirect_uris: uris,
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
    FormOrJson(form): FormOrJson<TokenRequest>,
) -> AppResult<Json<Token>> {
    tracing::info!(
        grant_type = %form.grant_type,
        client_id = %form.client_id,
        instance = %instance.domain,
        "token request",
    );
    // Verify client credentials
    let app = sqlx::query_as!(
        OauthApplication,
        "SELECT * FROM oauth_applications WHERE client_id = $1 AND instance_id = $2",
        form.client_id,
        instance.id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| {
        tracing::warn!(client_id = %form.client_id, instance = %instance.domain, "unknown client_id");
        AppError::Unauthorized
    })?;

    if app.client_secret != form.client_secret {
        tracing::warn!(client_id = %form.client_id, "client_secret mismatch");
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
            .ok_or_else(|| {
                tracing::warn!(code = %code_str, "authorization code not found or expired");
                AppError::Unauthorized
            })?;
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
    FormOrJson(form): FormOrJson<RevokeRequest>,
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

// ── POST /api/:server/login  (Elk single-instance sign-in hook) ────────────

#[derive(Debug, Deserialize)]
pub struct ElkLoginBody {
    pub force_login: Option<bool>,
    pub origin: String,
    pub lang: Option<String>,
}

/// Build the redirect_uri Elk expects: `{origin}/api/{server}/oauth/{encoded_origin}`.
/// This matches Elk's `getRedirectURI(origin, server)` in server/utils/shared.ts.
fn elk_redirect_uri(origin: &str, server: &str) -> String {
    let origin = origin.trim_end_matches('/');
    format!("{}/api/{}/oauth/{}", origin, server, urlencoding::encode(origin))
}

pub async fn elk_login(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Json(body): Json<ElkLoginBody>,
) -> AppResult<Json<String>> {
    let redirect_uri = elk_redirect_uri(&body.origin, &instance.domain);
    let scopes = "read write follow push";

    // Find or create a stable "Elk" OAuth app for this instance, keeping
    // redirect_uri in sync with the current origin.
    let existing = sqlx::query_as!(
        OauthApplication,
        "SELECT * FROM oauth_applications WHERE instance_id = $1 AND name = 'Elk' LIMIT 1",
        instance.id,
    )
    .fetch_optional(&state.db)
    .await?;

    let app = match existing {
        Some(a) if a.redirect_uris == redirect_uri => a,
        Some(a) => {
            // Origin changed (or old entry used /signin/callback) — update in place.
            sqlx::query!(
                "UPDATE oauth_applications SET redirect_uris = $1 WHERE id = $2",
                redirect_uri,
                a.id,
            )
            .execute(&state.db)
            .await?;
            OauthApplication { redirect_uris: redirect_uri.clone(), ..a }
        }
        None => {
            let client_id = generate_token(32);
            let client_secret = generate_token(64);
            sqlx::query_as!(
                OauthApplication,
                r#"INSERT INTO oauth_applications
                     (instance_id, name, client_id, client_secret, redirect_uris, scopes)
                   VALUES ($1, 'Elk', $2, $3, $4, $5)
                   RETURNING *"#,
                instance.id,
                client_id,
                client_secret,
                redirect_uri,
                scopes,
            )
            .fetch_one(&state.db)
            .await?
        }
    };

    let force = body.force_login.unwrap_or(false);
    let lang = body.lang.unwrap_or_default();
    let encoded_redirect = urlencoding::encode(&redirect_uri);
    let encoded_scope = urlencoding::encode(scopes);

    let mut url = format!(
        "https://{}/oauth/authorize?client_id={}&redirect_uri={}&response_type=code&scope={}",
        instance.domain, app.client_id, encoded_redirect, encoded_scope,
    );
    if force {
        url.push_str("&force_login=true");
    }
    if !lang.is_empty() {
        url.push_str(&format!("&lang={}", urlencoding::encode(&lang)));
    }

    Ok(Json(url))
}

// ── GET /api/:server/oauth/:origin  (Elk OAuth callback — server route) ───

#[derive(Debug, Deserialize)]
pub struct ElkOAuthCallbackQuery {
    pub code: Option<String>,
}

pub async fn elk_oauth_callback(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    axum::extract::Path((_server, encoded_origin)): axum::extract::Path<(String, String)>,
    Query(q): Query<ElkOAuthCallbackQuery>,
) -> Response {
    let origin = urlencoding::decode(&encoded_origin)
        .map(|s| s.into_owned())
        .unwrap_or_default();

    let code = match q.code {
        Some(c) => c,
        None => {
            tracing::warn!("elk_oauth_callback: missing code");
            return Redirect::to(&format!("{}/signin/callback?error=missing_code", origin))
                .into_response();
        }
    };

    let app = match sqlx::query_as!(
        OauthApplication,
        "SELECT * FROM oauth_applications WHERE instance_id = $1 AND name = 'Elk' LIMIT 1",
        instance.id,
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    {
        Some(a) => a,
        None => {
            tracing::warn!("elk_oauth_callback: no Elk app found for {}", instance.domain);
            return Redirect::to(&format!("{}/signin/callback?error=no_app", origin))
                .into_response();
        }
    };

    let code_row = sqlx::query!(
        r#"DELETE FROM oauth_authorization_codes
           WHERE code = $1 AND application_id = $2 AND expires_at > now()
           RETURNING account_id, scopes"#,
        code,
        app.id,
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    let Some(code_row) = code_row else {
        tracing::warn!("elk_oauth_callback: code not found or expired");
        return Redirect::to(&format!("{}/signin/callback?error=invalid_code", origin))
            .into_response();
    };

    let Some(account_id) = code_row.account_id else {
        tracing::warn!("elk_oauth_callback: code has no account_id");
        return Redirect::to(&format!("{}/signin/callback?error=no_account", origin))
            .into_response();
    };

    let token_str = generate_token(64);
    let db_ok = sqlx::query!(
        r#"INSERT INTO oauth_access_tokens (application_id, account_id, token, scopes)
           VALUES ($1, $2, $3, $4)"#,
        app.id,
        account_id,
        token_str,
        code_row.scopes,
    )
    .execute(&state.db)
    .await
    .is_ok();

    if !db_ok {
        tracing::error!("elk_oauth_callback: failed to insert access token");
        return Redirect::to(&format!("{}/signin/callback?error=db_error", origin))
            .into_response();
    }

    tracing::info!(
        instance = %instance.domain,
        "elk_oauth_callback: issued token, redirecting to signin/callback"
    );
    let redirect = format!(
        "{}/signin/callback?server={}&token={}",
        origin.trim_end_matches('/'),
        instance.domain,
        token_str,
    );
    Redirect::to(&redirect).into_response()
}

// ── GET /oauth/authorize ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AuthorizeParams {
    pub client_id: String,
    pub redirect_uri: String,
    pub response_type: Option<String>,
    pub scope: Option<String>,
    pub force_login: Option<String>,
    pub lang: Option<String>,
}

pub async fn authorize_form(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Query(params): Query<AuthorizeParams>,
    headers: axum::http::HeaderMap,
) -> Response {
    let app = match sqlx::query_as!(
        OauthApplication,
        "SELECT * FROM oauth_applications WHERE client_id = $1 AND instance_id = $2",
        params.client_id,
        instance.id,
    )
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some(a)) => a,
        _ => return (StatusCode::BAD_REQUEST, "Unknown client_id").into_response(),
    };

    let accept_lang = headers.get("accept-language").and_then(|v| v.to_str().ok());
    let locale = crate::locale::Locale::detect(params.lang.as_deref(), accept_lang);
    let scope = params.scope.as_deref().unwrap_or("read");
    let (toggle_en_url, toggle_ko_url) = authorize_toggle_urls(
        &params.client_id, &params.redirect_uri, scope,
    );
    let html = crate::templates::render("authorize.html", minijinja::context! {
        domain => instance.domain,
        app_name => app.name,
        client_id => params.client_id,
        redirect_uri => params.redirect_uri,
        scope => scope,
        error => "",
        lang => locale.as_str(),
        toggle_en_url => toggle_en_url,
        toggle_ko_url => toggle_ko_url,
        t_sign_in_to => locale.t("sign_in_to"),
        t_authorize => locale.t("authorize"),
        t_email => locale.t("email"),
        t_password => locale.t("password"),
        t_sign_in => locale.t("sign_in"),
    });
    Html(html).into_response()}

fn authorize_toggle_urls(client_id: &str, redirect_uri: &str, scope: &str) -> (String, String) {
    let enc_redirect = urlencoding::encode(redirect_uri);
    let enc_scope = urlencoding::encode(scope);
    let base = format!(
        "/oauth/authorize?client_id={}&redirect_uri={}&scope={}",
        client_id, enc_redirect, enc_scope,
    );
    (format!("{}&lang=en", base), format!("{}&lang=ko", base))
}


// ── POST /oauth/authorize ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AuthorizeForm {
    pub client_id: String,
    pub redirect_uri: String,
    pub scope: Option<String>,
    pub email: String,
    pub password: String,
    pub lang: Option<String>,
}

pub async fn authorize_submit(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Form(form): Form<AuthorizeForm>,
) -> Response {
    let locale = crate::locale::Locale::detect(form.lang.as_deref(), None);
    let app_name = sqlx::query_scalar!(
        "SELECT name FROM oauth_applications WHERE client_id = $1 AND instance_id = $2",
        form.client_id,
        instance.id,
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .unwrap_or_else(|| form.client_id.clone());
    let result = do_authorize(&state, &instance, &form).await;
    match result {
        Ok(redirect_url) => Redirect::to(&redirect_url).into_response(),
        Err(_) => {
            let scope = form.scope.as_deref().unwrap_or("read");
            let (toggle_en_url, toggle_ko_url) = authorize_toggle_urls(
                &form.client_id, &form.redirect_uri, scope,
            );
            let html = crate::templates::render("authorize.html", minijinja::context! {
                domain => instance.domain,
                app_name => app_name,
                client_id => form.client_id,
                redirect_uri => form.redirect_uri,
                scope => scope,
                error => locale.t("invalid_credentials"),
                lang => locale.as_str(),
                toggle_en_url => toggle_en_url,
                toggle_ko_url => toggle_ko_url,
                t_sign_in_to => locale.t("sign_in_to"),
                t_authorize => locale.t("authorize"),
                t_email => locale.t("email"),
                t_password => locale.t("password"),
                t_sign_in => locale.t("sign_in"),
            });
            Html(html).into_response()
        }
    }
}

async fn do_authorize(
    state: &AppState,
    instance: &crate::db::models::Instance,
    form: &AuthorizeForm,
) -> Result<String, String> {
    let app = sqlx::query_as!(
        OauthApplication,
        "SELECT * FROM oauth_applications WHERE client_id = $1 AND instance_id = $2",
        form.client_id,
        instance.id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| "Database error".to_string())?
    .ok_or_else(|| "Unknown application".to_string())?;

    let user = sqlx::query!(
        r#"SELECT u.id, u.password_hash, u.account_id
           FROM users u
           JOIN accounts a ON a.id = u.account_id
           WHERE u.email_normalized = lower($1)
             AND u.instance_id = $2
             AND u.confirmed_at IS NOT NULL"#,
        form.email,
        instance.id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| "Database error".to_string())?
    .ok_or_else(|| "Invalid email or password".to_string())?;

    verify_password(&form.password, &user.password_hash)
        .map_err(|_| "Invalid email or password".to_string())?;

    let scopes = form.scope.clone().unwrap_or_else(|| app.scopes.clone());
    let code = generate_token(32);
    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(10);

    sqlx::query!(
        r#"INSERT INTO oauth_authorization_codes
             (application_id, account_id, code, redirect_uri, scopes, expires_at)
           VALUES ($1, $2, $3, $4, $5, $6)"#,
        app.id,
        user.account_id,
        code,
        form.redirect_uri,
        scopes,
        expires_at,
    )
    .execute(&state.db)
    .await
    .map_err(|_| "Database error".to_string())?;

    let sep = if form.redirect_uri.contains('?') { '&' } else { '?' };
    Ok(format!("{}{}code={}", form.redirect_uri, sep, code))
}

