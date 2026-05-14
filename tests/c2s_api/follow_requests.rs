use reqwest::StatusCode;
use serde_json::Value;

use super::helpers::TestContext;

/// When alice locks her account, bob's follow creates a pending request.
/// GET /api/v1/follow_requests returns that request.
#[tokio::test]
async fn test_follow_requests_lists_pending_followers() {
    let ctx = TestContext::new("freq-list").await;

    // Lock alice's account so follows require approval.
    ctx.api
        .patch_multipart(
            "/api/v1/accounts/update_credentials",
            &ctx.alice_token,
            &[("locked", "true")],
        )
        .await;

    // Bob follows alice — should become pending.
    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;

    let resp = ctx
        .api
        .get("/api/v1/follow_requests", Some(&ctx.alice_token))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Vec<Value> = resp.json().await.unwrap();
    let ids: Vec<&str> = body.iter().filter_map(|a| a["id"].as_str()).collect();
    assert!(ids.contains(&ctx.bob_id.as_str()), "bob not in alice's follow requests");
}

/// GET /api/v1/follow_requests returns empty when there are no pending requests.
#[tokio::test]
async fn test_follow_requests_empty_when_none() {
    let ctx = TestContext::new("freq-empty").await;

    let resp = ctx
        .api
        .get("/api/v1/follow_requests", Some(&ctx.alice_token))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Vec<Value> = resp.json().await.unwrap();
    assert!(body.is_empty());
}

/// GET /api/v1/follow_requests requires authentication.
#[tokio::test]
async fn test_follow_requests_requires_auth() {
    let ctx = TestContext::new("freq-unauth").await;

    let resp = ctx.api.get("/api/v1/follow_requests", None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Authorizing a follow request sets followed_by to true in the relationship.
#[tokio::test]
async fn test_follow_requests_authorize() {
    let ctx = TestContext::new("freq-auth").await;

    ctx.api
        .patch_multipart(
            "/api/v1/accounts/update_credentials",
            &ctx.alice_token,
            &[("locked", "true")],
        )
        .await;

    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;

    let resp = ctx
        .api
        .post_json(
            &format!("/api/v1/follow_requests/{}/authorize", ctx.bob_id),
            Some(&ctx.alice_token),
            &serde_json::json!({}),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let rel: Value = resp.json().await.unwrap();
    assert_eq!(rel["followed_by"], true, "followed_by should be true after authorization");

    // The request should no longer appear in the list.
    let list_resp = ctx
        .api
        .get("/api/v1/follow_requests", Some(&ctx.alice_token))
        .await;
    let list: Vec<Value> = list_resp.json().await.unwrap();
    assert!(list.is_empty(), "follow request still listed after authorization");
}

/// Rejecting a follow request sets followed_by to false and removes the request.
#[tokio::test]
async fn test_follow_requests_reject() {
    let ctx = TestContext::new("freq-rej").await;

    ctx.api
        .patch_multipart(
            "/api/v1/accounts/update_credentials",
            &ctx.alice_token,
            &[("locked", "true")],
        )
        .await;

    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;

    let resp = ctx
        .api
        .post_json(
            &format!("/api/v1/follow_requests/{}/reject", ctx.bob_id),
            Some(&ctx.alice_token),
            &serde_json::json!({}),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let rel: Value = resp.json().await.unwrap();
    assert_eq!(rel["followed_by"], false, "followed_by should be false after rejection");

    // The request should no longer appear in the list.
    let list_resp = ctx
        .api
        .get("/api/v1/follow_requests", Some(&ctx.alice_token))
        .await;
    let list: Vec<Value> = list_resp.json().await.unwrap();
    assert!(list.is_empty(), "follow request still listed after rejection");
}
