use reqwest::StatusCode;
use serde_json::{json, Value};

use super::helpers::TestContext;

/// Following creates a follow notification for the followee.
#[tokio::test]
async fn test_follow_creates_notification() {
    let ctx = TestContext::new("notif-follow").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let resp = ctx.api.get("/api/v1/notifications", Some(&ctx.bob_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let notifs: Vec<Value> = resp.json().await.unwrap();

    let follow_notif = notifs.iter().find(|n| n["type"].as_str() == Some("follow"));
    assert!(follow_notif.is_some(), "no follow notification found");
    assert_eq!(
        follow_notif.unwrap()["account"]["id"].as_str(),
        Some(ctx.alice_id.as_str()),
    );
}

/// Favouriting creates a favourite notification for the status author.
#[tokio::test]
async fn test_favourite_creates_notification() {
    let ctx = TestContext::new("notif-fav").await;

    let status = ctx.api.post_status(&ctx.alice_token, "faveable notification test", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/favourite"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;

    let notifs: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    let fav_notif = notifs.iter().find(|n| n["type"].as_str() == Some("favourite"));
    assert!(fav_notif.is_some(), "no favourite notification found");
    assert_eq!(
        fav_notif.unwrap()["account"]["id"].as_str(),
        Some(ctx.bob_id.as_str()),
    );
}

/// Replying with a mention creates a mention notification.
#[tokio::test]
async fn test_reply_creates_mention_notification() {
    let ctx = TestContext::new("notif-mention").await;

    let parent = ctx.api.post_status(&ctx.alice_token, "parent for mention", "public").await;
    let parent_id = parent["id"].as_str().unwrap();

    ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.bob_token),
        &json!({
            "status": format!("@alice reply here"),
            "in_reply_to_id": parent_id,
            "visibility": "public"
        }),
    ).await;

    let notifs: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    let mention_notif = notifs.iter().find(|n| n["type"].as_str() == Some("mention"));
    assert!(mention_notif.is_some(), "no mention notification found");
}

/// GET /api/v1/notifications/:id/dismiss removes the notification.
#[tokio::test]
async fn test_dismiss_notification() {
    let ctx = TestContext::new("notif-dismiss").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let notifs: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(!notifs.is_empty(), "no notifications to dismiss");
    let notif_id = notifs[0]["id"].as_str().unwrap();

    let dismiss_resp = ctx.api.post_json(
        &format!("/api/v1/notifications/{notif_id}/dismiss"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(dismiss_resp.status(), StatusCode::OK);

    let after: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(
        !after.iter().any(|n| n["id"].as_str() == Some(notif_id)),
        "dismissed notification still appears",
    );
}

/// POST /api/v1/notifications/clear removes all notifications.
#[tokio::test]
async fn test_clear_notifications() {
    let ctx = TestContext::new("notif-clear").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let clear_resp = ctx.api.post_json(
        "/api/v1/notifications/clear",
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(clear_resp.status(), StatusCode::OK);

    let after: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(after.is_empty(), "notifications not cleared");
}

/// Reblogging creates a reblog notification for the status author.
#[tokio::test]
async fn test_reblog_creates_notification() {
    let ctx = TestContext::new("notif-reblog").await;

    let status = ctx.api.post_status(&ctx.alice_token, "reblog notify me", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/reblog"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;

    let notifs: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    let reblog_notif = notifs.iter().find(|n| n["type"].as_str() == Some("reblog"));
    assert!(reblog_notif.is_some(), "no reblog notification found");
    assert_eq!(
        reblog_notif.unwrap()["account"]["id"].as_str(),
        Some(ctx.bob_id.as_str()),
    );
}

/// GET /api/v1/notifications?types[]=follow returns only follow notifications.
#[tokio::test]
async fn test_notification_filter_types() {
    let ctx = TestContext::new("notif-types").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let status = ctx.api.post_status(&ctx.bob_token, "filterable", "public").await;
    let id = status["id"].as_str().unwrap();
    ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/favourite"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    let notifs: Vec<Value> = ctx.api.get(
        "/api/v1/notifications?types[]=follow",
        Some(&ctx.bob_token),
    ).await.json().await.unwrap();
    for n in &notifs {
        assert_eq!(n["type"].as_str(), Some("follow"),
            "non-follow notification returned when filtering for follow");
    }
}

/// GET /api/v1/notifications?exclude_types[]=follow omits follow notifications.
#[tokio::test]
async fn test_notification_exclude_types() {
    let ctx = TestContext::new("notif-excl").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let notifs: Vec<Value> = ctx.api.get(
        "/api/v1/notifications?exclude_types[]=follow",
        Some(&ctx.bob_token),
    ).await.json().await.unwrap();
    assert!(
        !notifs.iter().any(|n| n["type"].as_str() == Some("follow")),
        "follow notification appeared despite exclusion",
    );
}
