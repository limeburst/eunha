use axum::{
    extract::{Extension, Form, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Json, Redirect, Response},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    crypto,
    db::models::Instance,
    error::{AppError, AppResult},
    middleware::ResolvedInstance,
    state::AppState,
    templates,
};

use urlencoding;

#[derive(Debug, Deserialize)]
pub struct SignUpQuery {
    invite: Option<String>,
    lang: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SignUpForm {
    username: Option<String>,
    email: Option<String>,
    password: Option<String>,
    password_confirmation: Option<String>,
    invite: Option<String>,
    lang: Option<String>,
    reason: Option<String>,
}

pub async fn signup_get(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Query(q): Query<SignUpQuery>,
    headers: axum::http::HeaderMap,
) -> Response {
    let invite = q.invite.as_deref().unwrap_or("").trim().to_string();
    let accept_lang = headers.get("accept-language").and_then(|v| v.to_str().ok());
    let locale = crate::locale::Locale::detect(q.lang.as_deref(), accept_lang);

    if !instance.registrations_open {
        if invite.is_empty() {
            return render(&instance, &invite, false, false, None, locale);
        }
        if let Err(msg) = validate_invite(&state, &instance, &invite).await {
            return render(&instance, &invite, false, false, Some(locale.t(msg)), locale);
        }
    }

    render(&instance, &invite, true, false, None, locale)
}

pub async fn signup_post(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Form(form): Form<SignUpForm>,
) -> Response {
    let invite = form.invite.as_deref().unwrap_or("").trim().to_string();
    let locale = crate::locale::Locale::detect(form.lang.as_deref(), None);

    // Check registrations / invite — always validate a provided code; require
    // one when registrations are closed.
    let invite_id: Option<uuid::Uuid> = if !invite.is_empty() {
        match validate_invite(&state, &instance, &invite).await {
            Ok(id) => Some(id),
            Err(key) => {
                let show_form = instance.registrations_open;
                return render(&instance, &invite, show_form, false, Some(locale.t(key)), locale);
            }
        }
    } else if !instance.registrations_open {
        return render(&instance, &invite, false, false, Some(locale.t("err_invite_required")), locale);
    } else {
        None
    };

    // Unwrap fields — if any are missing the browser should have caught it, but
    // guard anyway to avoid a confusing error.
    let username = form.username.as_deref().unwrap_or("").trim().to_lowercase();
    let email = form.email.as_deref().unwrap_or("").trim().to_string();
    let password = form.password.as_deref().unwrap_or("");
    let confirm = form.password_confirmation.as_deref().unwrap_or("");

    // Validate
    if username.is_empty()
        || !username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return render(&instance, &invite, true, false, Some(locale.t("err_username_chars")), locale);
    }
    if email.is_empty() || !email.contains('@') {
        return render(&instance, &invite, true, false, Some(locale.t("err_invalid_email")), locale);
    }
    if password.len() < 8 {
        return render(&instance, &invite, true, false, Some(locale.t("err_password_short")), locale);
    }
    if password != confirm {
        return render(&instance, &invite, true, false, Some(locale.t("err_password_mismatch")), locale);
    }

    let email_normalised = email.to_lowercase();

