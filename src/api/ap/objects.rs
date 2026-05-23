use axum::{
    extract::{Extension, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::{
    error::{AppError, AppResult},
    middleware::ResolvedInstance,
    state::AppState,
};

pub const ACTIVITY_STREAMS: &str = "application/activity+json";
pub const CONTENT_TYPE: &str = "application/activity+json; charset=utf-8";

pub async fn get_actor(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Path(username): Path<String>,
) -> AppResult<Response> {
    let account = sqlx::query!(
        r#"SELECT a.* FROM accounts a
           WHERE a.username = $1 AND a.domain IS NULL"#,
        username,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let base = format!("https://{}", instance.domain);
    let actor_url = format!("{}/users/{}", base, username);

    let actor = json!({
        "@context": [
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1",
            {
                "manuallyApprovesFollowers": "as:manuallyApprovesFollowers",
                "toot": "http://joinmastodon.org/ns#",
                "featured": { "@id": "toot:featured", "@type": "@id" },
                "discoverable": "toot:discoverable",
                "indexable": "toot:indexable",
                "fep": "https://w3id.org/fep/044f#",
                "quote": { "@id": "fep:quote", "@type": "@id" },
                "quoteUrl": { "@id": "fep:quote", "@type": "@id" },
            }
        ],
        "id": actor_url,
        "type": "Person",
        "following": format!("{}/following", actor_url),
        "followers": format!("{}/followers", actor_url),
        "inbox": format!("{}/inbox", actor_url),
        "outbox": format!("{}/outbox", actor_url),
        "featured": format!("{}/collections/featured", actor_url),
        "preferredUsername": account.username,
        "name": account.display_name,
        "summary": account.note,
        "url": account.url,
        "manuallyApprovesFollowers": account.locked,
        "discoverable": account.discoverable,
        "indexable": account.indexable,
        "published": account.created_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        "icon": account.avatar.map(|a| json!({ "type": "Image", "url": a })),
        "image": account.header.map(|h| json!({ "type": "Image", "url": h })),
        "publicKey": {
            "id": format!("{}#main-key", actor_url),
            "owner": actor_url,
            "publicKeyPem": account.public_key,
        },
        "endpoints": {
            "sharedInbox": format!("{}/inbox", base),
        },
    });

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, CONTENT_TYPE)],
        Json(actor),
    )
        .into_response())
}
