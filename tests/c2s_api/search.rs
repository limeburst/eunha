use reqwest::StatusCode;
use serde_json::Value;

use super::helpers::TestContext;

/// Search by username finds the matching account.
#[tokio::test]
async fn test_search_accounts() {
    let ctx = TestContext::new("search-acct").await;

    let resp = ctx.api.get("/api/v2/search?q=alice&type=accounts", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    let accounts = body["accounts"].as_array().unwrap();
    assert!(accounts.iter().any(|a| a["username"].as_str() == Some("alice")));
}

/// Search for a status by its text returns the matching status.
#[tokio::test]
async fn test_search_statuses() {
    let ctx = TestContext::new("search-status").await;

    ctx.api.post_status(&ctx.alice_token, "uniqueterm12345", "public").await;

    let resp = ctx.api.get(
        "/api/v2/search?q=uniqueterm12345&type=statuses",
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    let statuses = body["statuses"].as_array().unwrap();
    assert!(
        statuses.iter().any(|s| s["content"].as_str().unwrap_or("").contains("uniqueterm12345")
            || s["text"].as_str().unwrap_or("").contains("uniqueterm12345")),
        "search did not find status with uniqueterm12345"
    );
}

/// GET /api/v2/search?type=hashtags finds tags created by posting.
#[tokio::test]
async fn test_search_hashtags() {
    let ctx = TestContext::new("search-hash").await;

    ctx.api.post_status(&ctx.alice_token, "I enjoy #searchhash999", "public").await;

    let body: Value = ctx.api.get(
        "/api/v2/search?q=searchhash999&type=hashtags",
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    let hashtags = body["hashtags"].as_array().unwrap();
    assert!(
        hashtags.iter().any(|t| t["name"].as_str() == Some("searchhash999")),
        "hashtag not found in search results",
    );
}
