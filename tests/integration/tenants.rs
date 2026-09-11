//! Several instances served by one process, dispatched by `Host`.
//!
//! Each `TestContext` brings up an instance of its own — database, seeded
//! accounts, random tokens — so two of them behind one listener are two tenants
//! that share nothing but the process, which is exactly what must not leak.

use crate::helpers::{ApiClient, TestContext};
use reqwest::StatusCode;
use serde_json::{json, Value};

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
