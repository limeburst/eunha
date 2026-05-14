use reqwest::StatusCode;
use serde_json::Value;

use super::helpers::TestContext;

/// POST /api/v1/apps with valid params returns client_id and client_secret.
#[tokio::test]
async fn test_register_app_returns_credentials() {
    let ctx = TestContext::new("apps-reg").await;

    let resp = ctx
        .api
        .post_json(
            "/api/v1/apps",
            None,
            &serde_json::json!({
                "client_name": "Test App",
                "redirect_uris": "urn:ietf:wg:oauth:2.0:oob",
                "scopes": "read write"
            }),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert!(body["client_id"].as_str().is_some(), "missing client_id");
    assert!(body["client_secret"].as_str().is_some(), "missing client_secret");
    assert_eq!(body["name"].as_str(), Some("Test App"));
}

/// Registered app response includes the redirect_uri and redirect_uris fields.
#[tokio::test]
async fn test_register_app_response_shape() {
    let ctx = TestContext::new("apps-shape").await;

    let resp = ctx
        .api
        .post_json(
            "/api/v1/apps",
            None,
            &serde_json::json!({
                "client_name": "Shape Test",
                "redirect_uris": "https://app.example/callback",
                "scopes": "read"
            }),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["redirect_uri"].as_str(), Some("https://app.example/callback"));
    assert_eq!(body["redirect_uris"][0].as_str(), Some("https://app.example/callback"));
    assert!(
        body["scopes"].as_array().is_some_and(|a| a.iter().any(|s| s.as_str() == Some("read"))),
        "scopes array should contain 'read'"
    );
}

/// Omitting scopes defaults to read.
#[tokio::test]
async fn test_register_app_default_scope() {
    let ctx = TestContext::new("apps-dflt").await;

    let resp = ctx
        .api
        .post_json(
            "/api/v1/apps",
            None,
            &serde_json::json!({
                "client_name": "Default Scope App",
                "redirect_uris": "urn:ietf:wg:oauth:2.0:oob"
            }),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(
        body["scopes"].as_array().and_then(|a| a.first()).and_then(|s| s.as_str()),
        Some("read"),
        "default scope should be read"
    );
}

/// Omitting client_name returns 422.
#[tokio::test]
async fn test_register_app_missing_client_name_unprocessable() {
    let ctx = TestContext::new("apps-noname").await;

    let resp = ctx
        .api
        .post_json(
            "/api/v1/apps",
            None,
            &serde_json::json!({
                "redirect_uris": "urn:ietf:wg:oauth:2.0:oob",
                "scopes": "read"
            }),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// Multiple apps can be registered independently (distinct client_ids).
#[tokio::test]
async fn test_register_multiple_apps() {
    let ctx = TestContext::new("apps-multi").await;

    let base_payload = |name: &str| {
        serde_json::json!({
            "client_name": name,
            "redirect_uris": "urn:ietf:wg:oauth:2.0:oob",
            "scopes": "read"
        })
    };

    let r1: Value = ctx
        .api
        .post_json("/api/v1/apps", None, &base_payload("App One"))
        .await
        .json()
        .await
        .unwrap();
    let r2: Value = ctx
        .api
        .post_json("/api/v1/apps", None, &base_payload("App Two"))
        .await
        .json()
        .await
        .unwrap();

    assert_ne!(
        r1["client_id"].as_str(),
        r2["client_id"].as_str(),
        "two apps should get distinct client_ids"
    );
}

/// POST /oauth/revoke invalidates a token so it can no longer be used.
#[tokio::test]
async fn test_revoke_token() {
    let ctx = TestContext::new("apps-revoke").await;

    // Verify that the token is currently valid.
    let before = ctx.api.get("/api/v1/accounts/verify_credentials", Some(&ctx.alice_token)).await;
    assert_eq!(before.status(), StatusCode::OK, "token should be valid before revocation");

    // Revoke it.
    let revoke_resp = ctx.api.post_json(
        "/oauth/revoke",
        None,
        &serde_json::json!({"token": ctx.alice_token}),
    ).await;
    assert_eq!(revoke_resp.status(), StatusCode::OK);

    // After revocation the token should no longer work.
    let after = ctx.api.get("/api/v1/accounts/verify_credentials", Some(&ctx.alice_token)).await;
    assert_eq!(after.status(), StatusCode::UNAUTHORIZED, "revoked token should return 401");
}
