use axum::{
    extract::{Extension, State},
    http::StatusCode,
    Json,
};
use serde_json::Value;

use crate::{
    error::AppResult,
    middleware::ResolvedInstance,
    state::AppState,
};

/// Handles both `/inbox` (shared inbox) and `/users/:username/inbox`.
pub async fn shared_inbox(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Json(activity): Json<Value>,
) -> AppResult<StatusCode> {
    let activity_type = activity.get("type").and_then(|t| t.as_str()).unwrap_or("");

    tracing::debug!(
        instance = %instance.domain,
        activity_type = activity_type,
        "received ActivityPub activity"
    );

    // TODO: verify HTTP Signature before processing
    // For now, queue the activity for async processing
    match activity_type {
        "Follow" => handle_follow(&state, &instance, &activity).await?,
        "Undo" => handle_undo(&state, &instance, &activity).await?,
        "Create" => handle_create(&state, &instance, &activity).await?,
        "Delete" => handle_delete(&state, &instance, &activity).await?,
        "Announce" => handle_announce(&state, &instance, &activity).await?,
        "Like" => handle_like(&state, &instance, &activity).await?,
        "Accept" | "Reject" => handle_accept_reject(&state, &instance, &activity).await?,
        "Update" => handle_update(&state, &instance, &activity).await?,
        "QuoteRequest" => handle_quote_request(&state, &instance, &activity).await?,
        other => {
            tracing::debug!("ignoring unhandled activity type: {other}");
        }
    }

    Ok(StatusCode::ACCEPTED)
}

async fn handle_follow(
    state: &AppState,
    instance: &crate::db::models::Instance,
    activity: &Value,
) -> AppResult<()> {
    let actor_uri = activity.get("actor").and_then(|a| a.as_str()).unwrap_or("");
    let object_uri = activity.get("object").and_then(|o| o.as_str()).unwrap_or("");
    let activity_uri = activity.get("id").and_then(|i| i.as_str()).unwrap_or("");

    // Resolve the target local account
    let target = sqlx::query!(
        "SELECT id, locked FROM accounts WHERE uri = $1 AND instance_id = $2",
        object_uri,
        instance.id,
    )
    .fetch_optional(&state.db)
    .await?;

    let Some(target) = target else { return Ok(()) };

    // Resolve or fetch the remote actor
    let follower = resolve_or_fetch_remote_account(state, actor_uri).await?;

    let follow_state = if target.locked { "pending" } else { "accepted" };

    sqlx::query!(
        r#"INSERT INTO follows (account_id, target_account_id, state, uri)
           VALUES ($1,$2,$3,$4)
           ON CONFLICT (account_id, target_account_id)
           DO UPDATE SET state = EXCLUDED.state, uri = EXCLUDED.uri"#,
        follower,
        target.id,
        follow_state,
        activity_uri,
    )
    .execute(&state.db)
    .await?;

    if follow_state == "accepted" {
        // Queue Accept activity back to the follower
        // TODO: queue outgoing Accept via federation worker
    }

    Ok(())
}

async fn handle_undo(
    state: &AppState,
    _instance: &crate::db::models::Instance,
    activity: &Value,
) -> AppResult<()> {
    let object = activity.get("object");
    let object_type = object.and_then(|o| o.get("type")).and_then(|t| t.as_str());

    if object_type == Some("Follow") {
        let follow_uri = object.and_then(|o| o.get("id")).and_then(|i| i.as_str()).unwrap_or("");
        sqlx::query!("DELETE FROM follows WHERE uri = $1", follow_uri)
            .execute(&state.db)
            .await?;
    }

    Ok(())
}

