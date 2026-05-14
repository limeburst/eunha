use reqwest::StatusCode;
use serde_json::Value;

use super::helpers::TestContext;

/// GET /api/v1/bookmarks returns statuses the user has bookmarked.
#[tokio::test]
async fn test_bookmarks_returns_bookmarked_statuses() {
    let ctx = TestContext::new("bmarks-list").await;

    let status = ctx
        .api
        .post_status(&ctx.alice_token, "bookmark listing test", "public")
        .await;
    let status_id = status["id"].as_str().unwrap().to_string();

    ctx.api
        .post_json(
            &format!("/api/v1/statuses/{status_id}/bookmark"),
            Some(&ctx.bob_token),
            &serde_json::json!({}),
        )
        .await;

    let resp = ctx.api.get("/api/v1/bookmarks", Some(&ctx.bob_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Vec<Value> = resp.json().await.unwrap();
    let ids: Vec<&str> = body.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(ids.contains(&status_id.as_str()), "bookmarked status missing from /api/v1/bookmarks");
}

/// The bookmarked field on the returned status objects is true.
#[tokio::test]
async fn test_bookmarks_status_has_bookmarked_true() {
    let ctx = TestContext::new("bmarks-field").await;

    let status = ctx
        .api
        .post_status(&ctx.alice_token, "bookmark field test", "public")
        .await;
    let status_id = status["id"].as_str().unwrap().to_string();

    ctx.api
        .post_json(
            &format!("/api/v1/statuses/{status_id}/bookmark"),
            Some(&ctx.bob_token),
            &serde_json::json!({}),
        )
        .await;

    let resp = ctx.api.get("/api/v1/bookmarks", Some(&ctx.bob_token)).await;
    let body: Vec<Value> = resp.json().await.unwrap();
    let entry = body.iter().find(|s| s["id"].as_str() == Some(&status_id)).unwrap();
    assert_eq!(entry["bookmarked"], true, "bookmarked field should be true");
}

/// GET /api/v1/bookmarks is empty when nothing has been bookmarked.
#[tokio::test]
async fn test_bookmarks_empty_when_none() {
    let ctx = TestContext::new("bmarks-empty").await;

    let resp = ctx.api.get("/api/v1/bookmarks", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Vec<Value> = resp.json().await.unwrap();
    assert!(body.is_empty());
}

/// GET /api/v1/bookmarks requires authentication.
#[tokio::test]
async fn test_bookmarks_requires_auth() {
    let ctx = TestContext::new("bmarks-unauth").await;

    let resp = ctx.api.get("/api/v1/bookmarks", None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Unbookmarking a status removes it from GET /api/v1/bookmarks.
#[tokio::test]
async fn test_bookmarks_unbookmark_removes_from_list() {
    let ctx = TestContext::new("bmarks-remove").await;

    let status = ctx
        .api
        .post_status(&ctx.alice_token, "unbookmark listing test", "public")
        .await;
    let status_id = status["id"].as_str().unwrap().to_string();

    ctx.api
        .post_json(
            &format!("/api/v1/statuses/{status_id}/bookmark"),
            Some(&ctx.bob_token),
            &serde_json::json!({}),
        )
        .await;

    ctx.api
        .post_json(
            &format!("/api/v1/statuses/{status_id}/unbookmark"),
            Some(&ctx.bob_token),
            &serde_json::json!({}),
        )
        .await;

    let resp = ctx.api.get("/api/v1/bookmarks", Some(&ctx.bob_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Vec<Value> = resp.json().await.unwrap();
    let ids: Vec<&str> = body.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(!ids.contains(&status_id.as_str()), "unbookmarked status still in bookmarks list");
}

/// The limit parameter caps the number of results returned.
#[tokio::test]
async fn test_bookmarks_limit_param() {
    let ctx = TestContext::new("bmarks-limit").await;

    for i in 0..3 {
        let s = ctx
            .api
            .post_status(&ctx.alice_token, &format!("bookmark limit {i}"), "public")
            .await;
        let sid = s["id"].as_str().unwrap().to_string();
        ctx.api
            .post_json(
                &format!("/api/v1/statuses/{sid}/bookmark"),
                Some(&ctx.bob_token),
                &serde_json::json!({}),
            )
            .await;
    }

    let resp = ctx
        .api
        .get("/api/v1/bookmarks?limit=2", Some(&ctx.bob_token))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(body.len(), 2, "limit=2 should return exactly 2 items");
}
