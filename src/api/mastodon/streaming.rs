use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Extension, Query, State,
    },
    http::HeaderMap,
    response::IntoResponse,
};
use bytes::Bytes;
use serde::Deserialize;
use std::collections::HashSet;
use std::time::Duration;
use uuid::Uuid;

use crate::{
    middleware::{AuthenticatedUser, ResolvedInstance},
    state::AppState,
    streaming::Event,
};

#[derive(Debug, Deserialize)]
pub struct StreamingParams {
    stream: Option<String>,
    /// Browsers can't set the Authorization header on WebSocket upgrades,
    /// so clients pass the token here instead.
    access_token: Option<String>,
}

pub async fn handler(
    ws: WebSocketUpgrade,
    Query(params): Query<StreamingParams>,
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    // Auth may already be resolved by the authenticate middleware (Bearer header).
    auth: Option<Extension<AuthenticatedUser>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let account_id: Option<i64> = auth.map(|a| a.0.account_id);

    // The masto library passes the access token as the WebSocket subprotocol rather
    // than as a query param. Browsers require the server to echo back the requested
    // subprotocol — if we don't, the browser aborts the connection immediately.
    let protocol_token = headers
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let token = params.access_token.clone().or_else(|| protocol_token.clone());
    let initial_stream = params.stream.clone();
    let instance_id = instance.id;

    let ws = if let Some(proto) = protocol_token.clone() {
        ws.protocols([proto])
    } else {
        ws
    };

    tracing::info!(?initial_stream, ?account_id, "streaming: upgrade accepted");
    ws.on_upgrade(move |socket| async move {
        let account_id = if account_id.is_some() {
            account_id
        } else if let Some(tok) = token {
            resolve_token(&state, &tok).await
        } else {
            None
        };

        tracing::info!(?initial_stream, ?account_id, "streaming: connection open");
        run(socket, initial_stream, account_id, instance_id, state).await;
        tracing::info!("streaming: connection closed");
    })
}

