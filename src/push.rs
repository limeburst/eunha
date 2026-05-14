use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use p256::{
    ecdsa::SigningKey,
    pkcs8::{EncodePrivateKey, LineEnding},
};
use uuid::Uuid;

use crate::state::AppState;

// ── VAPID key generation ───────────────────────────────────────────────────

/// Generates a P-256 VAPID keypair.
/// Returns (pkcs8_pem, public_key_base64url).
/// The PEM is stored in the DB; the base64url key is returned to clients.
pub fn generate_vapid_keypair() -> anyhow::Result<(String, String)> {
    use p256::elliptic_curve::rand_core::OsRng;
    let signing_key = SigningKey::random(&mut OsRng);
    let pem = signing_key
        .to_pkcs8_pem(LineEnding::LF)
        .map_err(|e| anyhow::anyhow!("pkcs8 encode: {e}"))?;

    let pub_point = signing_key
        .verifying_key()
        .to_encoded_point(false); // uncompressed 65-byte point
    let pub_b64 = URL_SAFE_NO_PAD.encode(pub_point.as_bytes());

    Ok((pem.to_string(), pub_b64))
}

/// Ensures the instance has a VAPID keypair, generating one if missing.
pub async fn ensure_vapid_keys(state: &AppState, instance_id: Uuid) -> anyhow::Result<()> {
    let needs_keys = sqlx::query_scalar!(
        "SELECT vapid_private_key = '' FROM instances WHERE id = $1",
        instance_id,
    )
    .fetch_optional(&state.db)
    .await?
    .flatten()
    .unwrap_or(true);

    if needs_keys {
        let (private_pem, public_b64) = generate_vapid_keypair()?;
        sqlx::query!(
            "UPDATE instances SET vapid_private_key = $1, vapid_public_key = $2 WHERE id = $3",
            private_pem,
            public_b64,
            instance_id,
        )
        .execute(&state.db)
        .await?;
        tracing::info!(instance_id = %instance_id, "generated VAPID keypair");
    }

    Ok(())
}

// ── Push delivery ──────────────────────────────────────────────────────────

/// Payload sent to the push endpoint, matching Mastodon's format.
#[derive(serde::Serialize)]
struct PushPayload<'a> {
    notification_id: i64,
    notification_type: &'a str,
    icon: &'a str,
    title: &'a str,
    body: &'a str,
    preferred_locale: &'a str,
}

/// Deliver a push notification to all subscriptions registered for `recipient_id`
/// where the corresponding alert type is enabled.
/// Failures are logged and swallowed — push is best-effort.
pub async fn deliver(
    state: AppState,
    recipient_id: Uuid,
    notification_id: i64,
    notification_type: &str,
    icon: &str,
    title: &str,
    body: &str,
) {
    if let Err(e) = try_deliver(
        &state,
        recipient_id,
        notification_id,
        notification_type,
        icon,
        title,
        body,
    )
    .await
    {
        tracing::warn!(error = %e, "push delivery error");
    }
}

async fn try_deliver(
    state: &AppState,
    recipient_id: Uuid,
    notification_id: i64,
    notification_type: &str,
    icon: &str,
    title: &str,
    body: &str,
) -> anyhow::Result<()> {
    // Look up subscriptions for the recipient that have this alert type enabled.
    let alert_col = match notification_type {
        "follow" | "follow_request" => "alert_follow",
        "favourite" => "alert_favourite",
        "reblog" => "alert_reblog",
        "mention" => "alert_mention",
        "poll" => "alert_poll",
        "status" => "alert_status",
        _ => return Ok(()),
    };

    let subs_query = format!(
        r#"SELECT wps.id, wps.endpoint, wps.p256dh, wps.auth,
                  i.vapid_private_key, i.vapid_public_key
           FROM web_push_subscriptions wps
           JOIN accounts a ON a.id = wps.account_id
           JOIN instances i ON i.id = a.instance_id
           WHERE wps.account_id = $1
             AND wps.{alert_col} = true"#
    );

    let rows = sqlx::query_as::<_, (i64, String, String, String, String, String)>(
        &subs_query,
    )
    .bind(recipient_id)
    .fetch_all(&state.db)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }

    let payload = serde_json::to_string(&PushPayload {
        notification_id,
        notification_type,
        icon,
        title,
        body,
        preferred_locale: "en",
    })?;

    for (_, endpoint, p256dh, auth, vapid_priv, _vapid_pub) in rows {
        if let Err(e) = send_one(
            state,
            &endpoint,
            &p256dh,
            &auth,
            &vapid_priv,
            &payload,
        )
        .await
        {
            tracing::warn!(
                endpoint = %endpoint,
                error = %e,
                "push send failed"
            );
        }
    }

    Ok(())
}

