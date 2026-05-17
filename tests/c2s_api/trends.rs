use serde_json::Value;

use super::helpers::TestContext;

/// Trending statuses excludes statuses from accounts blocked by the viewer.
#[tokio::test]
async fn test_trending_statuses_excludes_blocked_accounts() {
    let ctx = TestContext::new("trends-block").await;

    // Bob posts a public status that would trend.
    let status = ctx.api.post_status(&ctx.bob_token, "trending block post #trendsblock", "public").await;
    let status_id = status["id"].as_str().unwrap();

    // Verify it appears before the block.
    let before: Vec<Value> = ctx.api.get("/api/v1/trends/statuses", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(
        before.iter().any(|s| s["id"].as_str() == Some(status_id)),
        "bob's status should appear in trending before block",
    );

    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/block", ctx.bob_id),
        Some(&ctx.alice_token),
        &serde_json::json!({}),
    ).await;

    let after: Vec<Value> = ctx.api.get("/api/v1/trends/statuses", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(
        !after.iter().any(|s| s["id"].as_str() == Some(status_id)),
        "blocked account's statuses should be hidden from trending statuses",
    );
}

/// Trending statuses excludes statuses from muted accounts (authenticated viewer).
#[tokio::test]
async fn test_trending_statuses_excludes_muted_accounts() {
    let ctx = TestContext::new("trends-mute").await;

    let status = ctx.api.post_status(&ctx.bob_token, "trending mute post #trendsmute", "public").await;
    let status_id = status["id"].as_str().unwrap();

    // Verify it appears before the mute.
    let before: Vec<Value> = ctx.api.get("/api/v1/trends/statuses", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(
        before.iter().any(|s| s["id"].as_str() == Some(status_id)),
        "bob's status should appear in trending before mute",
    );

    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/mute", ctx.bob_id),
        Some(&ctx.alice_token),
        &serde_json::json!({}),
    ).await;

    let after: Vec<Value> = ctx.api.get("/api/v1/trends/statuses", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(
        !after.iter().any(|s| s["id"].as_str() == Some(status_id)),
        "muted account's statuses should be hidden from trending statuses",
    );
}
