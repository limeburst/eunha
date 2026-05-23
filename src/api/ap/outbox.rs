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
        "SELECT id, statuses_count FROM accounts WHERE username = $1 AND domain IS NULL",
        username,
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
        r#"SELECT s.id, s.text, s.spoiler_text, s.visibility, s.sensitive,
                  s.created_at, s.uri, s.url, s.in_reply_to_id, s.quote_of_id,
                  s.interaction_policy,
                  q.uri AS quote_uri
           FROM statuses s
           LEFT JOIN statuses q ON q.id = s.quote_of_id AND q.deleted_at IS NULL
           WHERE s.account_id = $1
             AND s.deleted_at IS NULL
             AND s.visibility IN (0, 1) /* vis::PUBLIC, vis::UNLISTED */
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

    // Batch-fetch mentions for all statuses in this page
    let status_ids: Vec<i64> = statuses.iter().map(|s| s.id).collect();
    let mention_rows = if !status_ids.is_empty() {
        sqlx::query!(
            r#"SELECT m.status_id, a.username, a.domain, a.url
               FROM mentions m
               JOIN accounts a ON a.id = m.account_id
               WHERE m.status_id = ANY($1::bigint[])"#,
            &status_ids,
        )
        .fetch_all(&state.db)
        .await
        .unwrap_or_default()
    } else {
        vec![]
    };
    let mut mention_maps: std::collections::HashMap<i64, std::collections::HashMap<String, (String, String)>> =
        std::collections::HashMap::new();
    for m in &mention_rows {
        let map = mention_maps.entry(m.status_id).or_default();
        let key_short = m.username.to_lowercase();
        let display = match &m.domain {
            Some(d) => format!("{}@{}", m.username, d),
            None => m.username.clone(),
        };
        let url = m.url.clone();
        map.entry(key_short.clone()).or_insert_with(|| (url.clone(), display.clone()));
        if let Some(d) = &m.domain {
            map.entry(format!("{}@{}", key_short, d)).or_insert_with(|| (url, display));
        }
    }

    use crate::api::mastodon::formatting::render_content;
    let actor_url = format!("https://{}/users/{}", instance.domain, username);

    let fep044f_context = json!({
        "fep": "https://w3id.org/fep/044f#",
        "quote": { "@id": "fep:quote", "@type": "@id" },
        "quoteUrl": { "@id": "fep:quote", "@type": "@id" },
    });

    let items: Vec<Value> = statuses
        .iter()
        .map(|s| {
            let note_url = s.url.clone().unwrap_or_else(|| {
                format!("https://{}/users/{}/statuses/{}", instance.domain, username, s.id)
            });
            let empty_map = std::collections::HashMap::new();
            let mention_map = mention_maps.get(&s.id).unwrap_or(&empty_map);
            let content = render_content(&s.text, &instance.domain, mention_map);
            let quote_uri = s.quote_uri.clone();
            let always = s.interaction_policy.as_ref()
                .and_then(|p| p.get("can_quote"))
                .and_then(|cq| cq.get("always"))
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect::<Vec<_>>())
                .unwrap_or_else(|| {
                    if matches!(s.visibility, crate::db::models::vis::PUBLIC | crate::db::models::vis::UNLISTED) {
                        vec!["https://www.w3.org/ns/activitystreams#Public".to_string()]
                    } else {
                        vec![]
                    }
                });
            let with_approval = s.interaction_policy.as_ref()
                .and_then(|p| p.get("can_quote"))
                .and_then(|cq| cq.get("with_approval"))
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect::<Vec<_>>())
                .unwrap_or_default();
            json!({
                "@context": [
                    "https://www.w3.org/ns/activitystreams",
                    fep044f_context.clone(),
                ],
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
                    "content": content,
                    "contentMap": { "und": content },
                    "attachment": [],
                    "tag": [],
                    "quote": quote_uri,
                    "quoteUrl": s.quote_uri.clone(),
                    "interactionPolicy": {
                        "canQuote": {
                            "automaticApproval": always,
                            "manualApproval": with_approval,
                        }
                    }
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