async fn handle_create(
    state: &AppState,
    instance: &crate::db::models::Instance,
    activity: &Value,
) -> AppResult<()> {
    let object = match activity.get("object") {
        Some(o) if o.is_object() => o,
        _ => return Ok(()),
    };
    let obj_type = object.get("type").and_then(|t| t.as_str()).unwrap_or("");
    if obj_type != "Note" {
        return Ok(());
    }

    let actor_uri = activity.get("actor").and_then(|a| a.as_str()).unwrap_or("");
    let note_uri = object.get("id").and_then(|i| i.as_str()).unwrap_or("");
    if note_uri.is_empty() || actor_uri.is_empty() {
        return Ok(());
    }

    // Resolve the author account
    let account_id = match resolve_or_fetch_remote_account(state, actor_uri).await {
        Ok(id) => id,
        Err(_) => return Ok(()),
    };

    let text = object.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();
    let spoiler_text = object.get("summary").and_then(|s| s.as_str()).unwrap_or("").to_string();
    let sensitive = object.get("sensitive").and_then(|s| s.as_bool()).unwrap_or(false);
    let url = object.get("url").and_then(|u| u.as_str()).map(str::to_owned);
    let published = object.get("published").and_then(|p| p.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.with_timezone(&chrono::Utc));

    // Determine visibility
    let to = object.get("to").and_then(|v| v.as_array()).map(|a| {
        a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>()
    }).unwrap_or_default();
    let visibility = if to.contains(&"https://www.w3.org/ns/activitystreams#Public") {
        "public"
    } else {
        "unlisted"
    };

    // Resolve in_reply_to
    let in_reply_to_uri = object.get("inReplyTo").and_then(|v| v.as_str());
    let in_reply_to_id: Option<i64> = if let Some(uri) = in_reply_to_uri {
        sqlx::query_scalar!("SELECT id FROM statuses WHERE uri = $1", uri)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten()
    } else {
        None
    };

    // Resolve quote_of_id from FEP-044f `quote` property or compat `quoteUrl`
    let quote_uri = object.get("quote")
        .and_then(|v| v.as_str())
        .or_else(|| object.get("quoteUrl").and_then(|v| v.as_str()))
        .or_else(|| object.get("quoteUri").and_then(|v| v.as_str()))
        .or_else(|| object.get("_misskey_quote").and_then(|v| v.as_str()));
    let quote_of_id: Option<i64> = if let Some(uri) = quote_uri {
        sqlx::query_scalar!("SELECT id FROM statuses WHERE uri = $1", uri)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten()
    } else {
        None
    };

    let status_id = crate::snowflake::next_id();
    let created_at = published.unwrap_or_else(chrono::Utc::now);

    sqlx::query!(
        r#"INSERT INTO statuses
             (id, instance_id, account_id, text, spoiler_text, visibility, sensitive,
              uri, url, in_reply_to_id, quote_of_id, reply, created_at)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
           ON CONFLICT (uri) WHERE uri IS NOT NULL AND uri != '' DO NOTHING"#,
        status_id,
        instance.id,
        account_id,
        text,
        spoiler_text,
        visibility,
        sensitive,
        note_uri,
        url,
        in_reply_to_id,
        quote_of_id,
        in_reply_to_id.is_some(),
        created_at,
    )
    .execute(&state.db)
    .await?;

    // Increment quotes_count on locally-known quoted status
    if let Some(qid) = quote_of_id {
        let _ = sqlx::query!(
            "UPDATE statuses SET quotes_count = quotes_count + 1 WHERE id = $1",
            qid,
        )
        .execute(&state.db)
        .await;
    }

    Ok(())
}

async fn handle_delete(
    state: &AppState,
    _instance: &crate::db::models::Instance,
    activity: &Value,
) -> AppResult<()> {
    let object_uri = activity.get("object").and_then(|o| {
        if o.is_string() { o.as_str() } else { o.get("id").and_then(|i| i.as_str()) }
    });

    if let Some(uri) = object_uri {
        sqlx::query!(
            "UPDATE statuses SET deleted_at = now() WHERE uri = $1",
            uri
        )
        .execute(&state.db)
        .await?;
    }

    Ok(())
}

async fn handle_announce(
    _state: &AppState,
    _instance: &crate::db::models::Instance,
    _activity: &Value,
) -> AppResult<()> {
    // TODO: create boost (reblog) status
    Ok(())
}

async fn handle_like(
    state: &AppState,
    _instance: &crate::db::models::Instance,
    activity: &Value,
) -> AppResult<()> {
    let actor_uri = activity.get("actor").and_then(|a| a.as_str()).unwrap_or("");
    let object_uri = activity.get("object").and_then(|o| o.as_str()).unwrap_or("");
    let activity_uri = activity.get("id").and_then(|i| i.as_str()).unwrap_or("");

    let Some(status) = sqlx::query!("SELECT id FROM statuses WHERE uri = $1", object_uri)
        .fetch_optional(&state.db)
        .await? else { return Ok(()) };

    let Some(account_id) = sqlx::query_scalar!(
        "SELECT id FROM accounts WHERE uri = $1", actor_uri
    )
    .fetch_optional(&state.db)
    .await? else { return Ok(()) };

    sqlx::query!(
        "INSERT INTO favourites (account_id, status_id, uri) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING",
        account_id, status.id, activity_uri
    )
    .execute(&state.db)
    .await?;

    sqlx::query!(
        "UPDATE statuses SET favourites_count = (SELECT COUNT(*) FROM favourites WHERE status_id = $1) WHERE id = $1",
        status.id
    )
    .execute(&state.db)
    .await?;

    Ok(())
}

async fn handle_accept_reject(
    state: &AppState,
    _instance: &crate::db::models::Instance,
    activity: &Value,
) -> AppResult<()> {
    let activity_type = activity.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let object = activity.get("object");
    let follow_uri = object.and_then(|o| {
        if o.is_string() { o.as_str() } else { o.get("id").and_then(|i| i.as_str()) }
    });

    if let Some(uri) = follow_uri {
        if activity_type == "Accept" {
            sqlx::query!(
                "UPDATE follows SET state = 'accepted' WHERE uri = $1",
                uri
            )
            .execute(&state.db)
            .await?;
        } else {
            sqlx::query!("DELETE FROM follows WHERE uri = $1", uri)
                .execute(&state.db)
                .await?;
        }
    }

    Ok(())
}

async fn handle_update(
    _state: &AppState,
    _instance: &crate::db::models::Instance,
    _activity: &Value,
) -> AppResult<()> {
    // TODO: handle actor updates, status edits
    Ok(())
}

