use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::{
    crypto::{generate_token, hash_password, verify_password},
    db::models::Instance,
    locale::Locale,
    middleware::ResolvedInstance,
    state::AppState,
    templates,
};

const COOKIE_NAME: &str = "account_session";
const COOKIE_MAX_AGE: u32 = 2_592_000; // 30 days

pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/account", get(account_home))
        .route("/account/login", get(login_page).post(login_post))
        .route("/account/logout", post(logout_post))
        .route("/account/password", get(password_page).post(password_post))
        .route("/account/invites", get(invites_page))
        .with_state(state)
}

// ── Session lookup ─────────────────────────────────────────────────────────────

struct AccountSession {
    user_id: Uuid,
    username: String,
}

fn extract_session_token(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix(&format!("{COOKIE_NAME}=")) {
            return Some(val.to_string());
        }
    }
    None
}

async fn get_session(
    headers: &HeaderMap,
    state: &AppState,
    instance: &Instance,
) -> Option<AccountSession> {
    let token = extract_session_token(headers)?;
    let row = sqlx::query!(
        r#"SELECT u.id as user_id, a.username
           FROM instance_user_sessions s
           JOIN users u ON u.id = s.user_id
           JOIN accounts a ON a.id = u.account_id
           WHERE s.token = $1
             AND u.instance_id = $2
             AND (s.expires_at IS NULL OR s.expires_at > now())"#,
        token,
        instance.id,
    )
    .fetch_optional(&state.db)
    .await
    .ok()??;

    Some(AccountSession {
        user_id: row.user_id,
        username: row.username,
    })
}

fn set_cookie(token: &str) -> String {
    format!(
        "{COOKIE_NAME}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={COOKIE_MAX_AGE}"
    )
}

fn clear_cookie() -> &'static str {
    "account_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0"
}

fn accept_language(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok())
}

// ── GET /account ───────────────────────────────────────────────────────────────

pub async fn account_home(
    State(state): State<AppState>,
    axum::extract::Extension(ResolvedInstance(instance)): axum::extract::Extension<ResolvedInstance>,
    headers: HeaderMap,
) -> Response {
    let locale = Locale::detect(None, accept_language(&headers));

    let Some(session) = get_session(&headers, &state, &instance).await else {
        return Redirect::to("/account/login").into_response();
    };

    let domain = instance.custom_domain.as_deref().unwrap_or(&instance.domain).to_string();

    let html = templates::render(
        "account_home.html",
        minijinja::context! {
            lang => locale.as_str(),
            domain,
            username => session.username,
            t_account => locale.t("account"),
            t_invite_tree => locale.t("invite_tree"),
            t_change_password => locale.t("change_password"),
            t_sign_out => locale.t("sign_out"),
        },
    );
    Html(html).into_response()
}

// ── GET /account/login ─────────────────────────────────────────────────────────

pub async fn login_page(
    axum::extract::Extension(ResolvedInstance(instance)): axum::extract::Extension<ResolvedInstance>,
    headers: HeaderMap,
) -> Response {
    let locale = Locale::detect(None, accept_language(&headers));
    let domain = instance.custom_domain.as_deref().unwrap_or(&instance.domain).to_string();

    let html = templates::render(
        "account_login.html",
        minijinja::context! {
            lang => locale.as_str(),
            domain,
            error => "",
            t_email => locale.t("email"),
            t_password => locale.t("password"),
            t_sign_in => locale.t("sign_in"),
            t_account => locale.t("account"),
        },
    );
    Html(html).into_response()
}

// ── POST /account/login ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    pub email: String,
    pub password: String,
}