async fn send_one(
    state: &AppState,
    endpoint: &str,
    p256dh: &str,
    auth: &str,
    vapid_private_pem: &str,
    payload: &str,
) -> anyhow::Result<()> {
    use web_push::{
        ContentEncoding, SubscriptionInfo, SubscriptionKeys, VapidSignatureBuilder,
        WebPushMessageBuilder,
    };

    let sub_info = SubscriptionInfo {
        endpoint: endpoint.to_string(),
        keys: SubscriptionKeys {
            auth: auth.to_string(),
            p256dh: p256dh.to_string(),
        },
    };

    let mut builder = WebPushMessageBuilder::new(&sub_info);
    builder.set_payload(ContentEncoding::AesGcm, payload.as_bytes());
    builder.set_ttl(86400);

    let sig_builder = VapidSignatureBuilder::from_pem(
        vapid_private_pem.as_bytes(),
        &sub_info,
    )?;
    builder.set_vapid_signature(sig_builder.build()?);

    let message = builder.build()?;

    send_with_reqwest(&state.http, message).await
}

async fn send_with_reqwest(
    http: &reqwest::Client,
    message: web_push::WebPushMessage,
) -> anyhow::Result<()> {
    let endpoint = message.endpoint.to_string();
    let ttl = message.ttl;
    let mut req = http
        .post(endpoint.as_str())
        .header("TTL", ttl.to_string());

    if let Some(payload) = message.payload {
        req = req
            .header("Content-Encoding", payload.content_encoding.to_str())
            .header("Content-Type", "application/octet-stream");

        for (k, v) in &payload.crypto_headers {
            req = req.header(*k, v.as_str());
        }

        req = req.body(payload.content);
    }

    let resp = req.send().await?;
    let status = resp.status();
    if !status.is_success() && status.as_u16() != 201 {
        let body = resp.text().await.unwrap_or_default();
        tracing::warn!(status = %status, body = %body, endpoint = %endpoint, "push relay rejected message");
    }

    Ok(())
}

// ── Notification creation helper ───────────────────────────────────────────

