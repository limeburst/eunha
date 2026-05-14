use reqwest::StatusCode;
use serde_json::{json, Value};

use super::helpers::TestContext;

/// Direct message creates a conversation visible to both sender and recipient.
#[tokio::test]
async fn test_conversations_lifecycle() {
    let ctx = TestContext::new("conv").await;

    let dm_resp = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "@bob hello in private",
            "visibility": "direct"
        }),
    ).await;
    assert_eq!(dm_resp.status(), StatusCode::OK);

    let convs: Vec<Value> = ctx.api.get("/api/v1/conversations", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(!convs.is_empty(), "conversation not created for recipient");
    let conv_id = convs[0]["id"].as_str().unwrap().to_string();
    assert_eq!(convs[0]["unread"].as_bool(), Some(true));

    let read_resp = ctx.api.post_json(
        &format!("/api/v1/conversations/{conv_id}/read"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(read_resp.status(), StatusCode::OK);
    let conv: Value = read_resp.json().await.unwrap();
    assert_eq!(conv["unread"].as_bool(), Some(false));

    let del_resp = ctx.api.delete(
        &format!("/api/v1/conversations/{conv_id}"),
        &ctx.bob_token,
    ).await;
    assert_eq!(del_resp.status(), StatusCode::OK);

    let after: Vec<Value> = ctx.api.get("/api/v1/conversations", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(!after.iter().any(|c| c["id"].as_str() == Some(conv_id.as_str())));
}

/// GET /api/v1/conversations respects limit parameter.
#[tokio::test]
async fn test_conversations_limit_param() {
    let ctx = TestContext::new("conv-limit").await;

    for i in 0..3 {
        ctx.api.post_json(
            "/api/v1/statuses",
            Some(&ctx.alice_token),
            &json!({
                "status": format!("@bob conv limit {i}"),
                "visibility": "direct"
            }),
        ).await;
    }

    let convs: Vec<Value> = ctx.api.get("/api/v1/conversations?limit=2", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(convs.len() <= 2, "limit=2 returned {} conversations", convs.len());
}

/// GET /api/v1/conversations since_id returns only conversations newer than the given id.
#[tokio::test]
async fn test_conversations_since_id() {
    let ctx = TestContext::new("conv-since").await;

    // First DM — older.
    ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({"status": "@bob conv since old", "visibility": "direct"}),
    ).await;

    let convs_all: Vec<Value> = ctx.api.get("/api/v1/conversations", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(!convs_all.is_empty());
    let old_conv_id = convs_all[0]["id"].as_str().unwrap().to_string();

    // Delete it so we can send a fresh DM and get a new conversation.
    ctx.api.delete(&format!("/api/v1/conversations/{old_conv_id}"), &ctx.bob_token).await;

    // Second DM — newer (fresh conversation).
    ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({"status": "@bob conv since new", "visibility": "direct"}),
    ).await;

    let after: Vec<Value> = ctx.api.get(
        &format!("/api/v1/conversations?since_id={old_conv_id}"),
        Some(&ctx.bob_token),
    ).await.json().await.unwrap();

    // The new conversation id should be > old_conv_id.
    let new_id: i64 = after[0]["id"].as_str().unwrap().parse().unwrap();
    let old_id: i64 = old_conv_id.parse().unwrap();
    assert!(new_id > old_id, "since_id should only return conversations newer than anchor");
}

/// GET /api/v1/conversations max_id returns only conversations older than the given id.
#[tokio::test]
async fn test_conversations_max_id() {
    let ctx = TestContext::new("conv-maxid").await;

    // First DM — older conversation.
    ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({"status": "@bob conv maxid old", "visibility": "direct"}),
    ).await;

    let convs_first: Vec<Value> = ctx.api.get("/api/v1/conversations", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(!convs_first.is_empty());
    let old_id = convs_first[0]["id"].as_str().unwrap().to_string();

    // Delete so next DM is a new conversation with a higher id.
    ctx.api.delete(&format!("/api/v1/conversations/{old_id}"), &ctx.bob_token).await;

    // Second DM — newer conversation.
    ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({"status": "@bob conv maxid new", "visibility": "direct"}),
    ).await;

    let convs_all: Vec<Value> = ctx.api.get("/api/v1/conversations", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(!convs_all.is_empty());
    let new_id = convs_all[0]["id"].as_str().unwrap().to_string();

    // max_id=new_id should exclude new_id and return only older ones.
    let with_max: Vec<Value> = ctx.api.get(
        &format!("/api/v1/conversations?max_id={new_id}"),
        Some(&ctx.bob_token),
    ).await.json().await.unwrap();

    assert!(
        !with_max.iter().any(|c| c["id"].as_str() == Some(new_id.as_str())),
        "max_id conversation itself should be excluded",
    );
}

/// Sender also sees their own conversation.
#[tokio::test]
async fn test_conversations_visible_to_sender() {
    let ctx = TestContext::new("conv-sender").await;

    ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "@bob a note to you",
            "visibility": "direct"
        }),
    ).await;

    let convs: Vec<Value> = ctx.api.get("/api/v1/conversations", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(!convs.is_empty(), "sender should also see the conversation");
}
