pub mod accounts;
pub mod convert;
pub mod instance;
pub mod media;
pub mod notifications;
pub mod oauth;
pub mod statuses;
pub mod streaming;
pub mod timelines;
pub mod types;

use axum::{
    middleware,
    routing::{delete, get, post},
    Router,
};
use crate::{middleware as mw, state::AppState};

pub fn router(state: AppState) -> Router<AppState> {
    let auth_required = Router::new()
        // Accounts
        .route("/api/v1/accounts/verify_credentials", get(accounts::verify_credentials))
        .route("/api/v1/accounts/{id}/follow", post(accounts::follow_account))
        .route("/api/v1/accounts/{id}/unfollow", post(accounts::unfollow_account))
        .route("/api/v1/accounts/relationships", get(accounts::get_relationships))
        // Statuses
        .route("/api/v1/statuses", post(statuses::post_status))
        .route("/api/v1/statuses/{id}", delete(statuses::delete_status))
        .route("/api/v1/statuses/{id}/favourite", post(statuses::favourite_status))
        .route("/api/v1/statuses/{id}/unfavourite", post(statuses::unfavourite_status))
        .route("/api/v1/statuses/{id}/reblog", post(statuses::reblog_status))
        // Timelines
        .route("/api/v1/timelines/home", get(timelines::home_timeline))
        // Notifications
        .route("/api/v1/notifications", get(notifications::get_notifications))
        .route("/api/v1/notifications/{id}", get(notifications::get_notification))
        .route("/api/v1/notifications/clear", post(notifications::clear_notifications))
        // Media
        .route("/api/v2/media", post(media::upload_media))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_auth,
        ));

    let public = Router::new()
        // Instance info
        .route("/api/v2/instance", get(instance::get_instance_v2))
        // Accounts (public)
        .route("/api/v1/accounts/lookup", get(accounts::lookup_account))
        .route("/api/v1/accounts/{id}", get(accounts::get_account))
        .route("/api/v1/accounts/{id}/statuses", get(accounts::get_account_statuses))
        // Statuses (public read)
        .route("/api/v1/statuses/{id}", get(statuses::get_status))
        // Timelines
        .route("/api/v1/timelines/public", get(timelines::public_timeline))
        // Streaming
        .route("/api/v1/streaming", get(streaming::handler))
        // OAuth
        .route("/api/v1/apps", post(oauth::register_app))
        .route("/oauth/token", post(oauth::issue_token))
        .route("/oauth/revoke", post(oauth::revoke_token));

    Router::new().merge(auth_required).merge(public)
}

async fn require_auth(
    auth: Option<axum::extract::Extension<mw::AuthenticatedUser>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, crate::error::AppError> {
    if auth.is_none() {
        return Err(crate::error::AppError::Unauthorized);
    }
    Ok(next.run(req).await)
}