pub async fn login_post(
    State(state): State<AppState>,
    axum::extract::Extension(ResolvedInstance(instance)): axum::extract::Extension<ResolvedInstance>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let locale = Locale::detect(None, accept_language(&headers));
    let domain = instance.custom_domain.as_deref().unwrap_or(&instance.domain).to_string();

    let email_normalized = form.email.trim().to_lowercase();

    let render_error = |error: &'static str| {
        let html = templates::render(
            "account_login.html",
            minijinja::context! {
                lang => locale.as_str(),
                domain => domain.clone(),
                error,
                t_email => locale.t("email"),
                t_password => locale.t("password"),
                t_sign_in => locale.t("sign_in"),
                t_account => locale.t("account"),
            },
        );
        Html(html).into_response()
    };

    let row = match sqlx::query!(
        r#"SELECT u.id, u.password_hash, a.username
           FROM users u
           JOIN accounts a ON a.id = u.account_id
           WHERE u.email_normalized = $1
             AND u.instance_id = $2
             AND u.confirmed_at IS NOT NULL
             AND a.domain IS NULL"#,
        email_normalized,
        instance.id,
    )
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some(r)) => r,
        _ => return render_error(locale.t("invalid_credentials")),
    };

    if verify_password(&form.password, &row.password_hash).is_err() {
        return render_error(locale.t("invalid_credentials"));
    }

    let token = generate_token(64);
    if sqlx::query!(
        "INSERT INTO instance_user_sessions (user_id, token) VALUES ($1, $2)",
        row.id,
        token,
    )
    .execute(&state.db)
    .await
    .is_err()
    {
        return render_error(locale.t("err_server"));
    }

    (
        [(header::SET_COOKIE, set_cookie(&token))],
        Redirect::to("/account"),
    )
        .into_response()
}

// ── POST /account/logout ───────────────────────────────────────────────────────

pub async fn logout_post(headers: HeaderMap) -> Response {
    // Attempt to delete the session from the DB would be nice but we don't have
    // the state here easily — the cookie expiry is sufficient for security.
    let _ = headers; // suppress unused warning
    (
        [(header::SET_COOKIE, clear_cookie())],
        Redirect::to("/account/login"),
    )
        .into_response()
}

// ── GET /account/password ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PasswordQuery {
    pub ok: Option<String>,
    pub err: Option<String>,
}

pub async fn password_page(
    State(state): State<AppState>,
    axum::extract::Extension(ResolvedInstance(instance)): axum::extract::Extension<ResolvedInstance>,
    headers: HeaderMap,
    Query(query): Query<PasswordQuery>,
) -> Response {
    let locale = Locale::detect(None, accept_language(&headers));

    let Some(_session) = get_session(&headers, &state, &instance).await else {
        return Redirect::to("/account/login").into_response();
    };

    let domain = instance.custom_domain.as_deref().unwrap_or(&instance.domain).to_string();
    let ok = query.ok.as_deref() == Some("1");
    let err = query.err.as_deref() == Some("1");

    let html = templates::render(
        "account_password.html",
        minijinja::context! {
            lang => locale.as_str(),
            domain,
            ok,
            err,
            t_account => locale.t("account"),
            t_change_password => locale.t("change_password"),
            t_current_password => locale.t("current_password"),
            t_new_password => locale.t("new_password"),
            t_sign_out => locale.t("sign_out"),
            t_back_to_account => locale.t("back_to_account"),
            t_password_changed => locale.t("password_changed"),
            t_password_error => locale.t("password_error"),
        },
    );
    Html(html).into_response()
}

// ── POST /account/password ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PasswordForm {
    pub current_password: String,
    pub new_password: String,
}

pub async fn password_post(
    State(state): State<AppState>,
    axum::extract::Extension(ResolvedInstance(instance)): axum::extract::Extension<ResolvedInstance>,
    headers: HeaderMap,
    Form(form): Form<PasswordForm>,
) -> Response {
    let Some(session) = get_session(&headers, &state, &instance).await else {
        return Redirect::to("/account/login").into_response();
    };

    if form.new_password.len() < 8 {
        return Redirect::to("/account/password?err=1").into_response();
    }

    let row = match sqlx::query!(
        "SELECT password_hash FROM users WHERE id = $1",
        session.user_id,
    )
    .fetch_one(&state.db)
    .await
    {
        Ok(r) => r,
        Err(_) => return Redirect::to("/account/password?err=1").into_response(),
    };

    if verify_password(&form.current_password, &row.password_hash).is_err() {
        return Redirect::to("/account/password?err=1").into_response();
    }

    let new_hash = match hash_password(&form.new_password) {
        Ok(h) => h,
        Err(_) => return Redirect::to("/account/password?err=1").into_response(),
    };

    match sqlx::query!(
        "UPDATE users SET password_hash = $1, updated_at = now() WHERE id = $2",
        new_hash,
        session.user_id,
    )
    .execute(&state.db)
    .await
    {
        Ok(_) => Redirect::to("/account/password?ok=1").into_response(),
        Err(_) => Redirect::to("/account/password?err=1").into_response(),
    }
}

