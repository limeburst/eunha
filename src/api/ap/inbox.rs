use axum::{
    extract::{Extension, OriginalUri, State},
    http::StatusCode,
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
    OriginalUri(uri): OriginalUri,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> AppResult<StatusCode> {
    let activity: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return Ok(StatusCode::BAD_REQUEST),
    };

    let activity_type = activity.get("type").and_then(|t| t.as_str()).unwrap_or("");

    tracing::debug!(
        instance = %instance.domain,
        activity_type,
        "received ActivityPub activity"
    );

    // Verify HTTP Signature; log failures but don't reject yet.
    if let Some(sig_header) = headers.get("signature") {
        if let Ok(sig_val) = sig_header.to_str() {
            if let Some(kid) = feder_core::signature::key_id_from_header(sig_val) {
                let actor_url = kid.split('#').next().unwrap_or(kid);
                match fetch_public_key(&state, actor_url).await {
                    Ok(pem) => {
                        let hdr_vec = crate::federation::signature::headers_to_vec(&headers);
                        let hdr_refs: Vec<(&str, &str)> =
                            hdr_vec.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                        if let Err(e) = feder_core::signature::verify_request(
                            "post", uri.path(), &hdr_refs, &body, &pem,
                        ) {
                            tracing::warn!(key_id = kid, error = %e, "HTTP Signature verification failed");
                        }
                    }
                    Err(e) => {
                        tracing::warn!(key_id = kid, error = %e, "could not fetch public key for verification");
                    }
                }
            }
        }
    }

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

async fn fetch_public_key(state: &AppState, actor_url: &str) -> anyhow::Result<String> {
    if let Some(pem) = sqlx::query_scalar!(
        "SELECT public_key FROM accounts WHERE uri = $1 AND public_key != ''",
        actor_url,
    )
    .fetch_optional(&state.db)
    .await?
    {
        return Ok(pem);
    }

    let resp = state
        .http
        .get(actor_url)
        .header("Accept", "application/activity+json, application/ld+json")
        .send()
        .await?;
    let actor: Value = resp.json().await?;
    let pem = actor
        .get("publicKey")
        .and_then(|k| k.get("publicKeyPem"))
        .and_then(|p| p.as_str())
        .ok_or_else(|| anyhow::anyhow!("no publicKeyPem"))?
        .to_string();
    Ok(pem)
}

async fn handle_follow(
    state: &AppState,
    instance: &crate::config::InstanceConfig,
    activity: &Value,
) -> AppResult<()> {
    let actor_uri = activity.get("actor").and_then(|a| a.as_str()).unwrap_or("");
    let object_uri = activity.get("object").and_then(|o| o.as_str()).unwrap_or("");
    let activity_uri = activity.get("id").and_then(|i| i.as_str()).unwrap_or("");

    let target = sqlx::query!(
        "SELECT id, locked, username, private_key FROM accounts WHERE uri = $1 AND domain IS NULL",
        object_uri,
    )
    .fetch_optional(&state.db)
    .await?;
    let Some(target) = target else { return Ok(()) };

    let follower_id = resolve_or_fetch_remote_account(state, actor_uri).await?;

    // Fetch the follower's account for push notification details
    let follower = sqlx::query!(
        "SELECT display_name, username, domain, avatar_remote_url FROM accounts WHERE id = $1",
        follower_id,
    )
    .fetch_optional(&state.db)
    .await?;

    if target.locked {
        sqlx::query!(
            r#"INSERT INTO follow_requests (account_id, target_account_id, uri)
               VALUES ($1, $2, $3)
               ON CONFLICT (account_id, target_account_id) DO UPDATE SET uri = EXCLUDED.uri"#,
            follower_id,
            target.id,
            activity_uri,
        )
        .execute(&state.db)
        .await?;

        // Notify the local user about the incoming follow request
        if let Some(ref f) = follower {
            let acct = match &f.domain {
                Some(d) => format!("{}@{}", f.username, d),
                None => f.username.clone(),
            };
            crate::push::create_and_push(
                state,
                target.id,
                follower_id,
                "follow_request",
                None,
                format!("{} wants to follow you", f.display_name),
                acct,
                f.avatar_remote_url.clone().unwrap_or_default(),
            )
            .await;
        }
    } else {
        sqlx::query!(
            r#"INSERT INTO follows (account_id, target_account_id, uri)
               VALUES ($1, $2, $3)
               ON CONFLICT (account_id, target_account_id) DO UPDATE SET uri = EXCLUDED.uri"#,
            follower_id,
            target.id,
            activity_uri,
        )
        .execute(&state.db)
        .await?;

        // Notify the local user about their new follower
        if let Some(ref f) = follower {
            let acct = match &f.domain {
                Some(d) => format!("{}@{}", f.username, d),
                None => f.username.clone(),
            };
            crate::push::create_and_push(
                state,
                target.id,
                follower_id,
                "follow",
                None,
                format!("{} followed you", f.display_name),
                acct,
                f.avatar_remote_url.clone().unwrap_or_default(),
            )
            .await;
        }

        // Send Accept back to the remote follower
        let accept_private_key = target.private_key.filter(|s| !s.is_empty());
        if accept_private_key.is_none() {
            tracing::warn!(username = %target.username, "local account has no private key; cannot send Accept");
        }
        if let Some(private_key) = accept_private_key {
            let follower_inbox = sqlx::query_scalar!(
                r#"SELECT CASE WHEN shared_inbox_url IS NOT NULL AND shared_inbox_url <> ''
                               THEN shared_inbox_url ELSE inbox_url END
                   FROM accounts WHERE id = $1"#,
                follower_id,
            )
            .fetch_optional(&state.db)
            .await?
            .flatten();

            match follower_inbox.filter(|s| !s.is_empty()) {
                None => {
                    tracing::warn!(actor_uri, "cannot send Accept: remote actor has no inbox URL");
                }
                Some(inbox) => {
                    let actor_url =
                        format!("https://{}/users/{}", instance.domain, target.username);
                    let accept_id = format!(
                        "https://{}/activities/{}",
                        instance.domain,
                        crate::snowflake::next_id()
                    );
                    let accept = feder_vocab::accept_follow(
                        &accept_id,
                        &actor_url,
                        activity_uri,
                        actor_uri,
                        &actor_url,
                    );
                    let key_id = format!("{}#main-key", actor_url);
                    let http = state.http.clone();
                    tracing::debug!(inbox, actor_uri, "delivering Accept");
                    tokio::spawn(async move {
                        if let Err(e) = crate::federation::delivery::deliver(
                            &http, &accept, &inbox, &key_id, &private_key,
                        )
                        .await
                        {
                            tracing::warn!(inbox, error = %e, "failed to deliver Accept");
                        }
                    });
                }
            }
        }
    }

    Ok(())
}

async fn handle_undo(
    state: &AppState,
    _instance: &crate::config::InstanceConfig,
    activity: &Value,
) -> AppResult<()> {
    let object = activity.get("object");
    let object_type = object.and_then(|o| o.get("type")).and_then(|t| t.as_str());

    if object_type == Some("Follow") {
        let follow_uri = object
            .and_then(|o| o.get("id"))
            .and_then(|i| i.as_str())
            .unwrap_or("");
        sqlx::query!("DELETE FROM follows WHERE uri = $1", follow_uri)
            .execute(&state.db)
            .await?;
        sqlx::query!("DELETE FROM follow_requests WHERE uri = $1", follow_uri)
            .execute(&state.db)
            .await?;
    }

    Ok(())
}

async fn handle_create(
    state: &AppState,
    _instance: &crate::config::InstanceConfig,
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

    let account_id = match resolve_or_fetch_remote_account(state, actor_uri).await {
        Ok(id) => id,
        Err(_) => return Ok(()),
    };

    let text = object
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    let spoiler_text = object
        .get("summary")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let sensitive = object
        .get("sensitive")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);
    let url = object.get("url").and_then(|u| u.as_str()).map(str::to_owned);
    let published = object
        .get("published")
        .and_then(|p| p.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.with_timezone(&chrono::Utc));

    let to = object
        .get("to")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();
    let visibility = if to.contains(&"https://www.w3.org/ns/activitystreams#Public") {
        crate::db::models::vis::PUBLIC
    } else {
        crate::db::models::vis::UNLISTED
    };

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

    let quote_uri = object
        .get("quote")
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
             (id, account_id, text, spoiler_text, visibility, sensitive,
              uri, url, in_reply_to_id, reply, created_at)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
           ON CONFLICT (uri) WHERE uri IS NOT NULL AND uri != '' DO NOTHING"#,
        status_id,
        account_id,
        text,
        spoiler_text,
        visibility,
        sensitive,
        note_uri,
        url,
        in_reply_to_id,
        in_reply_to_id.is_some(),
        created_at,
    )
    .execute(&state.db)
    .await?;

    if let Some(qid) = quote_of_id {
        let _ = sqlx::query!(
            "INSERT INTO quotes (status_id, quoted_status_id, state) VALUES ($1, $2, 1) ON CONFLICT DO NOTHING",
            status_id,
            qid,
        )
        .execute(&state.db)
        .await;
    }

    Ok(())
}

