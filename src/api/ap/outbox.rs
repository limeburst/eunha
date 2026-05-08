use axum::{
    extract::{Extension, Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    error::{AppError, AppResult},
    middleware::ResolvedInstance,
    state::AppState,
};
use super::objects::CONTENT_TYPE;

#[derive(Deserialize)]
pub struct OutboxQuery {
    pub page: Option<bool>,
    pub min_id: Option<i64>,
    pub max_id: Option<i64>,
}

pub async fn get_outbox(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Path(username): Path<String>,
    Query(q): Query<OutboxQuery>,
) -> AppResult<Response> {
    let account = sqlx::query!(
        "SELECT id, statuses_count FROM accounts WHERE username = $1 AND instance_id = $2 AND domain IS NULL",
        username, instance.id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let base_url = format!("https://{}/users/{}/outbox", instance.domain, username);

    if q.page != Some(true) {
        // Return the OrderedCollection summary
        let outbox = json!({
            "@context": "https://www.w3.org/ns/activitystreams",
            "id": base_url,
            "type": "OrderedCollection",
            "totalItems": account.statuses_count,
            "first": format!("{}?page=true", base_url),
            "last": format!("{}?page=true&min_id=0", base_url),
        });
        return Ok((StatusCode::OK, [(header::CONTENT_TYPE, CONTENT_TYPE)], Json(outbox)).into_response());
    }

    let statuses = sqlx::query!(
        r#"SELECT s.id, s.content, s.spoiler_text, s.visibility, s.sensitive,
                  s.created_at, s.uri, s.url, s.in_reply_to_id
           FROM statuses s
           WHERE s.account_id = $1
             AND s.deleted_at IS NULL
             AND s.visibility IN ('public', 'unlisted')
             AND ($2::bigint IS NULL OR s.id < $2)
             AND ($3::bigint IS NULL OR s.id > $3)
           ORDER BY s.id DESC
           LIMIT 20"#,
        account.id,
        q.max_id,
        q.min_id,
    )
    .fetch_all(&state.db)
    .await?;

    let actor_url = format!("https://{}/users/{}", instance.domain, username);

    let items: Vec<Value> = statuses
        .iter()
        .map(|s| {
            let note_url = s.url.clone().unwrap_or_else(|| {
                format!("https://{}/users/{}/statuses/{}", instance.domain, username, s.id)
            });
            json!({
                "@context": "https://www.w3.org/ns/activitystreams",
                "id": format!("{}/activity", note_url),
                "type": "Create",
                "actor": actor_url,
                "published": s.created_at.to_rfc3339(),
                "to": ["https://www.w3.org/ns/activitystreams#Public"],
                "cc": [format!("{}/followers", actor_url)],
                "object": {
                    "id": note_url,
                    "type": "Note",
                    "summary": if s.spoiler_text.is_empty() { None } else { Some(&s.spoiler_text) },
                    "inReplyTo": s.in_reply_to_id.map(|_| Value::Null),
                    "published": s.created_at.to_rfc3339(),
                    "url": note_url,
                    "attributedTo": actor_url,
                    "to": ["https://www.w3.org/ns/activitystreams#Public"],
                    "cc": [format!("{}/followers", actor_url)],
                    "sensitive": s.sensitive,
                    "content": s.content,
                    "contentMap": { "und": s.content },
                    "attachment": [],
                    "tag": [],
                }
            })
        })
        .collect();

    let first_id = statuses.first().map(|s| s.id);
    let last_id = statuses.last().map(|s| s.id);

    let page = json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{}?page=true", base_url),
        "type": "OrderedCollectionPage",
        "partOf": base_url,
        "prev": first_id.map(|id| format!("{}?page=true&min_id={}", base_url, id)),
        "next": last_id.map(|id| format!("{}?page=true&max_id={}", base_url, id)),
        "orderedItems": items,
    });

    Ok((StatusCode::OK, [(header::CONTENT_TYPE, CONTENT_TYPE)], Json(page)).into_response())
}
