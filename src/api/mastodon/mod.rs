pub mod accounts;
pub mod bookmarks;
pub mod convert;
pub mod emojis;
pub mod favourites;
pub mod instance;
pub mod invites;
pub mod lists;
pub mod markers;
pub mod media;
pub mod notifications;
pub mod oauth;
pub mod polls;
pub mod scheduled_statuses;
pub mod search;
pub mod signup;
pub mod statuses;
pub mod streaming;
pub mod tags;
pub mod timelines;
pub mod types;

use axum::{
    extract::DefaultBodyLimit,
    http::HeaderMap,
    middleware,
    routing::{delete, get, patch, post},
    Json, Router,
};
use crate::{middleware as mw, state::AppState};

/// Build a `Link: <...>; rel="next", <...>; rel="prev"` header value for
/// paginated list endpoints. Returns `None` when `ids` is empty.
///
/// `ids` must be ordered from newest (largest) to oldest (smallest), matching
/// the `ORDER BY id DESC` convention used on every list endpoint.
pub(crate) fn link_header(
    req_headers: &HeaderMap,
    path: &str,
    extra_query: &str, // non-pagination params already joined with '&', no trailing '&'
    newest_id: &str,
    oldest_id: &str,
) -> String {
    let host = req_headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    let proto = req_headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("https");
    let base = format!("{proto}://{host}");
    let sep = if extra_query.is_empty() { "" } else { "&" };
    let next = format!("{base}{path}?{extra_query}{sep}max_id={oldest_id}");
    let prev = format!("{base}{path}?{extra_query}{sep}min_id={newest_id}");
    format!(r#"<{next}>; rel="next", <{prev}>; rel="prev""#)
}

/// Strip pagination-specific keys from a query string and return the rest.
pub(crate) fn non_pagination_query(raw_query: Option<&str>) -> String {
    raw_query
        .unwrap_or("")
        .split('&')
        .filter(|kv| {
            !kv.is_empty()
                && !kv.starts_with("max_id=")
                && !kv.starts_with("min_id=")
                && !kv.starts_with("since_id=")
                && !kv.starts_with("limit=")
        })
        .collect::<Vec<_>>()
        .join("&")
}

pub fn router(state: AppState) -> Router<AppState> {
    let auth_required = Router::new()
        // Accounts — authenticated
        .route("/api/v1/accounts/verify_credentials", get(accounts::verify_credentials))
        .route("/api/v1/accounts/search", get(accounts::search_accounts))
        .route("/api/v1/accounts/relationships", get(accounts::get_relationships))
        .route("/api/v1/accounts/{id}/follow", post(accounts::follow_account))
        .route("/api/v1/accounts/{id}/unfollow", post(accounts::unfollow_account))
        .route("/api/v1/accounts/{id}/mute", post(accounts::mute_account))
        .route("/api/v1/accounts/{id}/unmute", post(accounts::unmute_account))
        .route("/api/v1/accounts/{id}/block", post(accounts::block_account))
        .route("/api/v1/accounts/{id}/unblock", post(accounts::unblock_account))
        // Preferences
        .route("/api/v1/preferences", get(accounts::get_preferences))
        // Follow requests
        .route("/api/v1/follow_requests", get(accounts::get_follow_requests))
        .route("/api/v1/follow_requests/{id}/authorize", post(accounts::authorize_follow_request))
        .route("/api/v1/follow_requests/{id}/reject", post(accounts::reject_follow_request))
        // Statuses — authenticated writes
        .route("/api/v1/statuses", post(statuses::post_status))
        .route("/api/v1/statuses/{id}", delete(statuses::delete_status).put(statuses::edit_status))
        .route("/api/v1/statuses/{id}/favourite", post(statuses::favourite_status))
        .route("/api/v1/statuses/{id}/unfavourite", post(statuses::unfavourite_status))
        .route("/api/v1/statuses/{id}/reblog", post(statuses::reblog_status))
        .route("/api/v1/statuses/{id}/unreblog", post(statuses::unreblog_status))
        .route("/api/v1/statuses/{id}/bookmark", post(statuses::bookmark_status))
        .route("/api/v1/statuses/{id}/unbookmark", post(statuses::unbookmark_status))
        .route("/api/v1/statuses/{id}/pin", post(statuses::pin_status))
        .route("/api/v1/statuses/{id}/unpin", post(statuses::unpin_status))
        .route("/api/v1/statuses/{id}/mute", post(statuses::mute_status))
        .route("/api/v1/statuses/{id}/unmute", post(statuses::unmute_status))
        .route("/api/v1/statuses/{id}/source", get(statuses::get_status_source))
        // Blocks / Mutes lists
        .route("/api/v1/blocks", get(accounts::get_blocks))
        .route("/api/v1/mutes", get(accounts::get_mutes))
        // Lists
        .route("/api/v1/lists", get(lists::get_lists).post(lists::create_list))
        .route("/api/v1/lists/{id}", get(lists::get_list).put(lists::update_list).delete(lists::delete_list))
        .route("/api/v1/lists/{id}/accounts", get(lists::get_list_accounts).post(lists::add_list_accounts).delete(lists::remove_list_accounts))
        // Notifications — single dismiss
        .route("/api/v1/notifications/{id}/dismiss", post(notifications::dismiss_notification))
        // Media — get / update
        .route("/api/v1/media/{id}", get(media::get_media).put(media::update_media))
        // Bookmarks / Favourites
        .route("/api/v1/bookmarks", get(bookmarks::get_bookmarks))
        .route("/api/v1/favourites", get(favourites::get_favourites))
        // Markers
        .route("/api/v1/markers", get(markers::get_markers).post(markers::set_markers))
        // Timelines
        .route("/api/v1/timelines/home", get(timelines::home_timeline))
        .route("/api/v1/timelines/list/{id}", get(timelines::list_timeline))
        // Polls (authenticated write)
        .route("/api/v1/polls/{id}/votes", post(polls::vote_poll))
        // Notifications
        .route("/api/v1/notifications", get(notifications::get_notifications))
        .route("/api/v1/notifications/{id}", get(notifications::get_notification))
        .route("/api/v1/notifications/clear", post(notifications::clear_notifications))
        // Invites
        .route("/api/v1/invites", get(invites::list_invites).post(invites::create_invite))
        .route("/api/v1/invites/{id}", delete(invites::delete_invite))
        // Suggestions
        .route("/api/v1/suggestions", get(accounts::get_suggestions))
        .route("/api/v1/suggestions/{id}", delete(accounts::dismiss_suggestion))
        // Followed tags
        .route("/api/v1/followed_tags", get(tags::list_followed_tags))
        .route("/api/v1/tags/{name}/follow", post(tags::follow_tag))
        .route("/api/v1/tags/{name}/unfollow", post(tags::unfollow_tag))
        // Scheduled statuses
        .route("/api/v1/scheduled_statuses", get(scheduled_statuses::list_scheduled_statuses))
        // Media — 25 MB limit matching Mastodon's default
        .route("/api/v1/media", post(media::upload_media))
        .route("/api/v2/media", post(media::upload_media))
        .route("/api/v1/accounts/update_credentials", patch(accounts::update_credentials))
        .layer(DefaultBodyLimit::max(25 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_auth,
        ));

    let public = Router::new()
        // Instance info
        .route("/api/v1/instance", get(instance::get_instance_v1))
        .route("/api/v2/instance", get(instance::get_instance_v2))
        // Accounts (public)
        .route("/api/v1/accounts/lookup", get(accounts::lookup_account))
        .route("/api/v1/accounts/{id}", get(accounts::get_account))
        .route("/api/v1/accounts/{id}/statuses", get(accounts::get_account_statuses))
        .route("/api/v1/accounts/{id}/followers", get(accounts::get_account_followers))
        .route("/api/v1/accounts/{id}/following", get(accounts::get_account_following))
        // Statuses (public read)
        .route("/api/v1/statuses/{id}", get(statuses::get_status))
        .route("/api/v1/statuses/{id}/context", get(statuses::get_status_context))
        .route("/api/v1/statuses/{id}/favourited_by", get(statuses::favourited_by))
        .route("/api/v1/statuses/{id}/reblogged_by", get(statuses::reblogged_by))
        .route("/api/v1/statuses/{id}/history", get(statuses::get_status_history))
        // Polls (public read)
        .route("/api/v1/polls/{id}", get(polls::get_poll))
        // Search
        .route("/api/v2/search", get(search::search))
        // Timelines
        .route("/api/v1/timelines/public", get(timelines::public_timeline))
        .route("/api/v1/timelines/tag/{hashtag}", get(timelines::tag_timeline))
        // Sign-up (server-rendered form)
        .route("/auth/signup", get(signup::signup_get).post(signup::signup_post))
        // Trends — no analytics data; always empty
        .route("/api/v1/trends/statuses", get(empty_array))
        .route("/api/v1/trends/tags", get(empty_array))
        .route("/api/v1/trends/links", get(empty_array))
        // Custom emojis (public)
        .route("/api/v1/custom_emojis", get(emojis::list_custom_emojis))
        // Announcements / conversations — not yet implemented
        .route("/api/v1/announcements", get(empty_array))
        .route("/api/v1/conversations", get(empty_array))
        // OAuth
        .route("/api/v1/apps", post(oauth::register_app))
        .route("/api/{server}/login", post(oauth::elk_login))
        .route("/api/{server}/oauth/{origin}", get(oauth::elk_oauth_callback))
        .route("/oauth/authorize", get(oauth::authorize_form).post(oauth::authorize_submit))
        .route("/oauth/token", post(oauth::issue_token))
        .route("/oauth/revoke", post(oauth::revoke_token));

    Router::new().merge(auth_required).merge(public)
}

/// Routes that must NOT be wrapped by CompressionLayer (WebSocket upgrades).
pub fn streaming_router() -> Router<AppState> {
    Router::new().route("/api/v1/streaming", get(streaming::handler))
}

async fn empty_array() -> Json<[(); 0]> {
    Json([])
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