/// Insert a notification record and fire push delivery in a background task.
pub async fn create_and_push(
    state: &AppState,
    recipient_id: Uuid,
    from_account_id: Uuid,
    notification_type: &'static str,
    status_id: Option<i64>,
    title: String,
    body: String,
    icon: String,
) {
    let db = state.db.clone();

    // Don't notify yourself
    if recipient_id == from_account_id {
        return;
    }

    // Check notification policy: route to notification_requests if filtered
    if should_filter_notification(&db, recipient_id, from_account_id, notification_type, status_id).await {
        route_to_request(&db, recipient_id, from_account_id, status_id).await;
        return;
    }

    // Dedup: don't insert the same (account, from, type, status) twice
    let existing = sqlx::query_scalar!(
        r#"SELECT 1 FROM notifications
           WHERE account_id = $1 AND from_account_id = $2
             AND notification_type = $3
             AND (status_id = $4 OR ($4::bigint IS NULL AND status_id IS NULL))
           LIMIT 1"#,
        recipient_id,
        from_account_id,
        notification_type,
        status_id,
    )
    .fetch_optional(&db)
    .await;

    if matches!(existing, Ok(Some(_))) {
        return;
    }

    let row = sqlx::query!(
        r#"INSERT INTO notifications (account_id, from_account_id, notification_type, status_id)
           VALUES ($1, $2, $3, $4)
           RETURNING id"#,
        recipient_id,
        from_account_id,
        notification_type,
        status_id,
    )
    .fetch_one(&db)
    .await;

    let notification_id = match row {
        Ok(r) => r.id,
        Err(e) => {
            tracing::warn!(error = %e, "failed to create notification");
            return;
        }
    };

    // Publish to the streaming API synchronously — it's just an in-process broadcast.
    if let Some(payload) = build_notification_payload(state, notification_id, notification_type, from_account_id, status_id).await {
        state.streaming.publish(crate::streaming::Event::Notification {
            for_account_id: recipient_id,
            payload: std::sync::Arc::new(payload),
        });
    }

    // Send email notification if Resend is configured
    if matches!(notification_type, "mention" | "follow" | "favourite" | "reblog") {
        if let Some(ref resend) = state.config.resend {
            let email_info = sqlx::query!(
                r#"SELECT u.email, a.username, a.display_name, i.domain, i.custom_domain
                   FROM users u
                   JOIN accounts a ON a.id = u.account_id
                   JOIN instances i ON i.id = u.instance_id
                   WHERE u.account_id = $1 AND u.confirmed_at IS NOT NULL"#,
                recipient_id,
            )
            .fetch_optional(&state.db)
            .await;
            if let Ok(Some(row)) = email_info {
                let actor_info = sqlx::query!(
                    "SELECT username, display_name FROM accounts WHERE id = $1",
                    from_account_id,
                )
                .fetch_optional(&state.db)
                .await;
                if let Ok(Some(actor)) = actor_info {
                    let actor_name = if actor.display_name.is_empty() {
                        format!("@{}", actor.username)
                    } else {
                        actor.display_name.clone()
                    };
                    let instance_domain = row.custom_domain.as_deref().unwrap_or(&row.domain);
                    let instance_url = format!("https://{}", instance_domain);
                    let recipient_name = if row.display_name.is_empty() {
                        row.username.clone()
                    } else {
                        row.display_name.clone()
                    };
                    let http = state.http.clone();
                    let api_key = resend.api_key.clone();
                    let from_addr = resend.from.clone();
                    let to_addr = row.email.clone();
                    tokio::spawn(async move {
                        if let Err(e) = crate::email::send_notification(
                            &http, &api_key, &from_addr, &to_addr,
                            &recipient_name, notification_type, &actor_name,
                            &instance_url, "en",
                        ).await {
                            tracing::debug!(error = %e, "notification email failed (non-fatal)");
                        }
                    });
                }
            }
        }
    }

    let state_clone = state.clone();
    let icon_s = icon;
    let title_s = title;
    let body_s = body;
    tokio::spawn(async move {
        deliver(
            state_clone,
            recipient_id,
            notification_id,
            notification_type,
            &icon_s,
            &title_s,
            &body_s,
        )
        .await;
    });
}

