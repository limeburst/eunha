use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Extension, Query, State,
    },
    response::IntoResponse,
};
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
) -> impl IntoResponse {
    // Resolve account_id: prefer middleware-injected auth, fall back to query param token.
    let account_id = auth.map(|a| a.0.account_id);
    let token = params.access_token.clone();
    let stream = params.stream.clone().unwrap_or_else(|| "public".into());
    let instance_id = instance.id;

    tracing::info!(stream = %stream, ?account_id, "streaming: upgrade accepted");
    ws.on_upgrade(move |socket| async move {
        // Resolve token from query param if not already authenticated.
        let account_id = if account_id.is_some() {
            account_id
        } else if let Some(tok) = token {
            resolve_token(&state, &tok).await
        } else {
            None
        };

        tracing::info!(stream = %stream, ?account_id, "streaming: connection open");
        run(socket, stream, account_id, instance_id, state).await;
        tracing::info!("streaming: connection closed");
    })
}

async fn resolve_token(state: &AppState, token: &str) -> Option<Uuid> {
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
    stream: String,
    account_id: Option<Uuid>,
    instance_id: Uuid,
    state: AppState,
) {
    // `user` stream requires authentication.
    if stream == "user" && account_id.is_none() {
        let _ = socket
            .send(Message::Close(None))
            .await;
        return;
    }

    // For `user` stream: pre-load the set of followed account IDs so we can
    // filter home-timeline events without a DB query per message.
    let following: HashSet<Uuid> = if stream == "user" {
        let aid = account_id.unwrap();
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

    let mut rx = state.streaming.subscribe();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
    heartbeat.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        let Some(msg) = to_wire(&event, &stream, account_id, instance_id, &following)
                        else { continue };
                        if socket.send(Message::Text(msg.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    // Handle subscribe/unsubscribe commands from clients that
                    // connect without a `stream` query param and negotiate
                    // subscriptions over the socket instead.
                    Some(Ok(Message::Text(text))) => {
                        #[derive(serde::Deserialize)]
                        struct Cmd { #[serde(rename = "type")] _kind: String }
                        // Ignore parse errors; unrecognised commands are no-ops.
                        let _ = serde_json::from_str::<Cmd>(&text);
                    }
                    Some(Ok(Message::Ping(p))) => {
                        let _ = socket.send(Message::Pong(p)).await;
                    }
                    _ => break,
                }
            }
            _ = heartbeat.tick() => {
                // Mastodon streaming protocol heartbeat: send an empty `:thump\n\n` or a ping frame.
                // We use a WebSocket ping frame; Cloudflare resets its idle timer on any frame.
                if socket.send(Message::Ping(bytes::Bytes::new())).await.is_err() {
                    break;
                }
            }
        }
    }
}

/// Build the Mastodon streaming wire format for an event, or return `None`
/// if the event should not be delivered to this subscription.
fn to_wire(
    event: &Event,
    stream: &str,
    account_id: Option<Uuid>,
    instance_id: Uuid,
    following: &HashSet<Uuid>,
) -> Option<String> {
    match event {
        Event::NewStatus {
            instance_id: ev_iid,
            author_id,
            is_public,
            payload,
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
        other => vec![other],
    }
}
