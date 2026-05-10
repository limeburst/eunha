pub mod auth;
pub mod instances;

use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts},
    routing::{get, patch, post},
    Router,
};

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
        .route(
            "/api/console/instances/{domain}/applications",
            get(instances::list_applications),
        )
        .route(
            "/api/console/instances/{domain}/applications/{account_id}/approve",
            post(instances::approve_application),
        )
        .route(
            "/api/console/instances/{domain}/applications/{account_id}/reject",
            post(instances::reject_application),
        )
        .with_state(state)
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
