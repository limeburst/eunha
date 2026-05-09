pub mod auth;
pub mod instance_auth;
pub mod instances;

use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts},
    routing::{get, patch, post},
    Router,
};
use uuid::Uuid;

use crate::{
    db::models::ConsoleUser,
    error::AppError,
    state::AppState,
};

pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/console/auth/signup", post(auth::signup))
        .route("/api/console/auth/login", post(auth::login))
        .route("/api/console/auth/me", get(auth::me))
        .route("/api/console/auth/password", patch(auth::change_password))
        .route("/api/console/auth/locale", patch(auth::update_locale))
        .route(
            "/api/console/instances",
            get(instances::list).post(instances::create),
        )
        .route(
            "/api/console/instances/{domain}",
            get(instances::get_one)
                .patch(instances::update)
                .delete(instances::delete),
        )
        .route(
            "/api/console/instances/{domain}/invites",
            get(instances::invite_tree).post(instances::create_console_invite),
        )
        .route("/api/console/instance_auth/login", post(instance_auth::login))
        .route("/api/console/instance_auth/me", get(instance_auth::me))
        .route("/api/console/instance_auth/password", patch(instance_auth::change_password))
        .route("/api/console/instance_auth/invite_tree", get(instance_auth::invite_tree))
        .with_state(state)
}

/// Extracts the authenticated instance user from a Bearer token in `instance_user_sessions`.
pub struct InstanceUserSession {
    pub user_id: Uuid,
    pub account_id: Uuid,
    pub instance_id: Uuid,
    pub username: String,
    pub instance_domain: String,
}

pub struct InstanceUserAuth(pub InstanceUserSession);

impl FromRequestParts<AppState> for InstanceUserAuth {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .ok_or(AppError::Unauthorized)?
            .to_string();

        let row = sqlx::query!(
            r#"SELECT
                 u.id           AS user_id,
                 u.account_id,
                 u.instance_id,
                 a.username,
                 inst.domain    AS instance_domain
               FROM instance_user_sessions s
               JOIN users u      ON u.id    = s.user_id
               JOIN accounts a   ON a.id    = u.account_id
               JOIN instances inst ON inst.id = u.instance_id
               WHERE s.token = $1
                 AND (s.expires_at IS NULL OR s.expires_at > now())"#,
            token,
        )
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::Database)?
        .ok_or(AppError::Unauthorized)?;

        Ok(InstanceUserAuth(InstanceUserSession {
            user_id: row.user_id,
            account_id: row.account_id,
            instance_id: row.instance_id,
            username: row.username,
            instance_domain: row.instance_domain,
        }))
    }
}

/// Extracts the authenticated console user from a Bearer token stored in
/// `console_sessions`. Returns 401 if the token is absent or invalid.
pub struct ConsoleAuth(pub ConsoleUser);

impl FromRequestParts<AppState> for ConsoleAuth {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .ok_or(AppError::Unauthorized)?
            .to_string();

        let user = sqlx::query_as!(
            ConsoleUser,
            r#"SELECT cu.* FROM console_users cu
               JOIN console_sessions cs ON cs.console_user_id = cu.id
               WHERE cs.token = $1
               AND (cs.expires_at IS NULL OR cs.expires_at > now())"#,
            token,
        )
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::Database)?
        .ok_or(AppError::Unauthorized)?;

        Ok(ConsoleAuth(user))
    }
}
