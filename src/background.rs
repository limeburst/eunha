use std::time::Duration;
use uuid::Uuid;

use crate::state::AppState;

/// Spawns all background tasks. Called once at startup.
pub fn spawn(state: AppState) {
    tokio::spawn(run_scheduled_statuses(state.clone()));
    tokio::spawn(run_poll_expiry(state));
}

// ── Scheduled status publisher ────────────────────────────────────────────

async fn run_scheduled_statuses(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        if let Err(e) = publish_due_statuses(&state).await {
            tracing::error!(error = %e, "scheduled status publish failed");
        }
    }
}

async fn publish_due_statuses(state: &AppState) -> anyhow::Result<()> {
    let rows = sqlx::query!(
        r#"SELECT id, account_id, params
           FROM scheduled_statuses
           WHERE scheduled_at <= now()
           ORDER BY scheduled_at ASC
           LIMIT 50"#,
    )
    .fetch_all(&state.db)
    .await?;

    for row in rows {
        match publish_one(state, row.id, row.account_id, &row.params).await {
            Ok(()) => {
                sqlx::query!("DELETE FROM scheduled_statuses WHERE id = $1", row.id)
                    .execute(&state.db)
                    .await?;
            }
            Err(e) => {
                tracing::warn!(id = row.id, error = %e, "failed to publish scheduled status");
                // Delete anyway to avoid retrying indefinitely on bad params
                sqlx::query!("DELETE FROM scheduled_statuses WHERE id = $1", row.id)
                    .execute(&state.db)
                    .await?;
            }
        }
    }
    Ok(())
}

async fn publish_one(
    state: &AppState,
    _scheduled_id: i64,
    account_id: Uuid,
    params: &Option<serde_json::Value>,
) -> anyhow::Result<()> {
    let params = params.as_ref().ok_or_else(|| anyhow::anyhow!("no params"))?;

    let account = sqlx::query_as!(
        crate::db::models::Account,
        "SELECT * FROM accounts WHERE id = $1",
        account_id,
    )
    .fetch_one(&state.db)
    .await?;

    let instance = sqlx::query_as!(
        crate::db::models::Instance,
        "SELECT * FROM instances WHERE id = $1",
        account.instance_id,
    )
    .fetch_one(&state.db)
    .await?;

    let text = params["text"].as_str().unwrap_or("").to_string();
    let visibility = params["visibility"].as_str().unwrap_or("public");
    let spoiler_text = params["spoiler_text"].as_str().unwrap_or("").to_string();
    let sensitive = params["sensitive"].as_bool().unwrap_or(false);
    let language = params["language"].as_str().map(str::to_string);
    let in_reply_to_id = params["in_reply_to_id"]
        .as_str()
        .and_then(|s| s.parse::<i64>().ok());

    use crate::api::mastodon::statuses::{
        extract_hashtags, extract_mention_handles, resolve_mention_accounts,
        build_mention_map, render_content, store_status_tags, store_status_mentions,
    };

    let hashtags = extract_hashtags(&text);
    let mention_handles = extract_mention_handles(&text);
    let resolved = resolve_mention_accounts(state, instance.id, &mention_handles).await;
    let mention_map = build_mention_map(&resolved);
    let content = render_content(&text, &instance.domain, &mention_map);

    let status = sqlx::query_as!(
        crate::db::models::Status,
        r#"INSERT INTO statuses
             (instance_id, account_id, text, content, spoiler_text, visibility,
              language, sensitive, in_reply_to_id)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
           RETURNING *"#,
        instance.id, account.id, text, content, spoiler_text, visibility,
        language, sensitive, in_reply_to_id,
    )
    .fetch_one(&state.db)
    .await?;

    let uri = format!(
        "https://{}/users/{}/statuses/{}",
        instance.domain, account.username, status.id
    );
    sqlx::query!(
        "UPDATE statuses SET uri = $1, url = $1 WHERE id = $2",
        uri, status.id,
    )
    .execute(&state.db)
    .await?;

    store_status_tags(state, status.id, &hashtags).await?;
    store_status_mentions(state, status.id, &resolved).await?;

    sqlx::query!(
        "UPDATE accounts SET statuses_count = statuses_count + 1 WHERE id = $1",
        account.id,
    )
    .execute(&state.db)
    .await?;

    // Attach media ids if any
    if let Some(ids) = params["media_ids"].as_array() {
        for id_val in ids {
            if let Some(id_str) = id_val.as_str() {
                if let Ok(media_id) = id_str.parse::<i64>() {
                    sqlx::query!(
                        "UPDATE media_attachments SET status_id = $1 WHERE id = $2 AND account_id = $3 AND status_id IS NULL",
                        status.id, media_id, account.id,
                    )
                    .execute(&state.db)
                    .await?;
                }
            }
        }
    }

    // Create poll if present
    if let Some(poll) = params["poll"].as_object() {
        if let Some(options) = poll.get("options").and_then(|o| o.as_array()) {
            if options.len() >= 2 {
                let expires_in = poll.get("expires_in").and_then(|v| v.as_i64());
                let multiple = poll.get("multiple").and_then(|v| v.as_bool()).unwrap_or(false);
                let expires_at = expires_in.map(|s| chrono::Utc::now() + chrono::Duration::seconds(s));
                let opts_json = serde_json::Value::Array(
                    options.iter()
                        .filter_map(|o| o.as_str())
                        .map(|o| serde_json::json!({ "title": o, "votes_count": 0 }))
                        .collect(),
                );
                sqlx::query!(
                    "INSERT INTO polls (status_id, account_id, options, multiple, expires_at) VALUES ($1,$2,$3,$4,$5)",
                    status.id, account.id, opts_json, multiple, expires_at,
                )
                .execute(&state.db)
                .await?;
            }
        }
    }

    // Publish to streaming
    use crate::api::mastodon::accounts::{build_status, fetch_status_media, spawn_card_fetch};
    let mut status_with_uri = status.clone();
    status_with_uri.uri = Some(uri);
    spawn_card_fetch(state, status_with_uri.id, status_with_uri.content.clone());
    if let Ok(media) = fetch_status_media(state, status_with_uri.id).await {
        if let Ok(api_status) = build_status(state, &status_with_uri, &account, media, None, None).await {
            if matches!(visibility, "public" | "unlisted" | "private") {
                if let Ok(payload) = serde_json::to_string(&api_status) {
                    let hashtags: Vec<String> = api_status.tags.iter().map(|t| t.name.clone()).collect();
                    state.streaming.publish(crate::streaming::Event::NewStatus {
                        instance_id: instance.id,
                        author_id: account.id,
                        is_public: visibility == "public",
                        is_direct: visibility == "direct",
                        status_id: status_with_uri.id,
                        hashtags,
                        payload: std::sync::Arc::new(payload),
                    });
                }
            }
        }
    }

    Ok(())
}