async fn handle_quote_request(
    state: &AppState,
    _instance: &crate::db::models::Instance,
    activity: &Value,
) -> AppResult<()> {
    let actor_uri = activity.get("actor").and_then(|a| a.as_str()).unwrap_or("");
    let object_uri = activity.get("object").and_then(|o| o.as_str()).unwrap_or("");

    if object_uri.is_empty() || actor_uri.is_empty() {
        return Ok(());
    }

    // Look up the local quoted status
    let Some(status) = sqlx::query!(
        "SELECT id, account_id, interaction_policy FROM statuses WHERE uri = $1 AND deleted_at IS NULL",
        object_uri,
    )
    .fetch_optional(&state.db)
    .await? else { return Ok(()) };

    // Look up or fetch the requesting remote account (ensure it exists locally)
    if resolve_or_fetch_remote_account(state, actor_uri).await.is_err() {
        return Ok(());
    }

    // Determine if the quoted status auto-approves quotes
    let always_public = status.interaction_policy.as_ref()
        .and_then(|p| p.get("can_quote"))
        .and_then(|cq| cq.get("always"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(|v| v.as_str() == Some("https://www.w3.org/ns/activitystreams#Public")))
        .unwrap_or(true); // default: public means auto-approve

    if always_public {
        // TODO: send Accept(QuoteRequest) via federation worker
        tracing::debug!(
            actor_uri,
            object_uri,
            "auto-accepting QuoteRequest"
        );
    } else {
        // TODO: queue pending quote approval notification for the local account owner
        tracing::debug!(
            actor_uri,
            object_uri,
            "queuing QuoteRequest for manual approval"
        );
    }

    Ok(())
}

/// Looks up a remote account by URI, fetching it from the remote server if unknown.
pub async fn resolve_or_fetch_remote_account(
    state: &AppState,
    actor_uri: &str,
) -> AppResult<i64> {
    if let Some(id) = sqlx::query_scalar!(
        "SELECT id FROM accounts WHERE uri = $1",
        actor_uri
    )
    .fetch_optional(&state.db)
    .await? {
        return Ok(id);
    }

    // Fetch the actor document
    let resp = state
        .http
        .get(actor_uri)
        .header("Accept", "application/activity+json, application/ld+json")
        .send()
        .await
        .map_err(|e| crate::error::AppError::Internal(e.into()))?;

    let actor: Value = resp.json().await.map_err(|e| crate::error::AppError::Internal(e.into()))?;

    let username = actor.get("preferredUsername").and_then(|u| u.as_str()).unwrap_or("unknown");
    let domain = url::Url::parse(actor_uri)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default();
    let display_name = actor.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
    let note = actor.get("summary").and_then(|s| s.as_str()).unwrap_or("").to_string();
    let url = actor.get("url").and_then(|u| u.as_str()).unwrap_or(actor_uri).to_string();
    let inbox_url = actor.get("inbox").and_then(|i| i.as_str()).unwrap_or("").to_string();
    let outbox_url = actor.get("outbox").and_then(|o| o.as_str()).unwrap_or("").to_string();
    let shared_inbox_url = actor
        .get("endpoints")
        .and_then(|e| e.get("sharedInbox"))
        .and_then(|s| s.as_str())
        .map(str::to_owned);
    let public_key = actor
        .get("publicKey")
        .and_then(|k| k.get("publicKeyPem"))
        .and_then(|p| p.as_str())
        .unwrap_or("")
        .to_string();

    // We need an instance_id for the remote domain — find or create a remote instance stub
    let remote_instance_id = get_or_create_remote_instance(state, &domain).await?;

    let new_account_id = crate::snowflake::next_id();
    let id = sqlx::query_scalar!(
        r#"INSERT INTO accounts
             (id, instance_id, username, domain, display_name, note, url, uri,
              inbox_url, outbox_url, shared_inbox_url, public_key)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
           ON CONFLICT (uri) WHERE uri != '' DO UPDATE
             SET display_name = EXCLUDED.display_name,
                 note = EXCLUDED.note,
                 public_key = EXCLUDED.public_key,
                 updated_at = now()
           RETURNING id"#,
        new_account_id,
        remote_instance_id,
        username,
        domain,
        display_name,
        note,
        url,
        actor_uri,
        inbox_url,
        outbox_url,
        shared_inbox_url,
        public_key,
    )
    .fetch_one(&state.db)
    .await?;

    Ok(id)
}

pub async fn get_or_create_remote_instance(state: &AppState, domain: &str) -> AppResult<uuid::Uuid> {
    if let Some(id) = sqlx::query_scalar!(
        "SELECT id FROM instances WHERE domain = $1",
        domain
    )
    .fetch_optional(&state.db)
    .await? {
        return Ok(id);
    }

    // Create a stub instance entry for this remote domain
    Ok(sqlx::query_scalar!(
        r#"INSERT INTO instances (domain, title, registrations_open, private_key, public_key)
           VALUES ($1, $1, false, '', '')
           ON CONFLICT (domain) DO UPDATE SET domain = EXCLUDED.domain
           RETURNING id"#,
        domain,
    )
    .fetch_one(&state.db)
    .await?)
}
