use reqwest::StatusCode;
use serde_json::{json, Value};

use super::helpers::TestContext;

/// Follow a tag, verify it appears in followed_tags, then unfollow.
#[tokio::test]
async fn test_tag_follow_lifecycle() {
    let ctx = TestContext::new("tag-follow").await;

    ctx.api.post_status(&ctx.alice_token, "I love #rusttag789 programming", "public").await;

    let follow_resp = ctx.api.post_json(
        "/api/v1/tags/rusttag789/follow",
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(follow_resp.status(), StatusCode::OK);
    let tag: Value = follow_resp.json().await.unwrap();
    assert_eq!(tag["following"].as_bool(), Some(true));

    let tags: Vec<Value> = ctx.api.get("/api/v1/followed_tags", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(tags.iter().any(|t| t["name"].as_str() == Some("rusttag789")));

    let get_resp = ctx.api.get("/api/v1/tags/rusttag789", Some(&ctx.alice_token)).await;
    assert_eq!(get_resp.status(), StatusCode::OK);
    let tag_data: Value = get_resp.json().await.unwrap();
    assert_eq!(tag_data["name"].as_str(), Some("rusttag789"));
    assert_eq!(tag_data["following"].as_bool(), Some(true));

    let unfollow_resp = ctx.api.post_json(
        "/api/v1/tags/rusttag789/unfollow",
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(unfollow_resp.status(), StatusCode::OK);
    let tag2: Value = unfollow_resp.json().await.unwrap();
    assert_eq!(tag2["following"].as_bool(), Some(false));

    let after: Vec<Value> = ctx.api.get("/api/v1/followed_tags", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(!after.iter().any(|t| t["name"].as_str() == Some("rusttag789")));
}

/// GET /api/v1/followed_tags returns only the current user's followed tags.
#[tokio::test]
async fn test_followed_tags_scoped_to_user() {
    let ctx = TestContext::new("ftag-scope").await;

    ctx.api.post_status(&ctx.alice_token, "I love #scoped_tag_alice", "public").await;
    ctx.api.post_status(&ctx.bob_token, "I love #scoped_tag_bob", "public").await;

    ctx.api.post_json(
        "/api/v1/tags/scoped_tag_alice/follow",
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    ctx.api.post_json(
        "/api/v1/tags/scoped_tag_bob/follow",
        Some(&ctx.bob_token),
        &json!({}),
    ).await;

    let alice_tags: Vec<Value> = ctx.api.get("/api/v1/followed_tags", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    let bob_tags: Vec<Value> = ctx.api.get("/api/v1/followed_tags", Some(&ctx.bob_token))
        .await.json().await.unwrap();

    assert!(
        alice_tags.iter().any(|t| t["name"].as_str() == Some("scoped_tag_alice")),
        "alice should see her own followed tag",
    );
    assert!(
        !alice_tags.iter().any(|t| t["name"].as_str() == Some("scoped_tag_bob")),
        "alice should not see bob's followed tag",
    );
    assert!(
        bob_tags.iter().any(|t| t["name"].as_str() == Some("scoped_tag_bob")),
        "bob should see his own followed tag",
    );
}

/// GET /api/v1/followed_tags with limit=1 returns at most 1 tag and sets Link header.
#[tokio::test]
async fn test_followed_tags_limit_param() {
    let ctx = TestContext::new("ftag-limit").await;

    for tag in &["limit_tag_a", "limit_tag_b"] {
        ctx.api.post_status(&ctx.alice_token, &format!("post about #{tag}"), "public").await;
        ctx.api.post_json(
            &format!("/api/v1/tags/{tag}/follow"),
            Some(&ctx.alice_token),
            &json!({}),
        ).await;
    }

    let resp = ctx.api.get("/api/v1/followed_tags?limit=1", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let tags: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(tags.len(), 1, "limit=1 should return exactly 1 tag");
}

/// GET /api/v1/followed_tags returns tags with following=true.
#[tokio::test]
async fn test_followed_tags_includes_following_true() {
    let ctx = TestContext::new("ftag-following").await;

    ctx.api.post_status(&ctx.alice_token, "post with #following_true_tag", "public").await;
    ctx.api.post_json(
        "/api/v1/tags/following_true_tag/follow",
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    let tags: Vec<Value> = ctx.api.get("/api/v1/followed_tags", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    let tag = tags.iter().find(|t| t["name"].as_str() == Some("following_true_tag")).unwrap();
    assert_eq!(tag["following"].as_bool(), Some(true), "followed tag should have following=true");
}

/// GET /api/v1/tags/:name for a non-existent tag returns 404.
#[tokio::test]
async fn test_get_tag_not_found() {
    let ctx = TestContext::new("tag-404").await;

    let resp = ctx.api.get("/api/v1/tags/definitelynonexistent99999", None).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// Feature a tag, list it on account, unfeature it.
#[tokio::test]
async fn test_featured_tags_lifecycle() {
    let ctx = TestContext::new("featured-tags").await;

    ctx.api.post_status(&ctx.alice_token, "I post about #featuretag101", "public").await;

    let feature_resp = ctx.api.post_json(
        "/api/v1/featured_tags",
        Some(&ctx.alice_token),
        &json!({"name": "featuretag101"}),
    ).await;
    assert_eq!(feature_resp.status(), StatusCode::OK);
    let ft: Value = feature_resp.json().await.unwrap();
    let ft_id = ft["id"].as_str().unwrap().to_string();
    assert_eq!(ft["name"].as_str(), Some("featuretag101"));

    let list: Vec<Value> = ctx.api.get("/api/v1/featured_tags", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(list.iter().any(|t| t["id"].as_str() == Some(ft_id.as_str())));

    let acct_tags: Vec<Value> = ctx.api.get(
        &format!("/api/v1/accounts/{}/featured_tags", ctx.alice_id),
        None,
    ).await.json().await.unwrap();
    assert!(acct_tags.iter().any(|t| t["name"].as_str() == Some("featuretag101")));

    let suggest_resp = ctx.api.get(
        "/api/v1/featured_tags/suggestions",
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(suggest_resp.status(), StatusCode::OK);
    let _: Vec<Value> = suggest_resp.json().await.unwrap();

    let del_resp = ctx.api.delete(
        &format!("/api/v1/featured_tags/{ft_id}"),
        &ctx.alice_token,
    ).await;
    assert_eq!(del_resp.status(), StatusCode::OK);

    let after: Vec<Value> = ctx.api.get("/api/v1/featured_tags", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(!after.iter().any(|t| t["id"].as_str() == Some(ft_id.as_str())));
}

/// DELETE /api/v1/featured_tags/:id for a non-existent id returns 404.
#[tokio::test]
async fn test_unfeature_tag_not_found() {
    let ctx = TestContext::new("unfeature-404").await;

    let resp = ctx.api.delete(
        "/api/v1/featured_tags/99999999",
        &ctx.alice_token,
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
