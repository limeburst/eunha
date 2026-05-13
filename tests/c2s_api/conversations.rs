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