    // Check uniqueness
    let username_taken = sqlx::query_scalar!(
        "SELECT 1 FROM accounts WHERE username = $1 AND instance_id = $2 AND domain IS NULL",
        username,
        instance.id,
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .is_some();

    if username_taken {
        return render(&instance, &invite, true, false, Some(locale.t("err_username_taken")), locale);
    }

    let email_taken = sqlx::query_scalar!(
        "SELECT 1 FROM users WHERE email_normalized = $1 AND instance_id = $2",
        email_normalised,
        instance.id,
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .is_some();

    if email_taken {
        return render(&instance, &invite, true, false, Some(locale.t("err_email_taken")), locale);
    }

    // Create account
    let (private_key, public_key) = match crypto::generate_rsa_keypair() {
        Ok(kp) => kp,
        Err(_) => return render(&instance, &invite, true, false, Some(locale.t("err_server")), locale),
    };

    let base_url = format!("https://{}", instance.domain);
    let uri = format!("{}/users/{}", base_url, username);
    let url = format!("{}/@{}", base_url, username);
    let inbox_url = format!("{}/inbox", uri);
    let outbox_url = format!("{}/outbox", uri);
    let shared_inbox_url = format!("https://{}/inbox", instance.domain);

    let account_id = sqlx::query_scalar!(
        r#"INSERT INTO accounts
             (instance_id, username, url, uri, private_key, public_key,
              inbox_url, outbox_url, shared_inbox_url)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
           RETURNING id"#,
        instance.id,
        username,
        url,
        uri,
        private_key,
        public_key,
        inbox_url,
        outbox_url,
        shared_inbox_url,
    )
    .fetch_one(&state.db)
    .await;

    let account_id = match account_id {
        Ok(id) => id,
        Err(_) => return render(&instance, &invite, true, false, Some(locale.t("err_server")), locale),
    };

    let password_hash = match crypto::hash_password(password) {
        Ok(h) => h,
        Err(_) => return render(&instance, &invite, true, false, Some(locale.t("err_server")), locale),
    };

    // Pending when approval_required and no invite was used (invites bypass approval)
    let needs_approval = instance.approval_required && invite_id.is_none();
    let reason = form.reason.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);

    let user_result = sqlx::query!(
        r#"INSERT INTO users
             (account_id, instance_id, email, email_normalized, password_hash, confirmed_at, invite_id,
              approved_at, reason)
           VALUES ($1,$2,$3,$4,$5,now(),$6,
                   CASE WHEN $7 THEN NULL ELSE now() END,
                   $8)"#,
        account_id,
        instance.id,
        email,
        email_normalised,
        password_hash,
        invite_id,
        needs_approval,
        reason,
    )
    .execute(&state.db)
    .await;

    if user_result.is_err() {
        return render(&instance, &invite, true, false, Some(locale.t("err_server")), locale);
    }

    // Increment invite uses (always, so the tree is accurate even with open registrations)
    if let Some(id) = invite_id {
        let _ = sqlx::query!(
            "UPDATE invites SET uses = uses + 1 WHERE id = $1",
            id,
        )
        .execute(&state.db)
        .await;
    }

    if needs_approval {
        return render(&instance, &invite, false, true, None, locale);
    }

    // Redirect to Elk's sign-in page
    (StatusCode::SEE_OTHER, [(header::LOCATION, "/auth/sign_in")]).into_response()
}

// ── helpers ────────────────────────────────────────────────────────────────

/// Validates an invite code and returns its UUID if valid.
/// On error, returns a locale key suitable for passing to `locale.t()`.
async fn validate_invite(
    state: &AppState,
    instance: &Instance,
    code: &str,
) -> Result<uuid::Uuid, &'static str> {
    let row = sqlx::query!(
        "SELECT id, uses, max_uses, expires_at FROM invites WHERE code = $1 AND instance_id = $2",
        code,
        instance.id,
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    let Some(inv) = row else {
        return Err("err_invalid_invite");
    };
    if inv.max_uses.map_or(false, |m| inv.uses >= m) {
        return Err("err_invite_maxed");
    }
    if inv.expires_at.map_or(false, |e| e < chrono::Utc::now()) {
        return Err("err_invite_expired");
    }
    Ok(inv.id)
}

// ── POST /api/v1/accounts ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ApiCreateAccountForm {
    pub username: String,
    pub email: String,
    pub password: String,
    pub agreement: Option<bool>,
    pub locale: Option<String>,
    pub reason: Option<String>,
    pub invite_code: Option<String>,
}