async fn handle_delete(
    state: &AppState,
    _instance: &crate::config::InstanceConfig,
    activity: &Value,
) -> AppResult<()> {
    let object_uri = activity.get("object").and_then(|o| {
        if o.is_string() {
            o.as_str()
        } else {
            o.get("id").and_then(|i| i.as_str())
        }
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
    state: &AppState,
    _instance: &crate::config::InstanceConfig,
    activity: &Value,
) -> AppResult<()> {
    let actor_uri = activity.get("actor").and_then(|a| a.as_str()).unwrap_or("");
    let object = activity.get("object");
    let announce_uri = activity.get("id").and_then(|i| i.as_str()).unwrap_or("");

    // object can be a URI string or an embedded object
    let boosted_uri = object.and_then(|o| {
        if o.is_string() {
            o.as_str()
        } else {
            o.get("id").and_then(|i| i.as_str())
        }
    });

    let Some(boosted_uri) = boosted_uri else {
        return Ok(());
    };
    if actor_uri.is_empty() || announce_uri.is_empty() {
        return Ok(());
    }

    let booster_id = match resolve_or_fetch_remote_account(state, actor_uri).await {
        Ok(id) => id,
        Err(_) => return Ok(()),
    };

    // Find the boosted status in our database
    let original = sqlx::query!(
        "SELECT id FROM statuses WHERE uri = $1 AND deleted_at IS NULL",
        boosted_uri,
    )
    .fetch_optional(&state.db)
    .await?;

    let Some(original) = original else {
        // We don't have this status locally; skip
        return Ok(());
    };

    let published = activity
        .get("published")
        .and_then(|p| p.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.with_timezone(&chrono::Utc))
        .unwrap_or_else(chrono::Utc::now);

    let boost_id = crate::snowflake::next_id();
    sqlx::query!(
        r#"INSERT INTO statuses
             (id, account_id, reblog_of_id, visibility, uri, url, created_at)
           VALUES ($1, $2, $3, $4, $5, $5, $6)
           ON CONFLICT (uri) WHERE uri IS NOT NULL AND uri != '' DO NOTHING"#,
        boost_id,
        booster_id,
        original.id,
        crate::db::models::vis::PUBLIC,
        announce_uri,
        published,
    )
    .execute(&state.db)
    .await?;

    // Update the original status's reblogs_count
    let _ = sqlx::query!(
        r#"INSERT INTO status_stats (status_id, reblogs_count, created_at, updated_at)
           VALUES ($1, 1, now(), now())
           ON CONFLICT (status_id) DO UPDATE
             SET reblogs_count = (SELECT COUNT(*) FROM statuses
                                  WHERE reblog_of_id = $1 AND deleted_at IS NULL),
                 updated_at = now()"#,
        original.id,
    )
    .execute(&state.db)
    .await;

    Ok(())
}

async fn handle_like(
    state: &AppState,
    _instance: &crate::config::InstanceConfig,
    activity: &Value,
) -> AppResult<()> {
    let actor_uri = activity.get("actor").and_then(|a| a.as_str()).unwrap_or("");
    let object_uri = activity.get("object").and_then(|o| o.as_str()).unwrap_or("");

    let Some(status) = sqlx::query!("SELECT id FROM statuses WHERE uri = $1", object_uri)
        .fetch_optional(&state.db)
        .await?
    else {
        return Ok(());
    };

    let Some(account_id) =
        sqlx::query_scalar!("SELECT id FROM accounts WHERE uri = $1", actor_uri)
            .fetch_optional(&state.db)
            .await?
    else {
        return Ok(());
    };

    sqlx::query!(
        "INSERT INTO favourites (account_id, status_id) VALUES ($1,$2) ON CONFLICT DO NOTHING",
        account_id,
        status.id
    )
    .execute(&state.db)
    .await?;

    sqlx::query!(
        r#"INSERT INTO status_stats (status_id, favourites_count, created_at, updated_at)
           VALUES ($1, (SELECT COUNT(*) FROM favourites WHERE status_id = $1), now(), now())
           ON CONFLICT (status_id) DO UPDATE
             SET favourites_count = (SELECT COUNT(*) FROM favourites WHERE status_id = $1),
                 updated_at = now()"#,
        status.id
    )
    .execute(&state.db)
    .await?;

    Ok(())
}

async fn handle_accept_reject(
    state: &AppState,
    _instance: &crate::config::InstanceConfig,
    activity: &Value,
) -> AppResult<()> {
    let activity_type = activity.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let object = activity.get("object");
    let follow_uri = object.and_then(|o| {
        if o.is_string() {
            o.as_str()
        } else {
            o.get("id").and_then(|i| i.as_str())
        }
    });

    if let Some(uri) = follow_uri {
        if activity_type == "Accept" {
            // Promote follow_request → follows when remote accepts our Follow
            let promoted = sqlx::query!(
                "DELETE FROM follow_requests WHERE uri = $1 RETURNING account_id, target_account_id",
                uri
            )
            .fetch_optional(&state.db)
            .await?;
            if let Some(row) = promoted {
                sqlx::query!(
                    r#"INSERT INTO follows (account_id, target_account_id, uri)
                       VALUES ($1, $2, $3) ON CONFLICT DO NOTHING"#,
                    row.account_id,
                    row.target_account_id,
                    uri
                )
                .execute(&state.db)
                .await?;

                // Update follower/following counts
                let _ = sqlx::query!(
                    r#"INSERT INTO account_stats (account_id, followers_count, created_at, updated_at)
                       VALUES ($1, 1, now(), now())
                       ON CONFLICT (account_id) DO UPDATE
                         SET followers_count = account_stats.followers_count + 1, updated_at = now()"#,
                    row.target_account_id,
                )
                .execute(&state.db)
                .await;
                let _ = sqlx::query!(
                    r#"INSERT INTO account_stats (account_id, following_count, created_at, updated_at)
                       VALUES ($1, 1, now(), now())
                       ON CONFLICT (account_id) DO UPDATE
                         SET following_count = account_stats.following_count + 1, updated_at = now()"#,
                    row.account_id,
                )
                .execute(&state.db)
                .await;
            }
        } else {
            sqlx::query!("DELETE FROM follow_requests WHERE uri = $1", uri)
                .execute(&state.db)
                .await?;
        }
    }

    Ok(())
}

async fn handle_update(
    state: &AppState,
    _instance: &crate::config::InstanceConfig,
    activity: &Value,
) -> AppResult<()> {
    let object = match activity.get("object") {
        Some(o) if o.is_object() => o,
        _ => return Ok(()),
    };

    let obj_type = object.get("type").and_then(|t| t.as_str()).unwrap_or("");
    match obj_type {
        "Person" | "Service" | "Application" | "Group" | "Organization" => {
            let actor_uri = object.get("id").and_then(|i| i.as_str()).unwrap_or("");
            if actor_uri.is_empty() {
                return Ok(());
            }

            let display_name = object
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let note = object
                .get("summary")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            let inbox_url = object
                .get("inbox")
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .to_string();
            let shared_inbox_url = object
                .get("endpoints")
                .and_then(|e| e.get("sharedInbox"))
                .and_then(|s| s.as_str())
                .map(str::to_owned);
            let public_key = object
                .get("publicKey")
                .and_then(|k| k.get("publicKeyPem"))
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();
            let locked = object
                .get("manuallyApprovesFollowers")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            sqlx::query!(
                r#"UPDATE accounts
                   SET display_name = $2, note = $3, inbox_url = $4,
                       shared_inbox_url = $5, public_key = $6, locked = $7,
                       updated_at = now()
                   WHERE uri = $1 AND domain IS NOT NULL"#,
                actor_uri,
                display_name,
                note,
                inbox_url,
                shared_inbox_url,
                public_key,
                locked,
            )
            .execute(&state.db)
            .await?;
        }
        "Note" => {
            // TODO: handle remote status edits (Update(Note))
        }
        _ => {}
    }

    Ok(())
}

async fn handle_quote_request(
    state: &AppState,
    _instance: &crate::config::InstanceConfig,
    activity: &Value,
) -> AppResult<()> {
    let actor_uri = activity.get("actor").and_then(|a| a.as_str()).unwrap_or("");
    let object_uri = activity.get("object").and_then(|o| o.as_str()).unwrap_or("");

    if object_uri.is_empty() || actor_uri.is_empty() {
        return Ok(());
    }

    let Some(status) = sqlx::query!(
        "SELECT id, account_id, quote_approval_policy FROM statuses WHERE uri = $1 AND deleted_at IS NULL",
        object_uri,
    )
    .fetch_optional(&state.db)
    .await?
    else {
        return Ok(());
    };

    if resolve_or_fetch_remote_account(state, actor_uri)
        .await
        .is_err()
    {
        return Ok(());
    }

    let always_public = status.quote_approval_policy == 0;
    if always_public {
        tracing::debug!(actor_uri, object_uri, "auto-accepting QuoteRequest");
        // TODO: send Accept(QuoteRequest) via federation worker
    } else {
        tracing::debug!(actor_uri, object_uri, "queuing QuoteRequest for manual approval");
        // TODO: queue pending quote approval notification for the local account owner
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
    .await?
    {
        return Ok(id);
    }

    let resp = state
        .http
        .get(actor_uri)
        .header("Accept", "application/activity+json, application/ld+json")
        .send()
        .await
        .map_err(|e| crate::error::AppError::Internal(e.into()))?;

    let actor: Value = resp
        .json()
        .await
        .map_err(|e| crate::error::AppError::Internal(e.into()))?;

    let username = actor
        .get("preferredUsername")
        .and_then(|u| u.as_str())
        .unwrap_or("unknown");
    let domain = url::Url::parse(actor_uri)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default();
    let display_name = actor
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let note = actor
        .get("summary")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let url = actor
        .get("url")
        .and_then(|u| u.as_str())
        .unwrap_or(actor_uri)
        .to_string();
    let inbox_url = actor
        .get("inbox")
        .and_then(|i| i.as_str())
        .unwrap_or("")
        .to_string();
    let outbox_url = actor
        .get("outbox")
        .and_then(|o| o.as_str())
        .unwrap_or("")
        .to_string();
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

    let new_id = crate::snowflake::next_id();
    let id = sqlx::query_scalar!(
        r#"INSERT INTO accounts
             (id, username, domain, display_name, note, url, uri,
              inbox_url, outbox_url, shared_inbox_url, public_key)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
           ON CONFLICT (uri) WHERE uri != '' DO UPDATE
             SET display_name = EXCLUDED.display_name,
                 note = EXCLUDED.note,
                 inbox_url = EXCLUDED.inbox_url,
                 shared_inbox_url = EXCLUDED.shared_inbox_url,
                 public_key = EXCLUDED.public_key,
                 updated_at = now()
           RETURNING id"#,
        new_id,
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