// ── Poll expiry notifier ──────────────────────────────────────────────────

async fn run_poll_expiry(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        if let Err(e) = notify_expired_polls(&state).await {
            tracing::error!(error = %e, "poll expiry task failed");
        }
    }
}

async fn notify_expired_polls(state: &AppState) -> anyhow::Result<()> {
    // Find polls that just expired and haven't had expiry notifications sent yet.
    // We track this with a simple approach: notify all unique voters + the poll author
    // for polls that expired in the last 2 minutes (our tick interval + buffer).
    let expired = sqlx::query!(
        r#"SELECT p.id, p.status_id, p.account_id
           FROM polls p
           WHERE p.expires_at IS NOT NULL
             AND p.expires_at <= now()
             AND p.expires_at > now() - interval '2 minutes'
           LIMIT 100"#,
    )
    .fetch_all(&state.db)
    .await?;

    for poll in expired {
        // Collect recipients: poll author + all voters
        let mut recipients: Vec<Uuid> = vec![poll.account_id];
        let voters = sqlx::query_scalar!(
            "SELECT DISTINCT account_id FROM poll_votes WHERE poll_id = $1",
            poll.id,
        )
        .fetch_all(&state.db)
        .await?;
        recipients.extend(voters);
        recipients.dedup();

        for recipient_id in recipients {
            crate::push::create_and_push(
                state,
                recipient_id,
                poll.account_id,
                "poll",
                Some(poll.status_id),
                "A poll you voted in has ended".into(),
                "".into(),
                "".into(),
            )
            .await;
        }
    }
    Ok(())
}
