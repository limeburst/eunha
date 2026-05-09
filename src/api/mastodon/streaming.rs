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
    let account_id = auth.map(|a| a.0.account_id);

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
    initial_stream: Option<String>,
    account_id: Option<Uuid>,
    instance_id: Uuid,
    state: AppState,
) {
    // Load followed account IDs for any authenticated user so we can filter
    // home-timeline events without a DB query per message.
    let following: HashSet<Uuid> = if let Some(aid) = account_id {
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
                            if let Some(msg) = to_wire(&event, stream, account_id, instance_id, &following) {
                                if socket.send(Message::Text(msg.into())).await.is_err() {
                                    return;
                                }
                                break; // avoid duplicate delivery if multiple streams match
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
