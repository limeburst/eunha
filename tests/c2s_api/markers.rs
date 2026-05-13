use reqwest::StatusCode;
use serde_json::Value;

use super::helpers::TestContext;

/// Set the home marker and get it back.
#[tokio::test]
async fn test_markers_home() {
    let ctx = TestContext::new("markers-home").await;

    let status = ctx.api.post_status(&ctx.alice_token, "marker test", "public").await;
    let id = status["id"].as_str().unwrap();

    let set_resp = ctx.api.post_form(
        "/api/v1/markers",
        Some(&ctx.alice_token),
        &[("home[last_read_id]", id)],
    ).await;
    assert_eq!(set_resp.status(), StatusCode::OK);
    let markers: Value = set_resp.json().await.unwrap();
    assert_eq!(markers["home"]["last_read_id"].as_str(), Some(id));

    let get_resp = ctx.api.get(
        "/api/v1/markers?timeline[]=home",
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(get_resp.status(), StatusCode::OK);
    let markers2: Value = get_resp.json().await.unwrap();
    assert_eq!(markers2["home"]["last_read_id"].as_str(), Some(id));
}

/// Set the notifications marker and get it back.
#[tokio::test]
async fn test_markers_notifications() {
    let ctx = TestContext::new("markers-notif").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;
    let notifs: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    let notif_id = notifs[0]["id"].as_str().unwrap();

    let set_resp = ctx.api.post_form(
        "/api/v1/markers",
        Some(&ctx.bob_token),
        &[("notifications[last_read_id]", notif_id)],
    ).await;
    assert_eq!(set_resp.status(), StatusCode::OK);
    let markers: Value = set_resp.json().await.unwrap();
    assert_eq!(markers["notifications"]["last_read_id"].as_str(), Some(notif_id));
}

/// Updating the home marker increments the version.
#[tokio::test]
async fn test_marker_version_increments() {
    let ctx = TestContext::new("markers-ver").await;

    let s1 = ctx.api.post_status(&ctx.alice_token, "marker v1", "public").await;
    let s2 = ctx.api.post_status(&ctx.alice_token, "marker v2", "public").await;
    let id1 = s1["id"].as_str().unwrap();
    let id2 = s2["id"].as_str().unwrap();

    ctx.api.post_form("/api/v1/markers", Some(&ctx.alice_token), &[("home[last_read_id]", id1)]).await;
    let m: Value = ctx.api.post_form("/api/v1/markers", Some(&ctx.alice_token), &[("home[last_read_id]", id2)])
        .await.json().await.unwrap();

    assert!(m["home"]["version"].as_i64().unwrap_or(0) >= 2, "version should be ≥ 2 after two updates");
}
