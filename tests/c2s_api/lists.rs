use reqwest::StatusCode;
use serde_json::{json, Value};

use super::helpers::TestContext;

/// Full list lifecycle: create, rename, list, delete.
#[tokio::test]
async fn test_list_crud() {
    let ctx = TestContext::new("list-crud").await;

    let create_resp = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.alice_token),
        &json!({"title": "My Friends"}),
    ).await;
    assert_eq!(create_resp.status(), StatusCode::OK);
    let list: Value = create_resp.json().await.unwrap();
    let list_id = list["id"].as_str().unwrap().to_string();
    assert_eq!(list["title"].as_str(), Some("My Friends"));

    // GET single list
    let get_resp = ctx.api.get(&format!("/api/v1/lists/{list_id}"), Some(&ctx.alice_token)).await;
    assert_eq!(get_resp.status(), StatusCode::OK);
    let got: Value = get_resp.json().await.unwrap();
    assert_eq!(got["id"].as_str(), Some(list_id.as_str()));

    // GET all lists
    let all: Vec<Value> = ctx.api.get("/api/v1/lists", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(all.iter().any(|l| l["id"].as_str() == Some(list_id.as_str())));

    // PUT to rename
    let rename_resp = ctx.api.put_json(
        &format!("/api/v1/lists/{list_id}"),
        Some(&ctx.alice_token),
        &json!({"title": "Close Friends"}),
    ).await;
    assert_eq!(rename_resp.status(), StatusCode::OK);
    let renamed: Value = rename_resp.json().await.unwrap();
    assert_eq!(renamed["title"].as_str(), Some("Close Friends"));

    // DELETE
    let del_resp = ctx.api.delete(&format!("/api/v1/lists/{list_id}"), &ctx.alice_token).await;
    assert_eq!(del_resp.status(), StatusCode::OK);

    let gone: Vec<Value> = ctx.api.get("/api/v1/lists", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(!gone.iter().any(|l| l["id"].as_str() == Some(list_id.as_str())));
}

/// Adding and removing accounts from a list.
#[tokio::test]
async fn test_list_add_and_remove_accounts() {
    let ctx = TestContext::new("list-accounts").await;

    // Alice must follow Bob before adding him to a list.
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let list: Value = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.alice_token),
        &json!({"title": "Test List"}),
    ).await.json().await.unwrap();
    let list_id = list["id"].as_str().unwrap();

    // Add Bob
    let add_resp = ctx.api.post_json(
        &format!("/api/v1/lists/{list_id}/accounts"),
        Some(&ctx.alice_token),
        &json!({"account_ids": [ctx.bob_id]}),
    ).await;
    assert_eq!(add_resp.status(), StatusCode::OK);

    let members: Vec<Value> = ctx.api.get(
        &format!("/api/v1/lists/{list_id}/accounts"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    assert!(members.iter().any(|a| a["id"].as_str() == Some(ctx.bob_id.as_str())));

    // Remove Bob
    let rm_resp = ctx.api.http
        .delete(ctx.api.url(&format!("/api/v1/lists/{list_id}/accounts")))
        .header("host", &ctx.api.host)
        .bearer_auth(&ctx.alice_token)
        .json(&json!({"account_ids": [ctx.bob_id]}))
        .send().await.unwrap();
    assert_eq!(rm_resp.status(), StatusCode::OK);

    let after: Vec<Value> = ctx.api.get(
        &format!("/api/v1/lists/{list_id}/accounts"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    assert!(!after.iter().any(|a| a["id"].as_str() == Some(ctx.bob_id.as_str())));
}

/// GET /api/v1/timelines/list/:id shows posts from list members.
#[tokio::test]
async fn test_list_timeline() {
    let ctx = TestContext::new("list-tl").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let list: Value = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.alice_token),
        &json!({"title": "TL List"}),
    ).await.json().await.unwrap();
    let list_id = list["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/lists/{list_id}/accounts"),
        Some(&ctx.alice_token),
        &json!({"account_ids": [ctx.bob_id]}),
    ).await;

    let status = ctx.api.post_status(&ctx.bob_token, "bob on the list", "public").await;
    let status_id = status["id"].as_str().unwrap();

    let timeline: Vec<Value> = ctx.api.get(
        &format!("/api/v1/timelines/list/{list_id}"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();

    assert!(
        timeline.iter().any(|s| s["id"].as_str() == Some(status_id)),
        "Bob's status should appear in the list timeline",
    );
}
