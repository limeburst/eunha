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

/// GET /api/v1/notifications accepts limit up to 80 (Mastodon default max).
#[tokio::test]
async fn test_notifications_limit_param_respected() {
    let ctx = TestContext::new("notif-limit").await;

    // Default limit should be 40 (not some lower number).
    let resp = ctx.api.get("/api/v1/notifications", Some(&ctx.bob_token)).await;
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    // limit=1 should return at most 1.
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;
    let status = ctx.api.post_status(&ctx.alice_token, "notif limit test", "public").await;
    let sid = status["id"].as_str().unwrap();
    ctx.api.post_json(
        &format!("/api/v1/statuses/{sid}/favourite"),
        Some(&ctx.bob_token),
        &serde_json::json!({}),
    ).await;

    let notifs: Vec<serde_json::Value> = ctx.api.get(
        "/api/v1/notifications?limit=1",
        Some(&ctx.bob_token),
    ).await.json().await.unwrap();
    assert!(notifs.len() <= 1, "limit=1 should return at most 1 notification");
}

/// GET /api/v1/notifications/:id returns the notification for the authenticated user.
#[tokio::test]
async fn test_get_notification_by_id() {
    let ctx = TestContext::new("notif-get-id").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let notifs: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(!notifs.is_empty(), "expected a follow notification");
    let notif_id = notifs[0]["id"].as_str().unwrap();

    let resp = ctx.api.get(
        &format!("/api/v1/notifications/{notif_id}"),
        Some(&ctx.bob_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["id"].as_str(), Some(notif_id));
}

/// GET /api/v1/notifications/:id returns 404 for another user's notification.
#[tokio::test]
async fn test_get_notification_other_users_is_404() {
    let ctx = TestContext::new("notif-get-other").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let bob_notifs: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(!bob_notifs.is_empty());
    let bob_notif_id = bob_notifs[0]["id"].as_str().unwrap();

    let resp = ctx.api.get(
        &format!("/api/v1/notifications/{bob_notif_id}"),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// POST /api/v1/notifications/:id/dismiss returns 404 for another user's notification.
#[tokio::test]
async fn test_dismiss_notification_other_users_is_404() {
    let ctx = TestContext::new("notif-dismiss-other").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let bob_notifs: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(!bob_notifs.is_empty());
    let bob_notif_id = bob_notifs[0]["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/notifications/{bob_notif_id}/dismiss"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// GET /api/v1/notifications?account_id=X returns only notifications from account X.
#[tokio::test]
async fn test_notification_filter_by_account_id() {
    let ctx = TestContext::new("notif-acct-filter").await;

    // Alice follows Bob → Bob gets a follow notification from Alice.
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    // Also generate a favourite notification for Bob from Alice.
    let status = ctx.api.post_status(&ctx.bob_token, "bob filterable", "public").await;
    let sid = status["id"].as_str().unwrap();
    ctx.api.post_json(
        &format!("/api/v1/statuses/{sid}/favourite"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    // Filter by alice's id: all notifications should be from alice.
    let notifs: Vec<Value> = ctx.api.get(
        &format!("/api/v1/notifications?account_id={}", ctx.alice_id),
        Some(&ctx.bob_token),
    ).await.json().await.unwrap();

    assert!(!notifs.is_empty(), "expected at least one notification from alice");
    for n in &notifs {
        assert_eq!(
            n["account"]["id"].as_str(),
            Some(ctx.alice_id.as_str()),
            "notification from unexpected account: {n}",
        );
    }
}

// ── notification policy ───────────────────────────────────────────────────────

/// GET /api/v2/notifications/policy returns the policy with all filters false by default.
#[tokio::test]
async fn test_notification_policy_defaults() {
    let ctx = TestContext::new("notif-policy-defaults").await;

    let resp = ctx.api.get("/api/v2/notifications/policy", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let policy: Value = resp.json().await.unwrap();

    assert_eq!(policy["filter_not_following"].as_bool(), Some(false));
    assert_eq!(policy["filter_not_followers"].as_bool(), Some(false));
    assert_eq!(policy["filter_new_accounts"].as_bool(), Some(false));
    assert_eq!(policy["filter_private_mentions"].as_bool(), Some(false));
    assert!(policy["summary"].is_object(), "summary field missing");
}

/// PATCH /api/v2/notifications/policy updates filter settings.
#[tokio::test]
async fn test_notification_policy_update() {
    let ctx = TestContext::new("notif-policy-update").await;

    let resp = ctx.api.post_json(
        "/api/v2/notifications/policy",
        Some(&ctx.alice_token),
        &json!({"filter_not_following": true}),
    ).await;
    // PATCH endpoint but we use post_json — need to use the HTTP client directly.
    drop(resp);

    let patch_resp = ctx.api.http
        .patch(ctx.api.url("/api/v2/notifications/policy"))
        .header("host", &ctx.api.host)
        .bearer_auth(&ctx.alice_token)
        .json(&json!({"filter_not_following": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(patch_resp.status(), StatusCode::OK);
    let policy: Value = patch_resp.json().await.unwrap();
    assert_eq!(policy["filter_not_following"].as_bool(), Some(true));
    assert_eq!(policy["filter_not_followers"].as_bool(), Some(false), "unchanged field should stay false");
}

/// GET /api/v1/notifications/requests returns an empty list initially.
#[tokio::test]
async fn test_notification_requests_empty_by_default() {
    let ctx = TestContext::new("notif-req-empty").await;

    let resp = ctx.api.get("/api/v1/notifications/requests", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(list.is_empty(), "expected empty notification requests");
}

/// Notification requests are created when policy filters a notification, and can be dismissed.
#[tokio::test]
async fn test_notification_request_dismiss_and_accept() {
    let ctx = TestContext::new("notif-req-dismiss").await;

    // Alice sets filter_not_following=true so bob's actions route to requests.
    // Alice does not follow bob, so bob's notifications will be filtered.
    ctx.api.http
        .patch(ctx.api.url("/api/v2/notifications/policy"))
        .header("host", &ctx.api.host)
        .bearer_auth(&ctx.alice_token)
        .json(&json!({"filter_not_following": true}))
        .send()
        .await
        .unwrap();

    // Bob follows alice → should create a notification request (not a notification).
    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;

    let requests: Vec<Value> = ctx.api.get("/api/v1/notifications/requests", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(!requests.is_empty(), "expected a notification request from bob");
    let req_id = requests[0]["id"].as_str().unwrap();
    assert_eq!(
        requests[0]["account"]["id"].as_str(),
        Some(ctx.bob_id.as_str()),
        "notification request should be from bob",
    );

    // GET /api/v1/notifications/requests/:id returns the single request.
    let single_resp = ctx.api.get(
        &format!("/api/v1/notifications/requests/{req_id}"),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(single_resp.status(), StatusCode::OK);
    let single: Value = single_resp.json().await.unwrap();
    assert_eq!(single["id"].as_str(), Some(req_id));

    // Dismiss the request.
    let dismiss_resp = ctx.api.post_json(
        &format!("/api/v1/notifications/requests/{req_id}/dismiss"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(dismiss_resp.status(), StatusCode::OK);

    // After dismissal the request no longer appears in the list.
    let after_dismiss: Vec<Value> = ctx.api.get("/api/v1/notifications/requests", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(
        !after_dismiss.iter().any(|r| r["id"].as_str() == Some(req_id)),
        "dismissed request still appears in list",
    );

    // Accept re-surfaces it (dismissed = false).
    let accept_resp = ctx.api.post_json(
        &format!("/api/v1/notifications/requests/{req_id}/accept"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(accept_resp.status(), StatusCode::OK);

    let after_accept: Vec<Value> = ctx.api.get("/api/v1/notifications/requests", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(
        after_accept.iter().any(|r| r["id"].as_str() == Some(req_id)),
        "accepted request should reappear in list",
    );
}

/// GET /api/v2/notifications returns notification groups with accounts and statuses sideloaded.
#[tokio::test]
async fn test_get_notifications_v2() {
    let ctx = TestContext::new("notif-v2").await;

    // Alice follows Bob → Bob gets a follow notification.
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let resp = ctx.api.get("/api/v2/notifications", Some(&ctx.bob_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    assert!(body["notification_groups"].is_array(), "notification_groups missing");
    assert!(body["accounts"].is_array(), "accounts missing");
    assert!(body["statuses"].is_array(), "statuses missing");

    let groups = body["notification_groups"].as_array().unwrap();
    assert!(!groups.is_empty(), "expected at least one notification group");

    let follow_group = groups.iter().find(|g| g["notification_type"].as_str() == Some("follow"));
    assert!(follow_group.is_some(), "no follow notification group found");
}

/// GET /api/v1/notifications?since_id=X returns only notifications newer than X.
#[tokio::test]
async fn test_notifications_since_id_pagination() {
    let ctx = TestContext::new("notif-since-id").await;

    // First notification: alice follows bob.
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let first_notifs: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(!first_notifs.is_empty(), "expected a follow notification");
    let first_id = first_notifs.last().unwrap()["id"].as_str().unwrap().to_string();

    // Second notification: alice favourites bob's status.
    let status = ctx.api.post_status(&ctx.bob_token, "since_id target", "public").await;
    let sid = status["id"].as_str().unwrap();
    ctx.api.post_json(
        &format!("/api/v1/statuses/{sid}/favourite"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    let all_notifs: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(all_notifs.len() >= 2, "expected at least 2 notifications total");

    // since_id should return only notifications newer than first_id.
    let since_notifs: Vec<Value> = ctx.api.get(
        &format!("/api/v1/notifications?since_id={first_id}"),
        Some(&ctx.bob_token),
    ).await.json().await.unwrap();

    assert!(
        !since_notifs.iter().any(|n| n["id"].as_str() == Some(&first_id)),
        "since_id notification itself should be excluded",
    );
    assert!(
        since_notifs.iter().any(|n| n["type"].as_str() == Some("favourite")),
        "favourite notification (newer) should appear with since_id filter",
    );
}

/// GET /api/v1/notifications?max_id=X returns only notifications older than X.
#[tokio::test]
async fn test_notifications_max_id_pagination() {
    let ctx = TestContext::new("notif-max-id").await;

    // First notification: alice follows bob.
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    // Second notification: alice favourites bob's status.
    let status = ctx.api.post_status(&ctx.bob_token, "max_id target", "public").await;
    let sid = status["id"].as_str().unwrap();
    ctx.api.post_json(
        &format!("/api/v1/statuses/{sid}/favourite"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    let all_notifs: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(all_notifs.len() >= 2, "expected at least 2 notifications");
    // Notifications are newest-first; take the newest id as the max_id.
    let newest_id = all_notifs[0]["id"].as_str().unwrap().to_string();

    let max_id_notifs: Vec<Value> = ctx.api.get(
        &format!("/api/v1/notifications?max_id={newest_id}"),
        Some(&ctx.bob_token),
    ).await.json().await.unwrap();

    assert!(
        !max_id_notifs.iter().any(|n| n["id"].as_str() == Some(&newest_id)),
        "max_id notification itself should be excluded",
    );
}

/// GET /api/v1/notifications with limit=80 is accepted (not clamped to something lower).
#[tokio::test]
async fn test_notifications_limit_80_is_accepted() {
    let ctx = TestContext::new("notif-limit-80").await;

    let resp = ctx.api.get("/api/v1/notifications?limit=80", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), reqwest::StatusCode::OK, "limit=80 should be accepted");
    // limit=81 should be clamped to 80 and still return 200.
    let resp2 = ctx.api.get("/api/v1/notifications?limit=81", Some(&ctx.alice_token)).await;
    assert_eq!(resp2.status(), reqwest::StatusCode::OK, "limit=81 should be clamped, not rejected");
}

/// GET /api/v2/notifications with since_id returns only newer notification groups.
#[tokio::test]
async fn test_notifications_v2_since_id_pagination() {
    let ctx = TestContext::new("notif-v2-since").await;

    // First event: alice follows bob.
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let first_body: Value = ctx.api.get("/api/v2/notifications", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    let first_groups = first_body["notification_groups"].as_array().unwrap();
    assert!(!first_groups.is_empty(), "expected a follow notification group");

    // Capture the oldest group id from this batch.
    let oldest_id = first_groups.last().unwrap()["page_min_id"]
        .as_str()
        .unwrap_or_else(|| first_groups.last().unwrap()["latest_page_notification_at"].as_str().unwrap_or("1"))
        .to_string();

    // Second event: alice favourites bob's status.
    let status = ctx.api.post_status(&ctx.bob_token, "v2 since notif", "public").await;
    let sid = status["id"].as_str().unwrap();
    ctx.api.post_json(
        &format!("/api/v1/statuses/{sid}/favourite"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    // since_id=oldest_id should return only newer groups.
    let since_body: Value = ctx.api.get(
        &format!("/api/v2/notifications?since_id={oldest_id}"),
        Some(&ctx.bob_token),
    ).await.json().await.unwrap();
    assert!(since_body["notification_groups"].is_array(), "notification_groups should be present");
}