pub async fn api_create_account(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    req_headers: HeaderMap,
    super::oauth::FormOrJson(form): super::oauth::FormOrJson<ApiCreateAccountForm>,
) -> AppResult<Json<super::types::Token>> {
    let invite_code = form.invite_code.as_deref().unwrap_or("").trim().to_string();
    let invite_id: Option<Uuid> = if !invite_code.is_empty() {
        Some(validate_invite(&state, &instance, &invite_code).await
            .map_err(|_| AppError::Unprocessable("Invalid or expired invite code".into()))?)
    } else if !instance.registrations_open {
        return Err(AppError::Unprocessable("This instance is not open for registration".into()));
    } else {
        None
    };

    let username = form.username.trim().to_lowercase();
    let email = form.email.trim().to_string();
    let password = &form.password;

    if username.is_empty() || !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(AppError::Unprocessable("Username can only contain letters, numbers, and underscores".into()));
    }
    if !email.contains('@') {
        return Err(AppError::Unprocessable("Invalid email address".into()));
    }
    if password.len() < 8 {
        return Err(AppError::Unprocessable("Password must be at least 8 characters".into()));
    }

    let email_normalized = email.to_lowercase();

    let username_taken = sqlx::query_scalar!(
        "SELECT 1 FROM accounts WHERE username = $1 AND instance_id = $2 AND domain IS NULL",
        username, instance.id,
    ).fetch_optional(&state.db).await?.is_some();
    if username_taken {
        return Err(AppError::Unprocessable("Username is already taken".into()));
    }

    let existing_email = sqlx::query!(
        "SELECT account_id, confirmation_token, email FROM users WHERE email_normalized = $1 AND instance_id = $2",
        email_normalized, instance.id,
    ).fetch_optional(&state.db).await?;

    if let Some(ref row) = existing_email {
        // Confirmed account → hard conflict
        if row.confirmation_token.is_none() {
            return Err(AppError::Unprocessable("Email is already taken".into()));
        }
        // Unconfirmed → resend and return a fresh profile token
        if let Some(ref resend) = state.config.resend {
            let tok = row.confirmation_token.clone().unwrap();
            let domain = instance.custom_domain.as_deref().unwrap_or(&instance.domain);
            let confirm_url = format!("https://{}/auth/confirm?token={}", domain, tok);
            let http = state.http.clone();
            let api_key = resend.api_key.clone();
            let from = resend.from.clone();
            let to_addr = row.email.clone();
            let uname = username.clone();
            let locale_str = form.locale.clone().unwrap_or_else(|| "en".into());
            tokio::spawn(async move {
                if let Err(e) = crate::email::send_confirmation(
                    &http, &api_key, &from, &to_addr, &uname, "", &confirm_url, &locale_str,
                ).await {
                    tracing::error!(error = %e, "failed to resend confirmation email");
                }
            });
            let app_id = extract_app_from_bearer(&state, &req_headers).await;
            let token_str = api_generate_token();
            sqlx::query!(
                r#"INSERT INTO oauth_access_tokens (application_id, account_id, token, scopes)
                   VALUES ($1, $2, $3, 'profile')"#,
                app_id, row.account_id, token_str,
            ).execute(&state.db).await?;
            return Ok(Json(super::types::Token {
                access_token: token_str,
                token_type: "Bearer".to_string(),
                scope: "profile".to_string(),
                created_at: chrono::Utc::now().timestamp(),
            }));
        }
        return Err(AppError::Unprocessable("Email is already taken".into()));
    }

    let (private_key, public_key) = crypto::generate_rsa_keypair()
        .map_err(|_| AppError::Internal(anyhow::anyhow!("key generation failed")))?;

    let domain = instance.custom_domain.as_deref().unwrap_or(&instance.domain);
    let base_url = format!("https://{}", domain);
    let uri = format!("https://{}/users/{}", instance.domain, username);
    let url = format!("{}/@{}", base_url, username);

    let account_id = sqlx::query_scalar!(
        r#"INSERT INTO accounts
             (instance_id, username, url, uri, private_key, public_key,
              inbox_url, outbox_url, shared_inbox_url)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
           RETURNING id"#,
        instance.id, username, url, uri, private_key, public_key,
        format!("{}/inbox", uri),
        format!("{}/outbox", uri),
        format!("https://{}/inbox", instance.domain),
    ).fetch_one(&state.db).await
        .map_err(|_| AppError::Internal(anyhow::anyhow!("account creation failed")))?;

    let password_hash = crypto::hash_password(password)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("password hashing failed")))?;

    let api_needs_approval = instance.approval_required && invite_id.is_none();
    let api_reason = form.reason.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);

    let (confirmation_token, scopes) = if state.config.resend.is_some() {
        (Some(api_generate_token()), "profile")
    } else {
        (None, "read write follow push")
    };

    sqlx::query!(
        r#"INSERT INTO users
             (account_id, instance_id, email, email_normalized, password_hash, confirmed_at, invite_id,
              approved_at, reason, confirmation_token)
           VALUES ($1,$2,$3,$4,$5,
                   CASE WHEN $9::TEXT IS NULL THEN now() ELSE NULL END,
                   $6,
                   CASE WHEN $7 THEN NULL ELSE now() END,
                   $8, $9)"#,
        account_id, instance.id, email, email_normalized, password_hash, invite_id,
        api_needs_approval, api_reason, confirmation_token,
    ).execute(&state.db).await
        .map_err(|_| AppError::Internal(anyhow::anyhow!("user creation failed")))?;

    if let Some(id) = invite_id {
        let _ = sqlx::query!("UPDATE invites SET uses = uses + 1 WHERE id = $1", id)
            .execute(&state.db).await;
    }

    let app_id = extract_app_from_bearer(&state, &req_headers).await;

    let token_str = api_generate_token();
    sqlx::query!(
        r#"INSERT INTO oauth_access_tokens (application_id, account_id, token, scopes)
           VALUES ($1, $2, $3, $4)"#,
        app_id, account_id, token_str, scopes,
    ).execute(&state.db).await?;

    if let (Some(ref tok), Some(ref resend)) = (&confirmation_token, &state.config.resend) {
        let domain = instance.custom_domain.as_deref().unwrap_or(&instance.domain);
        let confirm_url = format!("https://{}/auth/confirm?token={}", domain, tok);
        let http = state.http.clone();
        let api_key = resend.api_key.clone();
        let from = resend.from.clone();
        let to = email.clone();
        let uname = username.clone();
        let locale_str = form.locale.clone().unwrap_or_else(|| "en".into());
        tokio::spawn(async move {
            if let Err(e) = crate::email::send_confirmation(&http, &api_key, &from, &to, &uname, "", &confirm_url, &locale_str).await {
                tracing::error!(error = %e, "failed to send confirmation email");
            }
        });
    }

    Ok(Json(super::types::Token {
        access_token: token_str,
        token_type: "Bearer".to_string(),
        scope: scopes.to_string(),
        created_at: chrono::Utc::now().timestamp(),
    }))
}

