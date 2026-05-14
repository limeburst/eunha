use reqwest::StatusCode;
use serde_json::Value;

use super::helpers::TestContext;

/// GET /api/v1/favourites returns statuses the user has favourited.
#[tokio::test]
async fn test_favourites_returns_favourited_statuses() {
    let ctx = TestContext::new("favs-list").await;

    let status = ctx
        .api
        .post_status(&ctx.alice_token, "fav listing test", "public")
        .await;
    let status_id = status["id"].as_str().unwrap().to_string();

    ctx.api
        .post_json(
            &format!("/api/v1/statuses/{status_id}/favourite"),
            Some(&ctx.bob_token),
            &serde_json::json!({}),
        )
        .await;

    let resp = ctx.api.get("/api/v1/favourites", Some(&ctx.bob_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Vec<Value> = resp.json().await.unwrap();
    let ids: Vec<&str> = body.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(ids.contains(&status_id.as_str()), "favourited status missing from /api/v1/favourites");
}

/// GET /api/v1/favourites is empty when nothing has been favourited.
#[tokio::test]
async fn test_favourites_empty_when_none() {
    let ctx = TestContext::new("favs-empty").await;

    let resp = ctx.api.get("/api/v1/favourites", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Vec<Value> = resp.json().await.unwrap();
    assert!(body.is_empty());
}

/// GET /api/v1/favourites requires authentication.
#[tokio::test]
async fn test_favourites_requires_auth() {
    let ctx = TestContext::new("favs-unauth").await;

    let resp = ctx.api.get("/api/v1/favourites", None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Unfavouriting a status removes it from GET /api/v1/favourites.
#[tokio::test]
async fn test_favourites_unfavourite_removes_from_list() {
    let ctx = TestContext::new("favs-unfav").await;

    let status = ctx
        .api
        .post_status(&ctx.alice_token, "unfav listing test", "public")
        .await;
    let status_id = status["id"].as_str().unwrap().to_string();

    ctx.api
        .post_json(
            &format!("/api/v1/statuses/{status_id}/favourite"),
            Some(&ctx.bob_token),
            &serde_json::json!({}),
        )
        .await;

    ctx.api
        .post_json(
            &format!("/api/v1/statuses/{status_id}/unfavourite"),
            Some(&ctx.bob_token),
            &serde_json::json!({}),
        )
        .await;

    let resp = ctx.api.get("/api/v1/favourites", Some(&ctx.bob_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Vec<Value> = resp.json().await.unwrap();
    let ids: Vec<&str> = body.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(!ids.contains(&status_id.as_str()), "unfavourited status still in favourites list");
}

/// max_id pagination excludes the given status and newer ones.
#[tokio::test]
async fn test_favourites_max_id_pagination() {
    let ctx = TestContext::new("favs-maxid").await;

    let s1 = ctx.api.post_status(&ctx.alice_token, "fav maxid first", "public").await;
    let s2 = ctx.api.post_status(&ctx.alice_token, "fav maxid second", "public").await;
    let s1_id = s1["id"].as_str().unwrap().to_string();
    let s2_id = s2["id"].as_str().unwrap().to_string();

    for id in [&s1_id, &s2_id] {
        ctx.api.post_json(
            &format!("/api/v1/statuses/{id}/favourite"),
            Some(&ctx.bob_token),
            &serde_json::json!({}),
        ).await;
    }

    let paged: Vec<Value> = ctx.api.get(
        &format!("/api/v1/favourites?max_id={s2_id}"),
        Some(&ctx.bob_token),
    ).await.json().await.unwrap();

    assert!(
        !paged.iter().any(|s| s["id"].as_str() == Some(s2_id.as_str())),
        "max_id favourite should be excluded",
    );
    assert!(
        paged.iter().any(|s| s["id"].as_str() == Some(s1_id.as_str())),
        "s1 should appear when max_id=s2_id",
    );
}

/// since_id pagination returns only favourites newer than the given id.
#[tokio::test]
async fn test_favourites_since_id_pagination() {
    let ctx = TestContext::new("favs-since").await;

    let s1 = ctx.api.post_status(&ctx.alice_token, "fav since first", "public").await;
    let s2 = ctx.api.post_status(&ctx.alice_token, "fav since second", "public").await;
    let s1_id = s1["id"].as_str().unwrap().to_string();
    let s2_id = s2["id"].as_str().unwrap().to_string();

    for id in [&s1_id, &s2_id] {
        ctx.api.post_json(
            &format!("/api/v1/statuses/{id}/favourite"),
            Some(&ctx.bob_token),
            &serde_json::json!({}),
        ).await;
    }

    let paged: Vec<Value> = ctx.api.get(
        &format!("/api/v1/favourites?since_id={s1_id}"),
        Some(&ctx.bob_token),
    ).await.json().await.unwrap();

    assert!(
        !paged.iter().any(|s| s["id"].as_str() == Some(s1_id.as_str())),
        "since_id favourite should be excluded",
    );
    assert!(
        paged.iter().any(|s| s["id"].as_str() == Some(s2_id.as_str())),
        "s2 should appear when since_id=s1_id",
    );
}

/// The limit parameter caps the number of results returned.
#[tokio::test]
async fn test_favourites_limit_param() {
    let ctx = TestContext::new("favs-limit").await;

    for i in 0..3 {
        let s = ctx
            .api
            .post_status(&ctx.alice_token, &format!("fav limit {i}"), "public")
            .await;
        let sid = s["id"].as_str().unwrap().to_string();
        ctx.api
            .post_json(
                &format!("/api/v1/statuses/{sid}/favourite"),
                Some(&ctx.bob_token),
                &serde_json::json!({}),
            )
            .await;
    }

    let resp = ctx
        .api
        .get("/api/v1/favourites?limit=2", Some(&ctx.bob_token))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(body.len(), 2, "limit=2 should return exactly 2 items");
}
