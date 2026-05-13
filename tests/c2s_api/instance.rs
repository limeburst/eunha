use reqwest::StatusCode;
use serde_json::{json, Value};

use super::helpers::TestContext;

/// GET /api/v1/instance returns valid instance data.
#[tokio::test]
async fn test_instance_v1() {
    let ctx = TestContext::new("instance-v1").await;

    let resp = ctx.api.get("/api/v1/instance", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    assert!(body["uri"].as_str().is_some(), "uri field missing");
    assert!(body["title"].as_str().is_some(), "title field missing");
    assert!(body["version"].as_str().is_some(), "version field missing");
}

/// GET /api/v2/instance returns valid instance data including usage.
#[tokio::test]
async fn test_instance_v2() {
    let ctx = TestContext::new("instance-v2").await;

    let resp = ctx.api.get("/api/v2/instance", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    assert!(body["domain"].as_str().is_some(), "domain field missing");
    assert!(body["version"].as_str().is_some(), "version field missing");
}

/// GET /api/v1/instance/extended_description returns 200.
#[tokio::test]
async fn test_instance_extended_description() {
    let ctx = TestContext::new("inst-ext").await;

    let resp = ctx.api.get("/api/v1/instance/extended_description", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

/// GET /api/v1/instance/peers returns an array.
#[tokio::test]
async fn test_instance_peers() {
    let ctx = TestContext::new("inst-peers").await;

    let resp = ctx.api.get("/api/v1/instance/peers", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let _: Vec<String> = resp.json().await.unwrap();
}

/// GET /api/v1/instance/privacy_policy returns 200.
#[tokio::test]
async fn test_instance_privacy_policy() {
    let ctx = TestContext::new("inst-priv").await;

    let resp = ctx.api.get("/api/v1/instance/privacy_policy", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

/// GET /api/v1/instance/translation_languages returns 200.
#[tokio::test]
async fn test_instance_translation_languages() {
    let ctx = TestContext::new("inst-trans").await;

    let resp = ctx.api.get("/api/v1/instance/translation_languages", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

/// POST /api/v1/apps registers an application and returns credentials.
#[tokio::test]
async fn test_register_app() {
    let ctx = TestContext::new("oauth-app").await;

    let resp = ctx.api.post_json(
        "/api/v1/apps",
        None,
        &json!({
            "client_name": "Test App",
            "redirect_uris": "urn:ietf:wg:oauth:2.0:oob",
            "scopes": "read write"
        }),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    assert!(body["client_id"].as_str().is_some(), "client_id missing");
    assert!(body["client_secret"].as_str().is_some(), "client_secret missing");
    assert_eq!(body["name"].as_str(), Some("Test App"));
}

/// GET /api/v1/announcements returns an array (empty when none published).
#[tokio::test]
async fn test_get_announcements() {
    let ctx = TestContext::new("announce").await;

    let resp = ctx.api.get("/api/v1/announcements", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let _: Vec<Value> = resp.json().await.unwrap();
}

/// GET /api/v1/custom_emojis returns a JSON array.
#[tokio::test]
async fn test_get_custom_emojis() {
    let ctx = TestContext::new("emojis").await;

    let resp = ctx.api.get("/api/v1/custom_emojis", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let _: Vec<Value> = resp.json().await.unwrap();
}

/// GET /api/v1/trends/tags returns a JSON array.
#[tokio::test]
async fn test_trending_tags() {
    let ctx = TestContext::new("trends-tags").await;

    let resp = ctx.api.get("/api/v1/trends/tags", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let _: Vec<Value> = resp.json().await.unwrap();
}

/// GET /api/v1/trends/statuses returns a JSON array.
#[tokio::test]
async fn test_trending_statuses() {
    let ctx = TestContext::new("trends-stat").await;

    let resp = ctx.api.get("/api/v1/trends/statuses", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let _: Vec<Value> = resp.json().await.unwrap();
}

/// GET /api/v1/trends/links returns a JSON array.
#[tokio::test]
async fn test_trending_links() {
    let ctx = TestContext::new("trends-links").await;

    let resp = ctx.api.get("/api/v1/trends/links", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let _: Vec<Value> = resp.json().await.unwrap();
}
