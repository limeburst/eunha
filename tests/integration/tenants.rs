//! Several instances served by one process, dispatched by `Host`.
//!
//! Each `TestContext` brings up an instance of its own — database, seeded
//! accounts, random tokens — so two of them behind one listener are two tenants
//! that share nothing but the process, which is exactly what must not leak.

use crate::helpers::{ApiClient, TestContext};
use futures::StreamExt as _;
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest as _, Message};

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Serve these instances' states from one listener, as one process would.
async fn serve(states: Vec<eunha::state::AppState>) -> String {
    let tenants = eunha::tenants::Tenants::from_states("127.0.0.1:0", states)
        .expect("tenants with distinct domains form a registry");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, tenants.into_router()).await.unwrap();
    });
    base_url
}

/// Two instances in one process each answer only for their own host: their
/// own instance metadata, their own statuses, their own tokens.
#[tokio::test]
async fn test_tenants_in_one_process_are_dispatched_by_host() {
    let a = TestContext::new("tenant-a").await;
    let b = TestContext::new("tenant-b").await;
    let base_url = serve(vec![a.state.clone(), b.state.clone()]).await;
    let to_a = ApiClient::new(&base_url, &a.domain);
    let to_b = ApiClient::new(&base_url, &b.domain);

    for (client, domain) in [(&to_a, &a.domain), (&to_b, &b.domain)] {
        let instance: Value = client
            .get("/api/v2/instance", None)
            .await
            .json()
            .await
            .unwrap();
        assert_eq!(
            instance["domain"].as_str(),
            Some(domain.as_str()),
            "each host answers as its own instance",
        );
    }

    let status: Value = to_a
        .post_json(
            "/api/v1/statuses",
            Some(&a.alice_token),
            &json!({ "status": "only on A", "visibility": "public" }),
        )
        .await
        .json()
        .await
        .unwrap();
    let id = status["id"].as_str().expect("the status was created");
    assert!(
        status["uri"]
            .as_str()
            .is_some_and(|uri| uri.starts_with(&format!("https://{}/", a.domain))),
        "A's status is A's: {}",
        status["uri"],
    );
    assert_eq!(
        to_a.get(&format!("/api/v1/statuses/{id}"), None)
            .await
            .status(),
        StatusCode::OK,
        "A serves its own status",
    );
    assert_eq!(
        to_b.get(&format!("/api/v1/statuses/{id}"), None)
            .await
            .status(),
        StatusCode::NOT_FOUND,
        "B has never heard of A's status",
    );

    assert_eq!(
        to_a.get("/api/v1/accounts/verify_credentials", Some(&a.alice_token))
            .await
            .status(),
        StatusCode::OK,
        "A's token works on A",
    );
    assert_eq!(
        to_b.get("/api/v1/accounts/verify_credentials", Some(&a.alice_token))
            .await
            .status(),
        StatusCode::UNAUTHORIZED,
        "A's token means nothing to B",
    );

    let stranger = ApiClient::new(&base_url, "stranger.invalid");
    assert_eq!(
        stranger.get("/api/v2/instance", None).await.status(),
        StatusCode::MISDIRECTED_REQUEST,
        "a host no tenant serves is refused, not handed to one of them",
    );
}

/// A lone instance answers whatever host it is asked by, as eunha always has:
/// health checks and local clients do not send the instance's domain.
#[tokio::test]
async fn test_a_lone_tenant_answers_any_host() {
    let a = TestContext::new("tenant-lone").await;
    let base_url = serve(vec![a.state.clone()]).await;
    let other = ApiClient::new(&base_url, "localhost");
    assert_eq!(
        other.get("/api/v2/instance", None).await.status(),
        StatusCode::OK
    );
}

/// Open a streaming connection to whichever tenant `host` names, through the
/// shared listener.
async fn open_stream(base_url: &str, host: &str, stream: &str) -> Ws {
    let url = format!(
        "{}/api/v1/streaming?stream={stream}",
        base_url.replace("http://", "ws://")
    );
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert("host", host.parse().unwrap());
    let (ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("a WebSocket upgrade survives dispatch");
    ws
}

/// The next event on `ws`, or `None` if nothing arrives within `wait`.
async fn next_event(ws: &mut Ws, wait: Duration) -> Option<Value> {
    loop {
        match tokio::time::timeout(wait, ws.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => return serde_json::from_str(&text).ok(),
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => continue,
            _ => return None,
        }
    }
}

/// Each tenant's streaming events reach only its own streams. The streaming bus
/// lives in the process, which makes it the first place a shared process could
/// leak — and the connection has to survive being upgraded through the
/// dispatcher at all.
#[tokio::test]
async fn test_streams_do_not_cross_tenants() {
    let a = TestContext::new("tenant-stream-a").await;
    let b = TestContext::new("tenant-stream-b").await;
    let base_url = serve(vec![a.state.clone(), b.state.clone()]).await;
    let mut on_a = open_stream(&base_url, &a.domain, "public").await;
    let mut on_b = open_stream(&base_url, &b.domain, "public").await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    ApiClient::new(&base_url, &a.domain)
        .post_status(&a.alice_token, "streamed on A", "public")
        .await;

    let event = next_event(&mut on_a, Duration::from_secs(3))
        .await
        .expect("A's stream hears A's status");
    assert_eq!(event["event"], "update");
    assert!(
        next_event(&mut on_b, Duration::from_secs(1))
            .await
            .is_none(),
        "B's stream must hear nothing of A's status",
    );
}

/// Seed a remote actor, with its public key, into one tenant's database.
async fn seed_remote_actor(db: &sqlx::PgPool, uri: &str, public_key: &str) {
    sqlx::query(
        r#"INSERT INTO accounts
             (id, username, domain, display_name, note, url, uri, public_key,
              inbox_url, outbox_url, created_at, updated_at)
           VALUES ($1, 'carol', 'remote-tenant.invalid', 'carol', '', $2, $2, $3,
                   $2 || '/inbox', $2 || '/outbox', now(), now())"#,
    )
    .bind(eunha::snowflake::next_id())
    .bind(uri)
    .bind(public_key)
    .execute(db)
    .await
    .unwrap();
}

async fn followers_of_alice(ctx: &TestContext) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM follows WHERE target_account_id = $1")
        .bind(ctx.alice_id.parse::<i64>().unwrap())
        .fetch_one(&ctx.db)
        .await
        .unwrap()
}