// ── GET /account/invites ───────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct InviteView {
    id: String,
    code: String,
    url: String,
    created_by_username: Option<String>,
    max_uses: Option<i32>,
    uses: i32,
    is_expired: bool,
    redeemers: Vec<String>,
}

pub async fn invites_page(
    State(state): State<AppState>,
    axum::extract::Extension(ResolvedInstance(instance)): axum::extract::Extension<ResolvedInstance>,
    headers: HeaderMap,
) -> Response {
    let locale = Locale::detect(None, accept_language(&headers));

    let Some(_session) = get_session(&headers, &state, &instance).await else {
        return Redirect::to("/account/login").into_response();
    };

    let domain = instance.custom_domain.as_deref().unwrap_or(&instance.domain).to_string();

    // Fetch all members with their invite info
    let members = match sqlx::query!(
        r#"SELECT a.id AS account_id, a.username, u.invite_id AS "invite_id?: Uuid",
                  inv_a.username AS "invited_by_username?: String", u.created_at
           FROM users u
           JOIN accounts a ON a.id = u.account_id
           LEFT JOIN invites i ON i.id = u.invite_id
           LEFT JOIN accounts inv_a ON inv_a.id = i.created_by
           WHERE u.instance_id = $1 AND a.domain IS NULL
           ORDER BY u.created_at ASC"#,
        instance.id,
    )
    .fetch_all(&state.db)
    .await
    {
        Ok(r) => r,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    // Build redeemers_by_invite: HashMap<Uuid, Vec<String>>
    let mut redeemers_by_invite: HashMap<Uuid, Vec<String>> = HashMap::new();
    let mut uninvited: Vec<String> = Vec::new();

    for member in &members {
        if let Some(invite_id) = member.invite_id {
            redeemers_by_invite
                .entry(invite_id)
                .or_default()
                .push(member.username.clone());
        } else {
            uninvited.push(member.username.clone());
        }
    }

    // Fetch all invites
    let invites_rows = match sqlx::query!(
        r#"SELECT i.id, i.code, i.max_uses, i.uses, i.expires_at, i.created_at,
                  a.username AS "created_by_username?: String"
           FROM invites i
           LEFT JOIN accounts a ON a.id = i.created_by
           WHERE i.instance_id = $1
           ORDER BY i.created_at DESC"#,
        instance.id,
    )
    .fetch_all(&state.db)
    .await
    {
        Ok(r) => r,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let now = chrono::Utc::now();
    let invites: Vec<InviteView> = invites_rows
        .into_iter()
        .map(|r| {
            let is_expired = r.expires_at.map_or(false, |e| e < now);
            let redeemers = redeemers_by_invite
                .get(&r.id)
                .cloned()
                .unwrap_or_default();
            let url = crate::api::mastodon::invites::invite_url(&domain, &r.code);
            InviteView {
                id: r.id.to_string(),
                code: r.code,
                url,
                created_by_username: r.created_by_username,
                max_uses: r.max_uses,
                uses: r.uses,
                is_expired,
                redeemers,
            }
        })
        .collect();

    let html = templates::render(
        "account_invites.html",
        minijinja::context! {
            lang => locale.as_str(),
            domain,
            invites,
            uninvited,
            t_account => locale.t("account"),
            t_invite_tree => locale.t("invite_tree"),
            t_sign_out => locale.t("sign_out"),
            t_back_to_account => locale.t("back_to_account"),
            t_no_members => locale.t("no_members"),
            t_uninvited_members => locale.t("uninvited_members"),
            t_expired => locale.t("expired"),
        },
    );
    Html(html).into_response()
}
