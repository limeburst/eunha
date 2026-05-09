use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{Html, IntoResponse, Response},
};
use uuid::Uuid;

use crate::{db, error::AppError, state::AppState, db::models::Instance};

/// Resolved instance, injected into request extensions by [`resolve_instance`].
#[derive(Clone)]
pub struct ResolvedInstance(pub Instance);

/// Extracts the `Host` header, strips the port, and looks up the instance.
/// Returns 404 if the domain is not configured.
pub async fn resolve_instance(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let host = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_lowercase();

    if host == state.config.console_domain {
        return Ok(next.run(req).await);
    }

    match db::get_instance_by_domain(&state.db, &host).await {
        Ok(instance) => {
            req.extensions_mut().insert(ResolvedInstance(instance));
            Ok(next.run(req).await)
        }
        Err(AppError::NotFound) => Ok(unknown_host_page(&host).into_response()),
        Err(e) => Err(e),
    }
}

/// Resolved OAuth token + account, injected by [`authenticate`].
#[derive(Clone)]
pub struct AuthenticatedUser {
    pub account_id: Uuid,
    pub scopes: Vec<String>,
}

/// Bearer token authentication. Attaches `AuthenticatedUser` if a valid token
/// is present; passes through unauthenticated requests so endpoints can decide
/// whether auth is required.
pub async fn authenticate(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    if let Some(token) = extract_bearer(&req) {
        if let Some(tok) = sqlx::query!(
            r#"SELECT account_id, scopes, expires_at, revoked_at
               FROM oauth_access_tokens WHERE token = $1"#,
            token
        )
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        {
            let valid = tok.revoked_at.is_none()
                && tok.expires_at.map_or(true, |e| e > chrono::Utc::now());

            if valid {
                if let Some(account_id) = tok.account_id {
                    let user = AuthenticatedUser {
                        account_id,
                        scopes: tok.scopes.split_whitespace().map(str::to_owned).collect(),
                    };
                    req.extensions_mut().insert(user);
                }
            }
        }
    }
    next.run(req).await
}

fn unknown_host_page(host: &str) -> impl IntoResponse {
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>No instance found — eunha.social</title>
  <style>
    *, *::before, *::after {{ box-sizing: border-box; }}
    body {{
      font-family: ui-monospace, monospace;
      background: #0f0f0f;
      color: #e0e0e0;
      margin: 0;
      min-height: 100svh;
      display: flex;
      align-items: center;
      justify-content: center;
      padding: 2rem;
    }}
    main {{
      max-width: 480px;
      width: 100%;
    }}
    p {{ margin: 0 0 1rem; font-size: 0.8rem; line-height: 1.6; color: #888; }}
    h1 {{ margin: 0 0 2rem; font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.15em; color: #555; }}
    .host {{ color: #e0e0e0; }}
    a {{ color: #e0e0e0; }}
    a:hover {{ color: #888; }}
  </style>
</head>
<body>
  <main>
    <h1>eunha.social</h1>
    <p>No fediverse instance is hosted at <span class="host">{host}</span>.</p>
    <p>eunha.social is a fediverse instance hosting service. You can create your own Mastodon-compatible instance and be part of the open social web.</p>
    <p><a href="https://eunha.social">Sign up at eunha.social →</a></p>
  </main>
</body>
</html>"#
    );
    (StatusCode::NOT_FOUND, Html(html))
}

fn extract_bearer(req: &Request) -> Option<String> {
    let header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    header.strip_prefix("Bearer ").map(str::to_string)
}
