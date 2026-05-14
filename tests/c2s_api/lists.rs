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

/// GET /api/v1/lists/:id returns 404 when the list does not exist.
#[tokio::test]
async fn test_get_list_not_found() {
    let ctx = TestContext::new("list-404").await;

    let resp = ctx.api.get("/api/v1/lists/999999999", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// GET /api/v1/lists/:id returns 404 when the list belongs to another user.
#[tokio::test]
async fn test_get_list_other_user_is_404() {
    let ctx = TestContext::new("list-other-user").await;

    let list: Value = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.bob_token),
        &json!({"title": "Bob's private list"}),
    ).await.json().await.unwrap();
    let list_id = list["id"].as_str().unwrap();

    let resp = ctx.api.get(&format!("/api/v1/lists/{list_id}"), Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// GET /api/v1/lists response includes replies_policy and exclusive fields.
#[tokio::test]
async fn test_list_response_includes_replies_policy_and_exclusive() {
    let ctx = TestContext::new("list-fields").await;

    let list: Value = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.alice_token),
        &json!({"title": "Policy List", "replies_policy": "followed", "exclusive": true}),
    ).await.json().await.unwrap();

    assert_eq!(list["replies_policy"].as_str(), Some("followed"));
    assert_eq!(list["exclusive"].as_bool(), Some(true));

    let list_id = list["id"].as_str().unwrap();
    let fetched: Value = ctx.api.get(&format!("/api/v1/lists/{list_id}"), Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert_eq!(fetched["replies_policy"].as_str(), Some("followed"));
    assert_eq!(fetched["exclusive"].as_bool(), Some(true));
}

/// POST /api/v1/lists with empty title returns 422.
#[tokio::test]
async fn test_create_list_empty_title_returns_422() {
    let ctx = TestContext::new("list-empty-title").await;

    let resp = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.alice_token),
        &json!({"title": ""}),
    ).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// POST /api/v1/lists with invalid replies_policy returns 422.
#[tokio::test]
async fn test_create_list_invalid_replies_policy_returns_422() {
    let ctx = TestContext::new("list-bad-policy").await;

    let resp = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.alice_token),
        &json!({"title": "My List", "replies_policy": "whatever"}),
    ).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["error"].as_str().unwrap_or("").contains("Replies policy"),
        "error message should mention replies_policy: {body}",
    );
}

/// PUT /api/v1/lists/:id updates replies_policy and exclusive.
#[tokio::test]
async fn test_update_list_replies_policy_and_exclusive() {
    let ctx = TestContext::new("list-update-policy").await;

    let list: Value = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.alice_token),
        &json!({"title": "Update Policy", "replies_policy": "list", "exclusive": false}),
    ).await.json().await.unwrap();
    let list_id = list["id"].as_str().unwrap();

    let updated: Value = ctx.api.put_json(
        &format!("/api/v1/lists/{list_id}"),
        Some(&ctx.alice_token),
        &json!({"title": "Update Policy", "replies_policy": "followed", "exclusive": true}),
    ).await.json().await.unwrap();

    assert_eq!(updated["replies_policy"].as_str(), Some("followed"));
    assert_eq!(updated["exclusive"].as_bool(), Some(true));
}

