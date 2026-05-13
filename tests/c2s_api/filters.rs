use reqwest::StatusCode;
use serde_json::{json, Value};

use super::helpers::TestContext;

/// Full filter v1 lifecycle: create → get → list → update → delete.
#[tokio::test]
async fn test_filter_v1_crud() {
    let ctx = TestContext::new("filter-v1").await;

    let create_resp = ctx.api.post_json(
        "/api/v1/filters",
        Some(&ctx.alice_token),
        &json!({
            "phrase": "badword",
            "context": ["home", "notifications"],
            "irreversible": false,
            "whole_word": true
        }),
    ).await;
    assert_eq!(create_resp.status(), StatusCode::OK);
    let filter: Value = create_resp.json().await.unwrap();
    let filter_id = filter["id"].as_str().unwrap().to_string();
    assert_eq!(filter["phrase"].as_str(), Some("badword"));
    assert!(filter["context"].as_array().unwrap().iter().any(|c| c == "home"));

    let get_resp = ctx.api.get(
        &format!("/api/v1/filters/{filter_id}"),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(get_resp.status(), StatusCode::OK);
    let f: Value = get_resp.json().await.unwrap();
    assert_eq!(f["phrase"].as_str(), Some("badword"));

    let list: Vec<Value> = ctx.api.get("/api/v1/filters", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(list.iter().any(|f| f["id"].as_str() == Some(filter_id.as_str())));

    let update_resp = ctx.api.put_json(
        &format!("/api/v1/filters/{filter_id}"),
        Some(&ctx.alice_token),
        &json!({
            "phrase": "badword2",
            "context": ["home"],
        }),
    ).await;
    assert_eq!(update_resp.status(), StatusCode::OK);
    let updated: Value = update_resp.json().await.unwrap();
    assert_eq!(updated["phrase"].as_str(), Some("badword2"));

    let del_resp = ctx.api.delete(
        &format!("/api/v1/filters/{filter_id}"),
        &ctx.alice_token,
    ).await;
    assert_eq!(del_resp.status(), StatusCode::OK);

    let gone_resp = ctx.api.get(
        &format!("/api/v1/filters/{filter_id}"),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(gone_resp.status(), StatusCode::NOT_FOUND);
}

/// Full filter v2 lifecycle including keyword management.
#[tokio::test]
async fn test_filter_v2_crud() {
    let ctx = TestContext::new("filter-v2").await;

    let create_resp = ctx.api.post_json(
        "/api/v2/filters",
        Some(&ctx.alice_token),
        &json!({
            "title": "Spam Filter",
            "context": ["home", "public"],
            "filter_action": "warn",
            "keywords_attributes": [
                {"keyword": "spam", "whole_word": false}
            ]
        }),
    ).await;
    assert_eq!(create_resp.status(), StatusCode::OK);
    let filter: Value = create_resp.json().await.unwrap();
    let filter_id = filter["id"].as_str().unwrap().to_string();
    assert_eq!(filter["title"].as_str(), Some("Spam Filter"));
    assert!(filter["keywords"].as_array().unwrap().iter().any(|k| k["keyword"] == "spam"));

    let get_resp = ctx.api.get(
        &format!("/api/v2/filters/{filter_id}"),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(get_resp.status(), StatusCode::OK);

    let list: Vec<Value> = ctx.api.get("/api/v2/filters", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(list.iter().any(|f| f["id"].as_str() == Some(filter_id.as_str())));

    let update_resp = ctx.api.put_json(
        &format!("/api/v2/filters/{filter_id}"),
        Some(&ctx.alice_token),
        &json!({
            "title": "Updated Filter",
            "context": ["home"],
            "filter_action": "hide"
        }),
    ).await;
    assert_eq!(update_resp.status(), StatusCode::OK);
    let updated: Value = update_resp.json().await.unwrap();
    assert_eq!(updated["title"].as_str(), Some("Updated Filter"));
    assert_eq!(updated["filter_action"].as_str(), Some("hide"));

    // Add keyword
    let add_kw_resp = ctx.api.post_json(
        &format!("/api/v2/filters/{filter_id}/keywords"),
        Some(&ctx.alice_token),
        &json!({"keyword": "junk", "whole_word": true}),
    ).await;
    assert_eq!(add_kw_resp.status(), StatusCode::OK);
    let kw: Value = add_kw_resp.json().await.unwrap();
    let kw_id = kw["id"].as_str().unwrap().to_string();
    assert_eq!(kw["keyword"].as_str(), Some("junk"));

    // List keywords
    let kws: Vec<Value> = ctx.api.get(
        &format!("/api/v2/filters/{filter_id}/keywords"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    assert!(kws.iter().any(|k| k["keyword"] == "junk"));

    // Get single keyword
    let kw_resp = ctx.api.get(
        &format!("/api/v2/filter_keywords/{kw_id}"),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(kw_resp.status(), StatusCode::OK);

    // Update keyword
    let upd_kw: Value = ctx.api.put_json(
        &format!("/api/v2/filter_keywords/{kw_id}"),
        Some(&ctx.alice_token),
        &json!({"keyword": "garbage", "whole_word": false}),
    ).await.json().await.unwrap();
    assert_eq!(upd_kw["keyword"].as_str(), Some("garbage"));

    // Delete keyword
    let del_kw_resp = ctx.api.delete(
        &format!("/api/v2/filter_keywords/{kw_id}"),
        &ctx.alice_token,
    ).await;
    assert_eq!(del_kw_resp.status(), StatusCode::OK);

    // Delete filter
    let del_resp = ctx.api.delete(
        &format!("/api/v2/filters/{filter_id}"),
        &ctx.alice_token,
    ).await;
    assert_eq!(del_resp.status(), StatusCode::OK);
}