// ── GET /auth/confirm ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ConfirmQuery {
    pub token: String,
}

pub async fn confirm_email(
    State(state): State<AppState>,
    Query(q): Query<ConfirmQuery>,
) -> Response {
    let user = sqlx::query!(
        r#"UPDATE users
           SET confirmed_at = now(), confirmation_token = NULL
           WHERE confirmation_token = $1 AND confirmed_at IS NULL
           RETURNING account_id"#,
        q.token,
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    let Some(user) = user else {
        return (StatusCode::NOT_FOUND, Html(
            "<h1>Invalid confirmation link</h1><p>This link may have already been used or is invalid.</p>".to_string()
        )).into_response();
    };

    // Find the profile-scope token to discover which app the user signed up with.
    let tok = sqlx::query!(
        r#"SELECT id, application_id FROM oauth_access_tokens
           WHERE account_id = $1 AND scopes = 'profile' AND revoked_at IS NULL
           ORDER BY created_at DESC LIMIT 1"#,
        user.account_id,
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    if let Some(tok) = tok {
        if let Some(app_id) = tok.application_id {
            if let Ok(Some(app)) = sqlx::query!(
                "SELECT redirect_uris, scopes FROM oauth_applications WHERE id = $1",
                app_id,
            )
            .fetch_optional(&state.db)
            .await
            {
                // Revoke the profile token — the app will get a full-scope one via the code below.
                let _ = sqlx::query!(
                    "UPDATE oauth_access_tokens SET revoked_at = now() WHERE id = $1",
                    tok.id,
                )
                .execute(&state.db)
                .await;

                let redirect_uri = app.redirect_uris.lines().next().unwrap_or("").to_string();

                if !redirect_uri.is_empty() && redirect_uri != "urn:ietf:wg:oauth:2.0:oob" {
                    let code = api_generate_token();
                    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(10);

                    if sqlx::query!(
                        r#"INSERT INTO oauth_authorization_codes
                             (application_id, account_id, code, redirect_uri, scopes, expires_at)
                           VALUES ($1, $2, $3, $4, $5, $6)"#,
                        app_id, user.account_id, code, redirect_uri, app.scopes, expires_at,
                    )
                    .execute(&state.db)
                    .await
                    .is_ok()
                    {
                        let sep = if redirect_uri.contains('?') { '&' } else { '?' };
                        return Redirect::to(&format!("{}{}code={}", redirect_uri, sep, code))
                            .into_response();
                    }
                }
            }
        }
    }

    Html("<h1>Email confirmed!</h1><p>Your account is now active. You can sign in.</p>".to_string()).into_response()
}

// ── POST /auth/password  (request reset) ──────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PasswordResetRequestForm {
    pub email: Option<String>,
}

pub async fn request_password_reset(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Form(form): Form<PasswordResetRequestForm>,
) -> impl IntoResponse {
    // Always return 200 to avoid email enumeration
    let email = match form.email {
        Some(e) if !e.is_empty() => e.trim().to_lowercase(),
        _ => return StatusCode::OK.into_response(),
    };

    let row = sqlx::query!(
        "SELECT u.id, u.email, a.username FROM users u
         JOIN accounts a ON a.id = u.account_id
         WHERE u.email_normalized = $1 AND u.instance_id = $2 AND u.confirmed_at IS NOT NULL",
        email, instance.id,
    )
    .fetch_optional(&state.db)
    .await;

    let Ok(Some(row)) = row else {
        return StatusCode::OK.into_response();
    };

    let token = crypto::generate_token(32);
    let _ = sqlx::query!(
        "UPDATE users SET password_reset_token = $1, password_reset_sent_at = now() WHERE id = $2",
        token, row.id,
    )
    .execute(&state.db)
    .await;

    if let Some(ref resend) = state.config.resend {
        let domain = instance.custom_domain.as_deref().unwrap_or(&instance.domain);
        let reset_url = format!("https://{}/auth/password/reset?token={}", domain, token);
        let http = state.http.clone();
        let api_key = resend.api_key.clone();
        let from = resend.from.clone();
        let to = row.email.clone();
        let name = row.username.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::email::send_password_reset(
                &http, &api_key, &from, &to, &name, &reset_url, "en",
            ).await {
                tracing::error!(error = %e, "failed to send password reset email");
            }
        });
    }

    StatusCode::OK.into_response()
}