/// PUT /api/v1/lists/:id returns 404 when the list does not exist.
#[tokio::test]
async fn test_update_list_not_found() {
    let ctx = TestContext::new("list-put-404").await;

    let resp = ctx.api.put_json(
        "/api/v1/lists/999999999",
        Some(&ctx.alice_token),
        &json!({"title": "Ghost List"}),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// PUT /api/v1/lists/:id returns 404 when the list belongs to another user.
#[tokio::test]
async fn test_update_list_other_user_is_404() {
    let ctx = TestContext::new("list-put-other").await;

    let list: Value = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.bob_token),
        &json!({"title": "Bob's List"}),
    ).await.json().await.unwrap();
    let list_id = list["id"].as_str().unwrap();

    let resp = ctx.api.put_json(
        &format!("/api/v1/lists/{list_id}"),
        Some(&ctx.alice_token),
        &json!({"title": "Hijacked"}),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// DELETE /api/v1/lists/:id returns 404 when the list does not exist.
#[tokio::test]
async fn test_delete_list_not_found() {
    let ctx = TestContext::new("list-del-404").await;

    let resp = ctx.api.delete("/api/v1/lists/999999999", &ctx.alice_token).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// DELETE /api/v1/lists/:id returns 404 when the list belongs to another user.
#[tokio::test]
async fn test_delete_list_other_user_is_404() {
    let ctx = TestContext::new("list-del-other").await;

    let list: Value = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.bob_token),
        &json!({"title": "Bob's List to Delete"}),
    ).await.json().await.unwrap();
    let list_id = list["id"].as_str().unwrap();

    let resp = ctx.api.delete(&format!("/api/v1/lists/{list_id}"), &ctx.alice_token).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
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

/// List timeline respects max_id pagination.
#[tokio::test]
async fn test_list_timeline_max_id_pagination() {
    let ctx = TestContext::new("list-tl-maxid").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let list: Value = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.alice_token),
        &json!({"title": "MaxId List"}),
    ).await.json().await.unwrap();
    let list_id = list["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/lists/{list_id}/accounts"),
        Some(&ctx.alice_token),
        &json!({"account_ids": [ctx.bob_id]}),
    ).await;

    let s1 = ctx.api.post_status(&ctx.bob_token, "list maxid first", "public").await;
    let s2 = ctx.api.post_status(&ctx.bob_token, "list maxid second", "public").await;
    let s1_id = s1["id"].as_str().unwrap();
    let s2_id = s2["id"].as_str().unwrap();

    let paged: Vec<Value> = ctx.api.get(
        &format!("/api/v1/timelines/list/{list_id}?max_id={s2_id}"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();

    assert!(
        !paged.iter().any(|s| s["id"].as_str() == Some(s2_id)),
        "max_id status should be excluded",
    );
    assert!(
        paged.iter().any(|s| s["id"].as_str() == Some(s1_id)),
        "s1 should appear when max_id=s2_id",
    );
}

/// POST /api/v1/lists/:id/accounts returns 422 when the account is not followed.
#[tokio::test]
async fn test_list_add_unfollowed_account_returns_422() {
    let ctx = TestContext::new("list-add-unfollow").await;

    let list: Value = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.alice_token),
        &json!({"title": "No Follow List"}),
    ).await.json().await.unwrap();
    let list_id = list["id"].as_str().unwrap();

    // Alice does NOT follow Bob. Adding Bob to a list should return 422.
    let resp = ctx.api.post_json(
        &format!("/api/v1/lists/{list_id}/accounts"),
        Some(&ctx.alice_token),
        &json!({"account_ids": [ctx.bob_id]}),
    ).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY, "adding unfollowed account should return 422");
}

