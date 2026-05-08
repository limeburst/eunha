pub mod inbox;
pub mod objects;
pub mod outbox;

use axum::{
    routing::{get, post},
    Router,
};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users/{username}", get(objects::get_actor))
        .route("/users/{username}/inbox", post(inbox::shared_inbox))
        .route("/users/{username}/outbox", get(outbox::get_outbox))
        .route("/inbox", post(inbox::shared_inbox))
}