// ── PUT /auth/password  (apply reset) ─────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PasswordResetForm {
    pub token: Option<String>,
    pub password: Option<String>,
}

pub async fn apply_password_reset(
    State(state): State<AppState>,
    Form(form): Form<PasswordResetForm>,
) -> impl IntoResponse {
    let token = match form.token {
        Some(t) if !t.is_empty() => t,
        _ => return (StatusCode::UNPROCESSABLE_ENTITY, "Missing token").into_response(),
    };
    let password = match form.password {
        Some(p) if p.len() >= 8 => p,
        _ => return (StatusCode::UNPROCESSABLE_ENTITY, "Password must be at least 8 characters").into_response(),
    };

    let row = sqlx::query!(
        r#"SELECT id FROM users
           WHERE password_reset_token = $1
             AND password_reset_sent_at > now() - interval '1 hour'"#,
        token,
    )
    .fetch_optional(&state.db)
    .await;

    let Ok(Some(row)) = row else {
        return (StatusCode::UNPROCESSABLE_ENTITY, "Invalid or expired token").into_response();
    };

    let hash = match crypto::hash_password(&password) {
        Ok(h) => h,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Server error").into_response(),
    };

    let _ = sqlx::query!(
        "UPDATE users SET password_hash = $1, password_reset_token = NULL, password_reset_sent_at = NULL WHERE id = $2",
        hash, row.id,
    )
    .execute(&state.db)
    .await;

    StatusCode::OK.into_response()
}

// ── helpers ────────────────────────────────────────────────────────────────

async fn extract_app_from_bearer(state: &AppState, headers: &HeaderMap) -> Option<Uuid> {
    let val = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    let token = val.strip_prefix("Bearer ")?.trim();
    sqlx::query_scalar!(
        "SELECT application_id FROM oauth_access_tokens WHERE token = $1 AND account_id IS NULL",
        token
    ).fetch_optional(&state.db).await.ok().flatten().flatten()
}

fn api_generate_token() -> String {
    use rand::RngCore;
    let mut rng = rand::rng();
    (0..64).map(|_| format!("{:02x}", rng.next_u32() as u8)).collect()
}

// ── helpers ────────────────────────────────────────────────────────────────

fn render(
    instance: &Instance,
    invite: &str,
    show_form: bool,
    pending: bool,
    error: Option<&'static str>,
    locale: crate::locale::Locale,
) -> Response {
    let enc_invite = urlencoding::encode(invite);
    let toggle_en_url = if invite.is_empty() {
        "/auth/signup?lang=en".to_string()
    } else {
        format!("/auth/signup?invite={}&lang=en", enc_invite)
    };
    let toggle_ko_url = if invite.is_empty() {
        "/auth/signup?lang=ko".to_string()
    } else {
        format!("/auth/signup?invite={}&lang=ko", enc_invite)
    };
    let html = templates::render(
        "signup.html",
        minijinja::context! {
            instance_title => &instance.title,
            instance_domain => &instance.domain,
            show_form,
            pending,
            approval_required => instance.approval_required,
            invite,
            error,
            lang => locale.as_str(),
            toggle_en_url => toggle_en_url,
            toggle_ko_url => toggle_ko_url,
            t_create_account => locale.t("create_account"),
            t_username => locale.t("username"),
            t_email => locale.t("email"),
            t_password => locale.t("password"),
            t_confirm_password => locale.t("confirm_password"),
            t_already_account => locale.t("already_account"),
            t_sign_in => locale.t("sign_in"),
            t_registrations_closed => locale.t("registrations_closed"),
            t_invite_code => locale.t("invite_code"),
            t_continue_btn => locale.t("continue_btn"),
            t_reason => locale.t("reason"),
            t_reason_hint => locale.t("reason_hint"),
            t_pending_approval => locale.t("pending_approval"),
            t_apply_for_account => locale.t("apply_for_account"),
        },
    );
    Html(html).into_response()
}