/// Returns true if the notification should be routed to notification_requests
/// instead of the main notifications feed, based on the recipient's policy.
async fn should_filter_notification(
    db: &sqlx::PgPool,
    recipient_id: Uuid,
    from_account_id: Uuid,
    notification_type: &str,
    status_id: Option<i64>,
) -> bool {
    // Only filter certain notification types (not polls or admin actions)
    if !matches!(notification_type, "follow" | "follow_request" | "mention" | "favourite" | "reblog") {
        return false;
    }

    let policy = sqlx::query!(
        r#"SELECT notif_filter_not_following, notif_filter_not_followers,
                  notif_filter_new_accounts, notif_filter_private_mentions
           FROM users WHERE account_id = $1"#,
        recipient_id,
    )
    .fetch_optional(db)
    .await
    .ok()
    .flatten();

    let Some(policy) = policy else {
        return false; // No user row = remote account, don't filter
    };

    if !policy.notif_filter_not_following
        && !policy.notif_filter_not_followers
        && !policy.notif_filter_new_accounts
        && !policy.notif_filter_private_mentions
    {
        return false; // All filters off
    }

    // filter_not_following: sender is not followed by recipient
    if policy.notif_filter_not_following {
        let follows = sqlx::query_scalar!(
            "SELECT 1 FROM follows WHERE account_id = $1 AND target_account_id = $2 AND state = 'accepted'",
            recipient_id, from_account_id,
        )
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .is_some();
        if !follows {
            return true;
        }
    }

    // filter_not_followers: sender does not follow recipient
    if policy.notif_filter_not_followers {
        let is_follower = sqlx::query_scalar!(
            "SELECT 1 FROM follows WHERE account_id = $1 AND target_account_id = $2 AND state = 'accepted'",
            from_account_id, recipient_id,
        )
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .is_some();
        if !is_follower {
            return true;
        }
    }

    // filter_new_accounts: sender account is less than 30 days old
    if policy.notif_filter_new_accounts {
        let is_new = sqlx::query_scalar!(
            "SELECT 1 FROM accounts WHERE id = $1 AND created_at > now() - interval '30 days'",
            from_account_id,
        )
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .is_some();
        if is_new {
            return true;
        }
    }

    // filter_private_mentions: unsolicited DM (direct mention, not a reply to own status)
    if policy.notif_filter_private_mentions && notification_type == "mention" {
        if let Some(sid) = status_id {
            let is_private_unsolicited = sqlx::query_scalar!(
                r#"SELECT 1 FROM statuses s
                   WHERE s.id = $1
                     AND s.visibility = 'direct'
                     AND (s.in_reply_to_id IS NULL OR NOT EXISTS (
                       SELECT 1 FROM statuses parent
                       WHERE parent.id = s.in_reply_to_id
                         AND parent.account_id = $2
                     ))"#,
                sid, recipient_id,
            )
            .fetch_optional(db)
            .await
            .ok()
            .flatten()
            .is_some();
            if is_private_unsolicited {
                return true;
            }
        }
    }

    false
}

/// Upsert into notification_requests for a filtered notification.
async fn route_to_request(
    db: &sqlx::PgPool,
    recipient_id: Uuid,
    from_account_id: Uuid,
    status_id: Option<i64>,
) {
    let _ = sqlx::query!(
        r#"INSERT INTO notification_requests
               (account_id, from_account_id, last_status_id, notifications_count)
           VALUES ($1, $2, $3, 1)
           ON CONFLICT (account_id, from_account_id) DO UPDATE
             SET notifications_count = notification_requests.notifications_count + 1,
                 last_status_id = COALESCE($3, notification_requests.last_status_id),
                 dismissed = false,
                 updated_at = now()"#,
        recipient_id,
        from_account_id,
        status_id,
    )
    .execute(db)
    .await;
}

async fn build_notification_payload(
    state: &AppState,
    notification_id: i64,
    notification_type: &str,
    from_account_id: Uuid,
    status_id: Option<i64>,
) -> Option<String> {
    use crate::api::mastodon::convert::account_from_db;
    use crate::api::mastodon::accounts::{build_status, fetch_reblog_data, fetch_status_media};

    let created_at = sqlx::query_scalar!(
        "SELECT created_at FROM notifications WHERE id = $1",
        notification_id
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()?;

    let from_account = sqlx::query_as!(
        crate::db::models::Account,
        "SELECT * FROM accounts WHERE id = $1",
        from_account_id,
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()?;

    let api_account = account_from_db(&from_account);

    let status_json = if let Some(sid) = status_id {
        let s = sqlx::query_as!(
            crate::db::models::Status,
            "SELECT * FROM statuses WHERE id = $1 AND deleted_at IS NULL",
            sid,
        )
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();

        if let Some(s) = s {
            let saccount = sqlx::query_as!(
                crate::db::models::Account,
                "SELECT * FROM accounts WHERE id = $1",
                s.account_id,
            )
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten()?;

            let media = fetch_status_media(state, s.id).await.unwrap_or_default();
            let reblog = fetch_reblog_data(state, &s).await.unwrap_or(None);
            build_status(state, &s, &saccount, media, reblog, None)
                .await
                .ok()
                .and_then(|st| serde_json::to_value(st).ok())
        } else {
            None
        }
    } else {
        None
    };

    let payload = serde_json::json!({
        "id": notification_id.to_string(),
        "type": notification_type,
        "created_at": created_at.to_rfc3339(),
        "group_key": format!("ungrouped-{}", notification_id),
        "account": serde_json::to_value(api_account).ok(),
        "status": status_json,
        "filtered": null,
    });

    serde_json::to_string(&payload).ok()
}
