use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{Html, IntoResponse, Response},
};
use uuid::Uuid;

use crate::{db, error::AppError, state::AppState, db::models::Instance, templates};

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
            // If the request arrived on the eunha.social subdomain but the
            // instance has a canonical custom domain, redirect there permanently.
            if host == instance.domain {
                if let Some(ref custom) = instance.custom_domain {
                    let location = rebuild_url(&req, custom);
                    return Ok((
                        StatusCode::MOVED_PERMANENTLY,
                        [(axum::http::header::LOCATION, location)],
                    )
                        .into_response());
                }
            }
            req.extensions_mut().insert(ResolvedInstance(instance));
            Ok(next.run(req).await)
        }
        Err(AppError::NotFound) => Ok(unknown_host_page(&host).into_response()),
        Err(e) => Err(e),
    }
}

/// Reconstruct the request URL swapping in a different host.
fn rebuild_url(req: &Request, new_host: &str) -> String {
    let uri = req.uri();
    let path_and_query = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    format!("https://{new_host}{path_and_query}")
}

/// Resolved OAuth token + account, injected by [`authenticate`].
#[derive(Clone)]
pub struct AuthenticatedUser {
    pub account_id: i64,
    pub token_id: Uuid,
    pub scopes: Vec<String>,
    pub application_id: Option<i64>,
}

impl AuthenticatedUser {
    /// Returns `Err(AppError::Forbidden)` if the token does not cover `required`.
    ///
    /// Scope hierarchy:
    /// - `"read"` covers every `"read:*"` sub-scope.
    /// - `"write"` covers every `"write:*"` sub-scope.
    /// - `"follow"` covers `"write:follows"`, `"write:blocks"`, and `"write:mutes"`.
    pub fn require_scope(&self, required: &str) -> crate::error::AppResult<()> {
        if self.has_scope(required) {
            Ok(())
        } else {
            Err(crate::error::AppError::Forbidden)
        }
    }

    fn has_scope(&self, required: &str) -> bool {
        if self.scopes.iter().any(|s| s == required) {
            return true;
        }
        // Parent scope covers child: "read" → "read:*", "write" → "write:*"
        if let Some(parent) = required.split(':').next() {
            if self.scopes.iter().any(|s| s == parent) {
                return true;
            }
        }
        // "follow" covers social-graph write operations
        if matches!(required, "write:follows" | "write:blocks" | "write:mutes") {
            if self.scopes.iter().any(|s| s == "follow") {
                return true;
            }
        }
        false
    }
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
            r#"SELECT id, account_id, application_id, scopes, expires_at, revoked_at
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
                        token_id: tok.id,
                        scopes: tok.scopes.split_whitespace().map(str::to_owned).collect(),
                        application_id: tok.application_id,
                    };
                    req.extensions_mut().insert(user);
                }
            }
        }
    }
    next.run(req).await
}

/// Log failed requests (4xx/5xx) with method, path, and request body preview.
/// Skips body buffering for multipart uploads to avoid memory pressure.
pub async fn log_failures(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let content_type = req
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();

    let is_text = content_type.contains("json") || content_type.contains("x-www-form-urlencoded");
    let is_multipart = content_type.contains("multipart");

    let (parts, body) = req.into_parts();
    let (body_preview, rebuilt) = if is_text && !is_multipart {
        match axum::body::to_bytes(body, 4096).await {
            Ok(bytes) => {
                let preview = String::from_utf8_lossy(&bytes).into_owned();
                let new_body = axum::body::Body::from(bytes);
                (Some(preview), Request::from_parts(parts, new_body))
            }
            Err(_) => (None, Request::from_parts(parts, axum::body::Body::empty())),
        }
    } else {
        (None, Request::from_parts(parts, body))
    };

    let response = next.run(rebuilt).await;
    let status = response.status();

    if status.is_client_error() || status.is_server_error() {
        tracing::warn!(
            method = %method,
            path = %path,
            status = %status,
            body = body_preview.as_deref().unwrap_or(""),
            "request failed",
        );
    }

    response
}

fn unknown_host_page(host: &str) -> impl IntoResponse {
    let html = templates::render("unknown_host.html", minijinja::context! { host });
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
