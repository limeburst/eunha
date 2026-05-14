use reqwest::StatusCode;
use serde_json::{json, Value};

use super::helpers::TestContext;

/// GET /api/v1/featured_tags returns an empty array when no tags are featured.
#[tokio::test]
async fn test_featured_tags_empty_initially() {
    let ctx = TestContext::new("ftag-empty").await;

    let resp = ctx.api.get("/api/v1/featured_tags", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(list.is_empty(), "expected empty featured tags list, got: {list:?}");
}

/// GET /api/v1/featured_tags requires authentication.
#[tokio::test]
async fn test_featured_tags_requires_auth() {
    let ctx = TestContext::new("ftag-auth").await;

    let resp = ctx.api.get("/api/v1/featured_tags", None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// POST /api/v1/featured_tags creates a featured tag and GET returns it.
#[tokio::test]
async fn test_create_and_list_featured_tags() {
    let ctx = TestContext::new("ftag-create").await;

    let create_resp = ctx.api.post_json(
        "/api/v1/featured_tags",
        Some(&ctx.alice_token),
        &json!({"name": "rustlang"}),
    ).await;
    assert_eq!(create_resp.status(), StatusCode::OK);
    let tag: Value = create_resp.json().await.unwrap();
    assert_eq!(tag["name"].as_str(), Some("rustlang"));
    assert!(tag["id"].as_str().is_some(), "id field missing");
    assert!(tag["url"].as_str().is_some(), "url field missing");

    let list: Vec<Value> = ctx.api.get("/api/v1/featured_tags", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(
        list.iter().any(|t| t["name"].as_str() == Some("rustlang")),
        "created tag not in list: {list:?}",
    );
}

/// POST /api/v1/featured_tags with a tag name that already exists returns 200 (idempotent).
#[tokio::test]
async fn test_feature_tag_is_idempotent() {
    let ctx = TestContext::new("ftag-idem").await;

    ctx.api.post_json(
        "/api/v1/featured_tags",
        Some(&ctx.alice_token),
        &json!({"name": "idempotent"}),
    ).await;

    let second = ctx.api.post_json(
        "/api/v1/featured_tags",
        Some(&ctx.alice_token),
        &json!({"name": "idempotent"}),
    ).await;
    assert_eq!(second.status(), StatusCode::OK);

    let list: Vec<Value> = ctx.api.get("/api/v1/featured_tags", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    let count = list.iter().filter(|t| t["name"].as_str() == Some("idempotent")).count();
    assert_eq!(count, 1, "duplicate featured tags created after idempotent POST");
}

/// DELETE /api/v1/featured_tags/:id removes the tag.
#[tokio::test]
async fn test_unfeature_tag() {
    let ctx = TestContext::new("ftag-del").await;

    let tag: Value = ctx.api.post_json(
        "/api/v1/featured_tags",
        Some(&ctx.alice_token),
        &json!({"name": "toremove"}),
    ).await.json().await.unwrap();
    let tag_id = tag["id"].as_str().unwrap();

    let del_resp = ctx.api.delete(
        &format!("/api/v1/featured_tags/{tag_id}"),
        &ctx.alice_token,
    ).await;
    assert_eq!(del_resp.status(), StatusCode::OK);

    let list: Vec<Value> = ctx.api.get("/api/v1/featured_tags", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(
        !list.iter().any(|t| t["id"].as_str() == Some(tag_id)),
        "deleted tag still in list",
    );
}

/// DELETE /api/v1/featured_tags/:id returns 404 for a nonexistent id.
#[tokio::test]
async fn test_unfeature_tag_not_found() {
    let ctx = TestContext::new("ftag-del-404").await;

    let resp = ctx.api.delete("/api/v1/featured_tags/999999999", &ctx.alice_token).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// DELETE /api/v1/featured_tags/:id returns 404 for another user's tag.
#[tokio::test]
async fn test_unfeature_other_users_tag_is_404() {
    let ctx = TestContext::new("ftag-del-other").await;

    let tag: Value = ctx.api.post_json(
        "/api/v1/featured_tags",
        Some(&ctx.bob_token),
        &json!({"name": "bobstag"}),
    ).await.json().await.unwrap();
    let tag_id = tag["id"].as_str().unwrap();

    let resp = ctx.api.delete(
        &format!("/api/v1/featured_tags/{tag_id}"),
        &ctx.alice_token,
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// POST /api/v1/featured_tags creates a tag that does not appear in another user's list.
#[tokio::test]
async fn test_featured_tags_scoped_to_user() {
    let ctx = TestContext::new("ftag-scoped").await;

    ctx.api.post_json(
        "/api/v1/featured_tags",
        Some(&ctx.alice_token),
        &json!({"name": "alicesonly"}),
    ).await;

    let bob_list: Vec<Value> = ctx.api.get("/api/v1/featured_tags", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(
        !bob_list.iter().any(|t| t["name"].as_str() == Some("alicesonly")),
        "alice's featured tag should not appear in bob's list",
    );
}

/// GET /api/v1/featured_tags/suggestions returns a JSON array.
#[tokio::test]
async fn test_featured_tag_suggestions() {
    let ctx = TestContext::new("ftag-suggest").await;

    let resp = ctx.api.get("/api/v1/featured_tags/suggestions", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let _: Vec<Value> = resp.json().await.unwrap();
}