/// List timeline respects since_id pagination.
#[tokio::test]
async fn test_list_timeline_since_id_pagination() {
    let ctx = TestContext::new("list-tl-since").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let list: Value = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.alice_token),
        &json!({"title": "SinceId List"}),
    ).await.json().await.unwrap();
    let list_id = list["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/lists/{list_id}/accounts"),
        Some(&ctx.alice_token),
        &json!({"account_ids": [ctx.bob_id]}),
    ).await;

    let s1 = ctx.api.post_status(&ctx.bob_token, "list since first", "public").await;
    let s2 = ctx.api.post_status(&ctx.bob_token, "list since second", "public").await;
    let s1_id = s1["id"].as_str().unwrap();
    let s2_id = s2["id"].as_str().unwrap();

    let paged: Vec<Value> = ctx.api.get(
        &format!("/api/v1/timelines/list/{list_id}?since_id={s1_id}"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();

    assert!(
        !paged.iter().any(|s| s["id"].as_str() == Some(s1_id)),
        "since_id status should be excluded",
    );
    assert!(
        paged.iter().any(|s| s["id"].as_str() == Some(s2_id)),
        "s2 should appear when since_id=s1_id",
    );
}

/// Accounts in an exclusive list are excluded from home timeline.
#[tokio::test]
async fn test_exclusive_list_excludes_from_home_timeline() {
    let ctx = TestContext::new("excl-list-home").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let list: Value = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.alice_token),
        &json!({"title": "Exclusive", "exclusive": true}),
    ).await.json().await.unwrap();
    let list_id = list["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/lists/{list_id}/accounts"),
        Some(&ctx.alice_token),
        &json!({"account_ids": [ctx.bob_id]}),
    ).await;

    let status = ctx.api.post_status(&ctx.bob_token, "exclusivetermXYZ", "public").await;
    let status_id = status["id"].as_str().unwrap();

    // Bob's status should NOT appear on Alice's home timeline (exclusive list).
    let home: Vec<Value> = ctx.api.get("/api/v1/timelines/home", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(
        !home.iter().any(|s| s["id"].as_str() == Some(status_id)),
        "exclusive list member's status should be excluded from home timeline",
    );

    // But it should appear on the list timeline.
    let list_tl: Vec<Value> = ctx.api.get(
        &format!("/api/v1/timelines/list/{list_id}"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    assert!(
        list_tl.iter().any(|s| s["id"].as_str() == Some(status_id)),
        "exclusive list member's status should appear on list timeline",
    );
}

/// List timeline with replies_policy=none hides replies.
#[tokio::test]
async fn test_list_timeline_replies_policy_none() {
    let ctx = TestContext::new("list-rep-none").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let list: Value = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.alice_token),
        &json!({"title": "NoReplies", "replies_policy": "none"}),
    ).await.json().await.unwrap();
    let list_id = list["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/lists/{list_id}/accounts"),
        Some(&ctx.alice_token),
        &json!({"account_ids": [ctx.bob_id]}),
    ).await;

    // Normal post
    let s1 = ctx.api.post_status(&ctx.bob_token, "not a reply", "public").await;
    let s1_id = s1["id"].as_str().unwrap();
    // Reply to own status
    let s2: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.bob_token),
        &serde_json::json!({"status": "a reply here", "visibility": "public", "in_reply_to_id": s1_id}),
    ).await.json().await.unwrap();
    let s2_id = s2["id"].as_str().unwrap();

    let list_tl: Vec<Value> = ctx.api.get(
        &format!("/api/v1/timelines/list/{list_id}"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();

    assert!(
        list_tl.iter().any(|s| s["id"].as_str() == Some(s1_id)),
        "non-reply should appear in list with replies_policy=none",
    );
    assert!(
        !list_tl.iter().any(|s| s["id"].as_str() == Some(s2_id)),
        "reply should be hidden in list with replies_policy=none",
    );
}

/// List timeline with replies_policy=list shows replies only when replying to another list member.
#[tokio::test]
async fn test_list_timeline_replies_policy_list() {
    let ctx = TestContext::new("list-rep-list").await;

    // Create a third user (charlie) inline.
    let (charlie_id, charlie_token) =
        super::helpers::seed_user(&ctx.db, &ctx.domain, "charlie", "charlie@test.invalid").await;
    let charlie_id = charlie_id.to_string();

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;
    ctx.api.follow(&ctx.alice_token, &charlie_id).await;

    let list: Value = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.alice_token),
        &json!({"title": "ListPolicy", "replies_policy": "list"}),
    ).await.json().await.unwrap();
    let list_id = list["id"].as_str().unwrap();

    // Add bob but not charlie to the list.
    ctx.api.post_json(
        &format!("/api/v1/lists/{list_id}/accounts"),
        Some(&ctx.alice_token),
        &json!({"account_ids": [ctx.bob_id]}),
    ).await;

    let charlie_post = ctx.api.post_status(&charlie_token, "charlie says hi", "public").await;
    let charlie_post_id = charlie_post["id"].as_str().unwrap();

    let bob_post = ctx.api.post_status(&ctx.bob_token, "bob says hi", "public").await;
    let bob_post_id = bob_post["id"].as_str().unwrap();

    // Bob replies to Charlie (not in list) — should be hidden.
    let reply_to_charlie: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.bob_token),
        &serde_json::json!({"status": "reply to charlie", "visibility": "public", "in_reply_to_id": charlie_post_id}),
    ).await.json().await.unwrap();
    let reply_to_charlie_id = reply_to_charlie["id"].as_str().unwrap();

    // Bob replies to his own post (bob is in list) — should be visible.
    let reply_to_bob: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.bob_token),
        &serde_json::json!({"status": "reply to bob", "visibility": "public", "in_reply_to_id": bob_post_id}),
    ).await.json().await.unwrap();
    let reply_to_bob_id = reply_to_bob["id"].as_str().unwrap();

    let list_tl: Vec<Value> = ctx.api.get(
        &format!("/api/v1/timelines/list/{list_id}"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();

    assert!(
        !list_tl.iter().any(|s| s["id"].as_str() == Some(reply_to_charlie_id)),
        "reply to non-list-member should be hidden with replies_policy=list",
    );
    assert!(
        list_tl.iter().any(|s| s["id"].as_str() == Some(reply_to_bob_id)),
        "reply to list member should be visible with replies_policy=list",
    );
}