async fn resolve_token(state: &AppState, token: &str) -> Option<i64> {
    sqlx::query_scalar!(
        r#"SELECT account_id FROM oauth_access_tokens
           WHERE token = $1 AND revoked_at IS NULL
             AND (expires_at IS NULL OR expires_at > now())"#,
        token,
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .flatten()
}

async fn run(
    mut socket: WebSocket,
    initial_stream: Option<String>,
    account_id: Option<i64>,
    instance_id: Uuid,
    state: AppState,
) {
    // Load followed account IDs for any authenticated user so we can filter
    // home-timeline events without a DB query per message.
    let following: HashSet<i64> = if let Some(aid) = account_id {
        sqlx::query_scalar!(
            "SELECT target_account_id FROM follows
             WHERE account_id = $1 AND state = 'accepted'",
            aid,
        )
        .fetch_all(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect()
    } else {
        HashSet::new()
    };

    // Active stream subscriptions. Seeded by ?stream= query param; updated via
    // {"type":"subscribe"/"unsubscribe","stream":"..."} messages (multiplexed protocol).
    let mut subscribed: HashSet<String> = initial_stream.into_iter().collect();

    let mut rx = state.streaming.subscribe();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
    heartbeat.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        for stream in &subscribed {
                            let msg = if stream == "user" {
                                to_wire_user(&event, account_id, instance_id, &following, &state.db).await
                            } else if stream.starts_with("list:") || stream == "direct" {
                                to_wire_authenticated(&event, stream, account_id, instance_id, &state.db).await
                            } else {
                                to_wire(&event, stream, account_id, instance_id, &following)
                            };
                            if let Some(msg) = msg {
                                if socket.send(Message::Text(msg.into())).await.is_err() {
                                    return;
                                }
                                // No break: send a separate message per matching stream,
                                // matching Mastodon's behaviour.
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        tracing::info!(stream = ?subscribed, text = %text, "streaming: client message");
                        #[derive(Deserialize)]
                        struct Cmd {
                            #[serde(rename = "type")]
                            kind: String,
                            stream: Option<String>,
                        }
                        if let Ok(cmd) = serde_json::from_str::<Cmd>(&text) {
                            match cmd.kind.as_str() {
                                "subscribe" => {
                                    if let Some(s) = cmd.stream {
                                        // user stream requires authentication
                                        if s == "user" && account_id.is_none() {
                                            tracing::warn!("streaming: unauthenticated user stream subscribe ignored");
                                        } else {
                                            tracing::info!(stream = %s, "streaming: subscribed");
                                            subscribed.insert(s);
                                        }
                                    }
                                }
                                "unsubscribe" => {
                                    if let Some(s) = cmd.stream {
                                        tracing::info!(stream = %s, "streaming: unsubscribed");
                                        subscribed.remove(&s);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        let _ = socket.send(Message::Pong(p)).await;
                    }
                    Some(Ok(Message::Close(frame))) => {
                        tracing::info!(?frame, "streaming: client close frame");
                        break;
                    }
                    Some(Ok(other)) => {
                        tracing::info!(?other, "streaming: unexpected message type");
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::warn!(error = %e, "streaming: socket error");
                        break;
                    }
                    None => {
                        tracing::info!("streaming: socket recv returned None");
                        break;
                    }
                }
            }
            _ = heartbeat.tick() => {
                // Ping frame resets Cloudflare's idle connection timer.
                if socket.send(Message::Ping(Bytes::new())).await.is_err() {
                    break;
                }
            }
        }
    }
}

/// Like `to_wire` but for the `user` stream: injects per-viewer context fields
/// (favourited, reblogged, bookmarked, etc.) via a DB lookup.
async fn to_wire_user(
    event: &Event,
    account_id: Option<i64>,
    instance_id: Uuid,
    following: &HashSet<i64>,
    db: &sqlx::PgPool,
) -> Option<String> {
    match event {
        Event::NewStatus {
            instance_id: ev_iid,
            author_id,
            status_id,
            payload,
            ..
        } => {
            let deliver = *ev_iid == instance_id
                && account_id
                    .map(|aid| aid == *author_id || following.contains(author_id))
                    .unwrap_or(false);
            if !deliver {
                return None;
            }
            let aid = account_id?;

            let favourited = sqlx::query_scalar!(
                "SELECT 1 AS e FROM favourites WHERE account_id = $1 AND status_id = $2",
                aid, status_id
            )
            .fetch_optional(db).await.ok().flatten().is_some();

            let reblogged = sqlx::query_scalar!(
                "SELECT 1 AS e FROM statuses WHERE account_id = $1 AND reblog_of_id = $2 AND deleted_at IS NULL",
                aid, status_id
            )
            .fetch_optional(db).await.ok().flatten().is_some();

            let bookmarked = sqlx::query_scalar!(
                "SELECT 1 AS e FROM bookmarks WHERE account_id = $1 AND status_id = $2",
                aid, status_id
            )
            .fetch_optional(db).await.ok().flatten().is_some();

            // Inject viewer context fields into the existing payload JSON.
            let mut value: serde_json::Value = serde_json::from_str(payload).ok()?;
            if let serde_json::Value::Object(ref mut obj) = value {
                obj.insert("favourited".into(), serde_json::json!(favourited));
                obj.insert("reblogged".into(), serde_json::json!(reblogged));
                obj.insert("muted".into(), serde_json::json!(false));
                obj.insert("bookmarked".into(), serde_json::json!(bookmarked));
                obj.insert("pinned".into(), serde_json::json!(false));
                obj.insert("filtered".into(), serde_json::json!([]));
            }
            let enriched = serde_json::to_string(&value).ok()?;
            Some(wire("update", &["user"], &enriched))
        }
        // Notifications and deletes fall through to the standard path.
        other => to_wire(other, "user", account_id, instance_id, following),
    }
}

/// Build the Mastodon streaming wire format for an event, or return `None`
/// if the event should not be delivered to this subscription.
fn to_wire(
    event: &Event,
    stream: &str,
    account_id: Option<i64>,
    instance_id: Uuid,
    following: &HashSet<i64>,
) -> Option<String> {
    match event {
        Event::NewStatus {
            instance_id: ev_iid,
            author_id,
            is_public,
            hashtags,
            payload,
            ..
        } => {
            let deliver = match stream {
                "public" => *is_public,
                "public:local" => *is_public && *ev_iid == instance_id,
                "user" => {
                    *ev_iid == instance_id
                        && account_id
                            .map(|aid| aid == *author_id || following.contains(author_id))
                            .unwrap_or(false)
                }
                s if s.starts_with("hashtag:local:") => {
                    let tag = &s["hashtag:local:".len()..];
                    *is_public && *ev_iid == instance_id
                        && hashtags.iter().any(|h| h.eq_ignore_ascii_case(tag))
                }
                s if s.starts_with("hashtag:") && !s.starts_with("hashtag:local") => {
                    let tag = &s["hashtag:".len()..];
                    *is_public && hashtags.iter().any(|h| h.eq_ignore_ascii_case(tag))
                }
                _ => false,
            };
            if !deliver {
                return None;
            }
            let stream_arr = stream_label(stream);
            Some(wire("update", &stream_arr, payload))
        }

        Event::Notification {
            for_account_id,
            payload,
        } => {
            if stream != "user" {
                return None;
            }
            if account_id != Some(*for_account_id) {
                return None;
            }
            Some(wire("notification", &["user"], payload))
        }

        Event::DeleteStatus {
            instance_id: ev_iid,
            status_id,
        } => {
            if *ev_iid != instance_id {
                return None;
            }
            let stream_arr = stream_label(stream);
            // delete payload is just the ID string, not double-encoded JSON.
            Some(
                serde_json::json!({
                    "stream": stream_arr,
                    "event": "delete",
                    "payload": status_id.to_string(),
                })
                .to_string(),
            )
        }
    }
}

/// Handle `list:N` and `direct` streams which require DB lookups.
async fn to_wire_authenticated(
    event: &Event,
    stream: &str,
    account_id: Option<i64>,
    instance_id: Uuid,
    db: &sqlx::PgPool,
) -> Option<String> {
    let aid = account_id?;
    match event {
        Event::NewStatus {
            instance_id: ev_iid,
            author_id,
            is_direct,
            status_id,
            payload,
            ..
        } => {
            if *ev_iid != instance_id {
                return None;
            }
            let deliver = if stream == "direct" {
                *is_direct
                    && sqlx::query_scalar!(
                        "SELECT 1 AS e FROM statuses WHERE id = $1 AND account_id = $2 AND deleted_at IS NULL",
                        status_id, aid
                    )
                    .fetch_optional(db).await.ok().flatten().is_some()
                    || (*is_direct && *author_id == aid)
            } else if let Some(list_id_str) = stream.strip_prefix("list:") {
                if let Ok(list_id) = list_id_str.parse::<i64>() {
                    // Deliver if the author is in this list owned by the viewer
                    sqlx::query_scalar!(
                        r#"SELECT 1 AS e FROM list_accounts la
                           JOIN lists l ON l.id = la.list_id
                           WHERE la.list_id = $1 AND la.account_id = $2
                             AND l.account_id = $3"#,
                        list_id, *author_id, aid,
                    )
                    .fetch_optional(db).await.ok().flatten().is_some()
                } else {
                    false
                }
            } else {
                false
            };
            if !deliver {
                return None;
            }
            let stream_arr = stream_label(stream);
            Some(wire("update", &stream_arr, payload))
        }
        Event::DeleteStatus { instance_id: ev_iid, status_id } => {
            if *ev_iid != instance_id {
                return None;
            }
            let stream_arr = stream_label(stream);
            Some(serde_json::json!({
                "stream": stream_arr,
                "event": "delete",
                "payload": status_id.to_string(),
            }).to_string())
        }
        Event::Notification { .. } => None,
    }
}

/// Encode an event as a Mastodon streaming JSON message.
/// `payload` is already a serialised JSON string; `serde_json::json!` will
/// double-encode it as required by the protocol.
fn wire(event: &str, streams: &[&str], payload: &str) -> String {
    serde_json::json!({
        "stream": streams,
        "event": event,
        "payload": payload,
    })
    .to_string()
}

fn stream_label(stream: &str) -> Vec<&str> {
    match stream {
        "public:local" => vec!["public", "public:local"],
        s if s.starts_with("hashtag:local:") => {
            vec!["hashtag", "hashtag:local"]
        }
        s if s.starts_with("hashtag:") => vec!["hashtag"],
        s if s.starts_with("list:") => vec!["list"],
        other => vec![other],
    }
}