/// An activity delivered to one tenant's host lands in that tenant's database
/// and nowhere else — including one that names another tenant's account.
#[tokio::test]
async fn test_inbox_deliveries_stay_with_the_tenant_addressed() {
    let a = TestContext::new("tenant-inbox-a").await;
    let b = TestContext::new("tenant-inbox-b").await;
    let base_url = serve(vec![a.state.clone(), b.state.clone()]).await;
    let (private_key, public_key) = eunha::crypto::generate_rsa_keypair().unwrap();
    let carol = "https://remote-tenant.invalid/users/carol";
    seed_remote_actor(&a.db, carol, &public_key).await;
    seed_remote_actor(&b.db, carol, &public_key).await;
    let to_b = ApiClient::new(&base_url, &b.domain);
    let key_id = format!("{carol}#main-key");

    let follow = |id: &str, domain: &str| {
        json!({
            "@context": "https://www.w3.org/ns/activitystreams",
            "id": format!("https://remote-tenant.invalid/activities/{id}"),
            "type": "Follow",
            "actor": carol,
            "object": format!("https://{domain}/users/alice"),
        })
    };

    let response = to_b
        .post_signed(
            "/inbox",
            &follow("follow-b", &b.domain),
            &key_id,
            &private_key,
        )
        .await;
    assert!(
        response.status().is_success(),
        "B accepts a follow of its own alice: {}",
        response.status()
    );
    assert_eq!(
        followers_of_alice(&b).await,
        1,
        "B's alice gained a follower"
    );
    assert_eq!(followers_of_alice(&a).await, 0, "A's alice did not");

    // Names A's alice, but is delivered to B.
    to_b.post_signed(
        "/inbox",
        &follow("follow-a", &a.domain),
        &key_id,
        &private_key,
    )
    .await;
    assert_eq!(
        followers_of_alice(&a).await,
        0,
        "B cannot act on A's accounts"
    );
    assert_eq!(
        followers_of_alice(&b).await,
        1,
        "nor may B take A's alice for its own"
    );
}

/// Discovery answers only for the tenant asked: WebFinger in both its `acct:`
/// and URL forms, and the actor document.
#[tokio::test]
async fn test_discovery_answers_only_for_the_tenant_asked() {
    let a = TestContext::new("tenant-webfinger-a").await;
    let b = TestContext::new("tenant-webfinger-b").await;
    let base_url = serve(vec![a.state.clone(), b.state.clone()]).await;
    let to_a = ApiClient::new(&base_url, &a.domain);
    let to_b = ApiClient::new(&base_url, &b.domain);

    for resource in [
        format!("acct:alice@{}", a.domain),
        format!("https://{}/users/alice", a.domain),
    ] {
        let path = format!(
            "/.well-known/webfinger?resource={}",
            urlencoding::encode(&resource)
        );
        let found: Value = to_a.get(&path, None).await.json().await.unwrap();
        assert_eq!(
            found["subject"].as_str(),
            Some(format!("acct:alice@{}", a.domain).as_str()),
            "A answers for {resource}",
        );
        assert_eq!(
            to_b.get(&path, None).await.status(),
            StatusCode::NOT_FOUND,
            "B must not answer for {resource}",
        );
    }

    for (client, domain) in [(&to_a, &a.domain), (&to_b, &b.domain)] {
        let actor: Value = client.get("/users/alice", None).await.json().await.unwrap();
        assert_eq!(
            actor["id"].as_str(),
            Some(format!("https://{domain}/users/alice").as_str()),
            "each tenant serves its own alice",
        );
    }
}

/// Tenants in one process share none of the in-process state that requests
/// and background tasks read: their queue wake-ups and their URLs are their own.
#[tokio::test]
async fn test_tenants_share_no_in_process_state() {
    let a = TestContext::new("tenant-state-a").await;
    let b = TestContext::new("tenant-state-b").await;
    assert!(
        !Arc::ptr_eq(&a.state.queues, &b.state.queues),
        "queue wake-ups are per tenant"
    );
    assert!(
        !Arc::ptr_eq(&a.state.urls, &b.state.urls),
        "URLs are per tenant"
    );
    assert_eq!(a.state.urls.local_domain, a.domain);
    assert_eq!(b.state.urls.local_domain, b.domain);
}
