use reqwest::StatusCode;
use serde_json::{json, Value};

use super::helpers::TestContext;

// ── status access control ────────────────────────────────────────────────────

/// GET a private status as an unauthenticated stranger → 404.
#[tokio::test]
async fn test_get_private_status_unauthenticated() {
    let ctx = TestContext::new("prv-unauth").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice private", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.get(&format!("/api/v1/statuses/{id}"), None).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// GET a private status as a non-follower → 404.
#[tokio::test]
async fn test_get_private_status_non_follower() {
    let ctx = TestContext::new("prv-stranger").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice private", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx
        .api
        .get(&format!("/api/v1/statuses/{id}"), Some(&ctx.bob_token))
        .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// GET a private status as an accepted follower → 200.
#[tokio::test]
async fn test_get_private_status_accepted_follower() {
    let ctx = TestContext::new("prv-follower").await;

    // Bob follows Alice.
    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice private", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx
        .api
        .get(&format!("/api/v1/statuses/{id}"), Some(&ctx.bob_token))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

/// GET a private status as the author → 200.
#[tokio::test]
async fn test_get_private_status_author() {
    let ctx = TestContext::new("prv-author").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice private", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx
        .api
        .get(&format!("/api/v1/statuses/{id}"), Some(&ctx.alice_token))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

/// GET a direct status as an accepted follower → 404.
#[tokio::test]
async fn test_get_direct_status_follower() {
    let ctx = TestContext::new("dir-follower").await;

    // Bob follows Alice.
    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice direct", "direct").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx
        .api
        .get(&format!("/api/v1/statuses/{id}"), Some(&ctx.bob_token))
        .await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "follower should not see direct status"
    );
}

/// GET a direct status as the author → 200.
#[tokio::test]
async fn test_get_direct_status_author() {
    let ctx = TestContext::new("dir-author").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice direct", "direct").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx
        .api
        .get(&format!("/api/v1/statuses/{id}"), Some(&ctx.alice_token))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

/// GET a direct status as a mentioned recipient → 200.
#[tokio::test]
async fn test_get_direct_status_mentioned_recipient() {
    let ctx = TestContext::new("dir-recipient").await;

    // Alice DMs Bob.
    let status: serde_json::Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "@bob hello dm",
            "visibility": "direct",
        }),
    ).await.json().await.unwrap();
    let id = status["id"].as_str().unwrap();

    // Bob (mentioned recipient) should be able to see it.
    let resp = ctx.api.get(&format!("/api/v1/statuses/{id}"), Some(&ctx.bob_token)).await;
    assert_eq!(resp.status(), StatusCode::OK, "mentioned recipient should see direct status");
}

/// GET a direct status as a non-mentioned third party → 404.
#[tokio::test]
async fn test_get_direct_status_non_recipient() {
    let ctx = TestContext::new("dir-non-recipient").await;

    // Alice DMs Bob (NOT Charlie).
    let status: serde_json::Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "@bob private hello",
            "visibility": "direct",
        }),
    ).await.json().await.unwrap();
    let id = status["id"].as_str().unwrap();

    // Charlie is not mentioned → should not see it.
    let (_, charlie_token) = super::helpers::seed_user(
        &ctx.db, &ctx.domain, "charlie-dir-nonrec", "charlie-dir-nonrec@test.invalid"
    ).await;
    let resp = ctx.api.get(&format!("/api/v1/statuses/{id}"), Some(&charlie_token)).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "non-recipient should not see direct status");
}

// ── block visibility ─────────────────────────────────────────────────────────

/// GET a public status when the viewer has been blocked by the author → 404.
#[tokio::test]
async fn test_get_status_blocked_by_author_returns_404() {
    let ctx = TestContext::new("status-blocked-by-author").await;

    let status = ctx.api.post_status(&ctx.alice_token, "public post", "public").await;
    let id = status["id"].as_str().unwrap();

    // Alice blocks Bob.
    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/block", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    let resp = ctx.api.get(&format!("/api/v1/statuses/{id}"), Some(&ctx.bob_token)).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND,
        "blocked user should not be able to view the blocker's status");
}

/// GET a public status when the viewer has blocked the author → 404.
#[tokio::test]
async fn test_get_status_viewer_blocked_author_returns_404() {
    let ctx = TestContext::new("status-viewer-blocks").await;

    let status = ctx.api.post_status(&ctx.alice_token, "public post", "public").await;
    let id = status["id"].as_str().unwrap();

    // Bob blocks Alice.
    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/block", ctx.alice_id),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;

    let resp = ctx.api.get(&format!("/api/v1/statuses/{id}"), Some(&ctx.bob_token)).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND,
        "blocker should not see blocked account's status via direct lookup");
}

// ── reblog restrictions ───────────────────────────────────────────────────────

/// Reblogging a private status of a non-followed account → 404 (status not visible).
#[tokio::test]
async fn test_reblog_private_status_of_stranger_returns_404() {
    let ctx = TestContext::new("reblog-prv-stranger").await;

    // Alice posts private; Bob does NOT follow Alice.
    let status = ctx.api.post_status(&ctx.alice_token, "private from stranger", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/reblog"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// Unreblogs of a private status from a non-followed account → 404.
#[tokio::test]
async fn test_unreblog_private_status_of_stranger_returns_404() {
    let ctx = TestContext::new("unreblog-prv-stranger").await;

    let status = ctx.api.post_status(&ctx.alice_token, "private never reblogged", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/unreblog"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// Reblogging a private status → 403.
#[tokio::test]
async fn test_reblog_private_returns_403() {
    let ctx = TestContext::new("reblog-prv").await;

    // Bob follows Alice so he can see her private status.
    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice private rb", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx
        .api
        .post_json(
            &format!("/api/v1/statuses/{id}/reblog"),
            Some(&ctx.bob_token),
            &json!({}),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// Reblogging a direct status by a non-recipient → 404 (status not visible).
#[tokio::test]
async fn test_reblog_direct_returns_404() {
    let ctx = TestContext::new("reblog-dir").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice direct rb", "direct").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx
        .api
        .post_json(
            &format!("/api/v1/statuses/{id}/reblog"),
            Some(&ctx.bob_token),
            &json!({}),
        )
        .await;
    // Direct messages are hidden from non-recipients, so the server returns 404.
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── authentication requirements ──────────────────────────────────────────────

/// POST /api/v1/statuses without a token → 401.
#[tokio::test]
async fn test_post_status_requires_auth() {
    let ctx = TestContext::new("auth-post").await;

    let resp = ctx
        .api
        .post_json(
            "/api/v1/statuses",
            None,
            &json!({"status": "no auth", "visibility": "public"}),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// DELETE /api/v1/statuses/:id without a token → 401.
#[tokio::test]
async fn test_delete_status_requires_auth() {
    let ctx = TestContext::new("auth-del").await;

    let status = ctx.api.post_status(&ctx.alice_token, "to delete", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx
        .api
        .http
        .delete(ctx.api.url(&format!("/api/v1/statuses/{id}")))
        .header("host", &ctx.domain)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── soft delete ───────────────────────────────────────────────────────────────

/// Deleted status returns 404 on GET.
#[tokio::test]
async fn test_deleted_status_returns_410() {
    let ctx = TestContext::new("del-410").await;

    let status = ctx.api.post_status(&ctx.alice_token, "to be deleted", "public").await;
    let id = status["id"].as_str().unwrap();

    let del_resp = ctx.api.delete(&format!("/api/v1/statuses/{id}"), &ctx.alice_token).await;
    assert_eq!(del_resp.status(), StatusCode::OK);

    let get_resp = ctx.api.get(&format!("/api/v1/statuses/{id}"), None).await;
    assert_eq!(get_resp.status(), StatusCode::GONE);
}

/// Non-existent status returns 404 (distinct from deleted which returns 410).
#[tokio::test]
async fn test_nonexistent_status_returns_404() {
    let ctx = TestContext::new("nonexist-404").await;
    let get_resp = ctx.api.get("/api/v1/statuses/9999999999", None).await;
    assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
}

/// Deleted status is absent from the public timeline.
#[tokio::test]
async fn test_deleted_status_absent_from_public_timeline() {
    let ctx = TestContext::new("del-timeline").await;

    let status = ctx.api.post_status(&ctx.alice_token, "delete from timeline", "public").await;
    let id = status["id"].as_str().unwrap().to_string();

    ctx.api.delete(&format!("/api/v1/statuses/{id}"), &ctx.alice_token).await;

    let timeline = ctx.api.public_timeline().await;
    let ids: Vec<&str> = timeline.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(!ids.contains(&id.as_str()), "deleted status still appears in public timeline");
}

/// DELETE /api/v1/statuses/:id response includes the text field (supports delete-and-redraft).
#[tokio::test]
async fn test_delete_status_response_includes_text() {
    let ctx = TestContext::new("del-text").await;

    let status = ctx.api.post_status(&ctx.alice_token, "delete and redraft me", "public").await;
    let id = status["id"].as_str().unwrap();

    let del_resp = ctx.api.delete(&format!("/api/v1/statuses/{id}"), &ctx.alice_token).await;
    assert_eq!(del_resp.status(), StatusCode::OK);
    let body: Value = del_resp.json().await.unwrap();
    let text = body["text"].as_str().unwrap_or("");
    assert!(text.contains("delete and redraft"), "deleted status response should include original text");
}

/// Only the author can delete their own status; another user gets 403.
#[tokio::test]
async fn test_delete_status_by_non_author_returns_403() {
    let ctx = TestContext::new("del-author").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice status to del", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.delete(&format!("/api/v1/statuses/{id}"), &ctx.bob_token).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ── favourites ────────────────────────────────────────────────────────────────

/// Favouriting a private status of a non-followed account → 404.
#[tokio::test]
async fn test_favourite_private_status_of_stranger_returns_404() {
    let ctx = TestContext::new("fav-prv-stranger").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice private fav", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/favourite"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// Favouriting a private status of a followed account → 200.
#[tokio::test]
async fn test_favourite_private_status_of_followed_returns_200() {
    let ctx = TestContext::new("fav-prv-followed").await;

    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice private fav followed", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/favourite"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["favourited"].as_bool(), Some(true));
}

/// Favouriting without auth → 401.
#[tokio::test]
async fn test_favourite_requires_auth() {
    let ctx = TestContext::new("fav-401").await;

    let status = ctx.api.post_status(&ctx.alice_token, "fav auth test", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/favourite"),
        None,
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Unfavouriting a status that was not favourited returns 200 (idempotent).
#[tokio::test]
async fn test_unfavourite_not_favourited_is_200() {
    let ctx = TestContext::new("unfav-noop").await;

    let status = ctx.api.post_status(&ctx.alice_token, "never favourited", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/unfavourite"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["favourited"].as_bool(), Some(false));
}

/// Unfavouriting a private status of a non-followed account → 404.
#[tokio::test]
async fn test_unfavourite_private_status_of_stranger_returns_404() {
    let ctx = TestContext::new("unfav-prv-stranger").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice private unfav", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/unfavourite"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// Favouriting a status increments its favourites_count by 1.
#[tokio::test]
async fn test_favourite_increments_count() {
    let ctx = TestContext::new("fav-inc").await;

    let status = ctx.api.post_status(&ctx.alice_token, "favourable", "public").await;
    let id = status["id"].as_str().unwrap();
    let before: i64 = status["favourites_count"].as_i64().unwrap_or(0);

    let fav_resp = ctx
        .api
        .post_json(
            &format!("/api/v1/statuses/{id}/favourite"),
            Some(&ctx.bob_token),
            &json!({}),
        )
        .await;
    assert_eq!(fav_resp.status(), StatusCode::OK);
    let after: Value = fav_resp.json().await.unwrap();
    assert_eq!(after["favourites_count"].as_i64().unwrap_or(0), before + 1);
}

/// Unfavouriting a status decrements its favourites_count by 1.
#[tokio::test]
async fn test_unfavourite_decrements_count() {
    let ctx = TestContext::new("unfav-dec").await;

    let status = ctx.api.post_status(&ctx.alice_token, "unfavourable", "public").await;
    let id = status["id"].as_str().unwrap();

    // First favourite it.
    ctx.api
        .post_json(
            &format!("/api/v1/statuses/{id}/favourite"),
            Some(&ctx.bob_token),
            &json!({}),
        )
        .await;

    let unfav_resp = ctx
        .api
        .post_json(
            &format!("/api/v1/statuses/{id}/unfavourite"),
            Some(&ctx.bob_token),
            &json!({}),
        )
        .await;
    assert_eq!(unfav_resp.status(), StatusCode::OK);
    let after: Value = unfav_resp.json().await.unwrap();
    assert_eq!(after["favourites_count"].as_i64().unwrap_or(0), 0);
}

/// Double-favouriting doesn't inflate the count.
#[tokio::test]
async fn test_favourite_is_idempotent() {
    let ctx = TestContext::new("fav-idem").await;

    let status = ctx.api.post_status(&ctx.alice_token, "fav twice", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api
        .post_json(&format!("/api/v1/statuses/{id}/favourite"), Some(&ctx.bob_token), &json!({}))
        .await;
    let second = ctx
        .api
        .post_json(&format!("/api/v1/statuses/{id}/favourite"), Some(&ctx.bob_token), &json!({}))
        .await;
    let body: Value = second.json().await.unwrap();
    assert_eq!(body["favourites_count"].as_i64().unwrap_or(-1), 1);
}

// ── reblog count ──────────────────────────────────────────────────────────────

/// Reblogging a public status increments reblogs_count.
#[tokio::test]
async fn test_reblog_increments_count() {
    let ctx = TestContext::new("reblog-cnt").await;

    let status = ctx.api.post_status(&ctx.alice_token, "rebloggable", "public").await;
    let id = status["id"].as_str().unwrap();
    let before = status["reblogs_count"].as_i64().unwrap_or(0);

    let rb_resp = ctx
        .api
        .post_json(
            &format!("/api/v1/statuses/{id}/reblog"),
            Some(&ctx.bob_token),
            &json!({}),
        )
        .await;
    assert_eq!(rb_resp.status(), StatusCode::OK);

    // Fetch the original to check the count.
    let updated: Value = ctx
        .api
        .get(&format!("/api/v1/statuses/{id}"), Some(&ctx.bob_token))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(updated["reblogs_count"].as_i64().unwrap_or(0), before + 1);
}

/// Reblog response wraps the original in a `reblog` field with reblogged=true.
#[tokio::test]
async fn test_reblog_response_shape() {
    let ctx = TestContext::new("reblog-shape").await;

    let status = ctx.api.post_status(&ctx.alice_token, "reblog shape", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp: Value = ctx
        .api
        .post_json(&format!("/api/v1/statuses/{id}/reblog"), Some(&ctx.bob_token), &json!({}))
        .await
        .json()
        .await
        .unwrap();

    assert_eq!(resp["reblog"]["id"].as_str(), Some(id), "reblog.id should be the original status id");
    assert_eq!(resp["reblog"]["reblogged"], true, "reblog.reblogged should be true");
    assert_eq!(resp["reblog"]["reblogs_count"].as_i64(), Some(1));
}

/// Reblogging the same status twice is idempotent — count stays at 1.
#[tokio::test]
async fn test_reblog_idempotent() {
    let ctx = TestContext::new("reblog-idem").await;

    let status = ctx.api.post_status(&ctx.alice_token, "idempotent reblog", "public").await;
    let id = status["id"].as_str().unwrap();

    // First boost
    ctx.api.post_json(&format!("/api/v1/statuses/{id}/reblog"), Some(&ctx.bob_token), &json!({})).await;
    // Second boost of same status
    ctx.api.post_json(&format!("/api/v1/statuses/{id}/reblog"), Some(&ctx.bob_token), &json!({})).await;

    let updated: Value = ctx.api.get(&format!("/api/v1/statuses/{id}"), Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert_eq!(
        updated["reblogs_count"].as_i64(),
        Some(1),
        "double-reblog should not increment reblogs_count twice",
    );
}

/// Reblogging a reblog boosts the original status.
#[tokio::test]
async fn test_reblog_of_reblog_boosts_original() {
    let ctx = TestContext::new("reblog-chain").await;

    // Alice posts; Bob boosts it
    let original = ctx.api.post_status(&ctx.alice_token, "original post", "public").await;
    let original_id = original["id"].as_str().unwrap();

    let bob_boost: Value = ctx.api.post_json(
        &format!("/api/v1/statuses/{original_id}/reblog"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await.json().await.unwrap();
    let bob_boost_id = bob_boost["id"].as_str().unwrap();

    // Alice then boosts Bob's boost — should be boosting the original
    let alice_boost: Value = ctx.api.post_json(
        &format!("/api/v1/statuses/{bob_boost_id}/reblog"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await.json().await.unwrap();

    assert_eq!(
        alice_boost["reblog"]["id"].as_str(),
        Some(original_id),
        "boosting a boost should produce a boost of the original",
    );
}

/// Unreblog response is the original status at the top level with reblogged=false.
#[tokio::test]
async fn test_unreblog_response_shape() {
    let ctx = TestContext::new("unreblog-shape").await;

    let status = ctx.api.post_status(&ctx.alice_token, "unreblog shape", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api
        .post_json(&format!("/api/v1/statuses/{id}/reblog"), Some(&ctx.bob_token), &json!({}))
        .await;

    let resp: Value = ctx
        .api
        .post_json(&format!("/api/v1/statuses/{id}/unreblog"), Some(&ctx.bob_token), &json!({}))
        .await
        .json()
        .await
        .unwrap();

    assert_eq!(resp["id"].as_str(), Some(id), "unreblog should return the original status");
    assert_eq!(resp["reblogged"], false, "reblogged should be false after unreblog");
    assert_eq!(resp["reblogs_count"].as_i64(), Some(0));
}

// ── conversation mute ─────────────────────────────────────────────────────────

/// POST /api/v1/statuses/:id/mute sets muted=true on the status.
#[tokio::test]
async fn test_conversation_mute() {
    let ctx = TestContext::new("conv-mute").await;

    let status = ctx.api.post_status(&ctx.alice_token, "mutable conv", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx
        .api
        .post_json(&format!("/api/v1/statuses/{id}/mute"), Some(&ctx.alice_token), &json!({}))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["muted"], true, "status should be muted after POST .../mute");
}

/// POST /api/v1/statuses/:id/unmute sets muted=false on the status.
#[tokio::test]
async fn test_conversation_unmute() {
    let ctx = TestContext::new("conv-unmute").await;

    let status = ctx.api.post_status(&ctx.alice_token, "unmutable conv", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api
        .post_json(&format!("/api/v1/statuses/{id}/mute"), Some(&ctx.alice_token), &json!({}))
        .await;

    let resp = ctx
        .api
        .post_json(&format!("/api/v1/statuses/{id}/unmute"), Some(&ctx.alice_token), &json!({}))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["muted"], false, "status should be unmuted after POST .../unmute");
}

// ── status thread & context ──────────────────────────────────────────────────

/// A reply has in_reply_to_id set to the parent status id.
#[tokio::test]
async fn test_reply_sets_in_reply_to_id() {
    let ctx = TestContext::new("reply-id").await;

    let parent = ctx.api.post_status(&ctx.alice_token, "parent post", "public").await;
    let parent_id = parent["id"].as_str().unwrap();

    let reply_resp = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.bob_token),
        &json!({"status": "reply text", "in_reply_to_id": parent_id, "visibility": "public"}),
    ).await;
    assert_eq!(reply_resp.status(), StatusCode::OK);
    let reply: Value = reply_resp.json().await.unwrap();
    assert_eq!(reply["in_reply_to_id"].as_str(), Some(parent_id));
}

/// A cross-account reply sets in_reply_to_account_id to the parent author's id.
#[tokio::test]
async fn test_reply_sets_in_reply_to_account_id() {
    let ctx = TestContext::new("reply-acct-id").await;

    let parent = ctx.api.post_status(&ctx.alice_token, "parent for account id test", "public").await;
    let parent_id = parent["id"].as_str().unwrap();

    let reply: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.bob_token),
        &json!({"status": "reply for acct-id", "in_reply_to_id": parent_id, "visibility": "public"}),
    ).await.json().await.unwrap();

    assert_eq!(
        reply["in_reply_to_account_id"].as_str(),
        Some(ctx.alice_id.as_str()),
        "in_reply_to_account_id should be alice's id",
    );
}

/// GET /api/v1/statuses/:id/context returns ancestors and descendants.
#[tokio::test]
async fn test_status_context_ancestors_and_descendants() {
    let ctx = TestContext::new("ctx-thread").await;

    let grandparent = ctx.api.post_status(&ctx.alice_token, "grandparent", "public").await;
    let gp_id = grandparent["id"].as_str().unwrap();

    let parent: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.bob_token),
        &json!({"status": "parent", "in_reply_to_id": gp_id, "visibility": "public"}),
    ).await.json().await.unwrap();
    let p_id = parent["id"].as_str().unwrap();

    let child: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({"status": "child", "in_reply_to_id": p_id, "visibility": "public"}),
    ).await.json().await.unwrap();
    let c_id = child["id"].as_str().unwrap();

    let resp = ctx.api.get(&format!("/api/v1/statuses/{p_id}/context"), None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let ctx_body: Value = resp.json().await.unwrap();

    let ancestor_ids: Vec<&str> = ctx_body["ancestors"]
        .as_array().unwrap().iter()
        .filter_map(|s| s["id"].as_str())
        .collect();
    let descendant_ids: Vec<&str> = ctx_body["descendants"]
        .as_array().unwrap().iter()
        .filter_map(|s| s["id"].as_str())
        .collect();

    assert!(ancestor_ids.contains(&gp_id), "grandparent not in ancestors");
    assert!(descendant_ids.contains(&c_id), "child not in descendants");
}

// ── status edit & history ────────────────────────────────────────────────────

/// Editing a reblog returns 422 Unprocessable Entity.
#[tokio::test]
async fn test_edit_reblog_returns_422() {
    let ctx = TestContext::new("edit-reblog-422").await;

    let original = ctx.api.post_status(&ctx.alice_token, "original to boost", "public").await;
    let original_id = original["id"].as_str().unwrap();

    let rb_resp: Value = ctx.api.post_json(
        &format!("/api/v1/statuses/{original_id}/reblog"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await.json().await.unwrap();
    let reblog_id = rb_resp["id"].as_str().unwrap();

    let edit_resp = ctx.api.put_json(
        &format!("/api/v1/statuses/{reblog_id}"),
        Some(&ctx.bob_token),
        &json!({"status": "edited reblog", "visibility": "public"}),
    ).await;
    assert_eq!(edit_resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// Editing a status changes its content.
#[tokio::test]
async fn test_edit_status_changes_content() {
    let ctx = TestContext::new("edit-content").await;

    let status = ctx.api.post_status(&ctx.alice_token, "original text", "public").await;
    let id = status["id"].as_str().unwrap();

    let edit_resp = ctx.api.put_json(
        &format!("/api/v1/statuses/{id}"),
        Some(&ctx.alice_token),
        &json!({"status": "edited text", "visibility": "public"}),
    ).await;
    assert_eq!(edit_resp.status(), StatusCode::OK);
    let edited: Value = edit_resp.json().await.unwrap();
    // content is HTML — spaces may be encoded as &#32;
    let content = edited["content"].as_str().unwrap_or("");
    assert!(
        content.contains("edited"),
        "edited content not found: {content:?}"
    );
}

/// GET /api/v1/statuses/:id/history returns at least two entries after an edit.
#[tokio::test]
async fn test_status_history_after_edit() {
    let ctx = TestContext::new("edit-history").await;

    let status = ctx.api.post_status(&ctx.alice_token, "v1 text", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api.put_json(
        &format!("/api/v1/statuses/{id}"),
        Some(&ctx.alice_token),
        &json!({"status": "v2 text", "visibility": "public"}),
    ).await;

    let resp = ctx.api.get(&format!("/api/v1/statuses/{id}/history"), None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let history: Vec<Value> = resp.json().await.unwrap();
    assert!(history.len() >= 2, "expected at least 2 history entries, got {}", history.len());
}

/// GET /api/v1/statuses/:id/source returns the original plaintext.
#[tokio::test]
async fn test_status_source_returns_text() {
    let ctx = TestContext::new("status-src").await;

    let status = ctx.api.post_status(&ctx.alice_token, "source text here", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.get(&format!("/api/v1/statuses/{id}/source"), Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["text"].as_str().unwrap_or("").contains("source text here"),
        "source text not returned"
    );
}

/// Only the author can fetch status source; stranger gets 403.
#[tokio::test]
async fn test_status_source_forbidden_for_non_author() {
    let ctx = TestContext::new("status-src-403").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice's text", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.get(&format!("/api/v1/statuses/{id}/source"), Some(&ctx.bob_token)).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// GET /api/v1/statuses/:id/source after edit returns the updated text.
#[tokio::test]
async fn test_status_source_reflects_edit() {
    let ctx = TestContext::new("status-src-edit").await;

    let status = ctx.api.post_status(&ctx.alice_token, "original text", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api.put_json(
        &format!("/api/v1/statuses/{id}"),
        Some(&ctx.alice_token),
        &json!({"status": "updated text after edit"}),
    ).await;

    let src: Value = ctx.api.get(&format!("/api/v1/statuses/{id}/source"), Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(
        src["text"].as_str().unwrap_or("").contains("updated text after edit"),
        "source should reflect the updated text after editing",
    );
}

/// Editing a status owned by another user returns 403.
#[tokio::test]
async fn test_edit_status_by_non_author_returns_403() {
    let ctx = TestContext::new("edit-non-author").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice's status", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.put_json(
        &format!("/api/v1/statuses/{id}"),
        Some(&ctx.bob_token),
        &json!({"status": "bob tries to edit alice's status"}),
    ).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// Favouriting a nonexistent status returns 404.
#[tokio::test]
async fn test_favourite_nonexistent_status_returns_404() {
    let ctx = TestContext::new("fav-nonexist").await;

    let resp = ctx.api.post_json(
        "/api/v1/statuses/999999999/favourite",
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// Reblogging a nonexistent status returns 404.
#[tokio::test]
async fn test_reblog_nonexistent_status_returns_404() {
    let ctx = TestContext::new("reblog-nonexist").await;

    let resp = ctx.api.post_json(
        "/api/v1/statuses/999999999/reblog",
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// Status content warning (spoiler_text) round-trips correctly.
#[tokio::test]
async fn test_spoiler_text_preserved() {
    let ctx = TestContext::new("cw").await;

    let resp = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({"status": "body text", "spoiler_text": "content warning", "visibility": "public"}),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let status: Value = resp.json().await.unwrap();
    assert_eq!(status["spoiler_text"].as_str(), Some("content warning"));
}

// ── bookmarks ────────────────────────────────────────────────────────────────

/// Bookmarking a private status of a non-followed account → 404.
#[tokio::test]
async fn test_bookmark_private_status_of_stranger_returns_404() {
    let ctx = TestContext::new("bk-prv-stranger").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice private bk", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/bookmark"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// Bookmarking a private status of a followed account → 200.
#[tokio::test]
async fn test_bookmark_private_status_of_followed_returns_200() {
    let ctx = TestContext::new("bk-prv-followed").await;

    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice private bk followed", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/bookmark"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["bookmarked"].as_bool(), Some(true));
}

/// Bookmarking a nonexistent status → 404.
#[tokio::test]
async fn test_bookmark_nonexistent_returns_404() {
    let ctx = TestContext::new("bk-404").await;

    let resp = ctx.api.post_json(
        "/api/v1/statuses/999999999/bookmark",
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// Bookmarking without auth → 401.
#[tokio::test]
async fn test_bookmark_requires_auth() {
    let ctx = TestContext::new("bk-401").await;

    let status = ctx.api.post_status(&ctx.alice_token, "bk auth test", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/bookmark"),
        None,
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Unbookmarking a status that was not bookmarked returns 200 (idempotent).
#[tokio::test]
async fn test_unbookmark_not_bookmarked_is_200() {
    let ctx = TestContext::new("ubk-noop").await;

    let status = ctx.api.post_status(&ctx.alice_token, "never bookmarked", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/unbookmark"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["bookmarked"].as_bool(), Some(false));
}

/// Unbookmarking a private status of a non-followed account → 404.
#[tokio::test]
async fn test_unbookmark_private_status_of_stranger_returns_404() {
    let ctx = TestContext::new("ubk-prv-stranger").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice private ubk", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/unbookmark"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// Bookmarking and unbookmarking a status.
#[tokio::test]
async fn test_bookmark_and_unbookmark() {
    let ctx = TestContext::new("bookmark").await;

    let status = ctx.api.post_status(&ctx.alice_token, "bookmarkable", "public").await;
    let id = status["id"].as_str().unwrap();

    let bk_resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/bookmark"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(bk_resp.status(), StatusCode::OK);
    let bk: Value = bk_resp.json().await.unwrap();
    assert_eq!(bk["bookmarked"].as_bool(), Some(true));

    let ubk_resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/unbookmark"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(ubk_resp.status(), StatusCode::OK);
    let ubk: Value = ubk_resp.json().await.unwrap();
    assert_eq!(ubk["bookmarked"].as_bool(), Some(false));
}

/// GET /api/v1/bookmarks returns bookmarked statuses.
#[tokio::test]
async fn test_bookmarks_list() {
    let ctx = TestContext::new("bookmarks-list").await;

    let status = ctx.api.post_status(&ctx.alice_token, "bookmark me", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/bookmark"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    let resp = ctx.api.get("/api/v1/bookmarks", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(list.iter().any(|s| s["id"].as_str() == Some(id)));
}

// ── pin / unpin ───────────────────────────────────────────────────────────────

/// Pinning and unpinning a status.
#[tokio::test]
async fn test_pin_and_unpin() {
    let ctx = TestContext::new("pin").await;

    let status = ctx.api.post_status(&ctx.alice_token, "pin me", "public").await;
    let id = status["id"].as_str().unwrap();

    let pin_resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/pin"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(pin_resp.status(), StatusCode::OK);
    let pinned: Value = pin_resp.json().await.unwrap();
    assert_eq!(pinned["pinned"].as_bool(), Some(true));

    let unpin_resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/unpin"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(unpin_resp.status(), StatusCode::OK);
    let unpinned: Value = unpin_resp.json().await.unwrap();
    assert_eq!(unpinned["pinned"].as_bool(), Some(false));
}

/// Pinning a private (own) status succeeds.
#[tokio::test]
async fn test_pin_private_status() {
    let ctx = TestContext::new("pin-private").await;

    let status = ctx.api.post_status(&ctx.alice_token, "pin my private post", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/pin"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["pinned"].as_bool(), Some(true));
}

/// Pinning another user's status returns 422.
#[tokio::test]
async fn test_pin_other_users_status_returns_422() {
    let ctx = TestContext::new("pin-other").await;

    let status = ctx.api.post_status(&ctx.bob_token, "bob's pinnable post", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/pin"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// Pinning a nonexistent status returns 404.
#[tokio::test]
async fn test_pin_nonexistent_status_returns_404() {
    let ctx = TestContext::new("pin-404").await;

    let resp = ctx.api.post_json(
        "/api/v1/statuses/999999999/pin",
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// Pinning without auth returns 401.
#[tokio::test]
async fn test_pin_requires_auth() {
    let ctx = TestContext::new("pin-401").await;

    let status = ctx.api.post_status(&ctx.alice_token, "pin auth test", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/pin"),
        None,
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Unpinning a status that is not pinned returns 200 (idempotent).
#[tokio::test]
async fn test_unpin_not_pinned_is_200() {
    let ctx = TestContext::new("unpin-noop").await;

    let status = ctx.api.post_status(&ctx.alice_token, "never pinned", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/unpin"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["pinned"].as_bool(), Some(false));
}

/// Unpinning a nonexistent status returns 404.
#[tokio::test]
async fn test_unpin_nonexistent_status_returns_404() {
    let ctx = TestContext::new("unpin-404").await;

    let resp = ctx.api.post_json(
        "/api/v1/statuses/999999999/unpin",
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// Unpinning without auth returns 401.
#[tokio::test]
async fn test_unpin_requires_auth() {
    let ctx = TestContext::new("unpin-401").await;

    let status = ctx.api.post_status(&ctx.alice_token, "unpin auth test", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/unpin"),
        None,
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── favourited_by / reblogged_by ──────────────────────────────────────────────

/// GET /api/v1/statuses/:id/favourited_by on a private status (unauthenticated) → 404.
#[tokio::test]
async fn test_favourited_by_private_status_unauthenticated_is_404() {
    let ctx = TestContext::new("fav-by-prv").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice private favby", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.get(&format!("/api/v1/statuses/{id}/favourited_by"), None).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// GET /api/v1/statuses/:id/favourited_by excludes accounts blocked by the viewer.
#[tokio::test]
async fn test_favourited_by_excludes_blocked_accounts() {
    let ctx = TestContext::new("fav-by-block").await;

    let status = ctx.api.post_status(&ctx.alice_token, "fav-by block test", "public").await;
    let id = status["id"].as_str().unwrap();

    // Bob favourites Alice's status.
    ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/favourite"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;

    // Alice blocks Bob.
    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/block", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    // Alice views favourited_by — Bob should not appear.
    let list: Vec<Value> = ctx.api.get(
        &format!("/api/v1/statuses/{id}/favourited_by"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    assert!(
        !list.iter().any(|a| a["id"].as_str() == Some(ctx.bob_id.as_str())),
        "blocked account appeared in favourited_by",
    );
}

/// GET /api/v1/statuses/:id/favourited_by lists accounts that favourited.
#[tokio::test]
async fn test_favourited_by_list() {
    let ctx = TestContext::new("fav-by").await;

    let status = ctx.api.post_status(&ctx.alice_token, "fav me bob", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/favourite"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;

    let resp = ctx.api.get(
        &format!("/api/v1/statuses/{id}/favourited_by"),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(list.iter().any(|a| a["id"].as_str() == Some(ctx.bob_id.as_str())));
}

/// GET /api/v1/statuses/:id/reblogged_by on a private status (unauthenticated) → 404.
#[tokio::test]
async fn test_reblogged_by_private_status_unauthenticated_is_404() {
    let ctx = TestContext::new("rb-by-prv").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice private rbby", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.get(&format!("/api/v1/statuses/{id}/reblogged_by"), None).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// GET /api/v1/statuses/:id/reblogged_by excludes accounts blocked by the viewer.
#[tokio::test]
async fn test_reblogged_by_excludes_blocked_accounts() {
    let ctx = TestContext::new("rb-by-block").await;

    let status = ctx.api.post_status(&ctx.alice_token, "rb-by block test", "public").await;
    let id = status["id"].as_str().unwrap();

    // Bob reblogs Alice's status.
    ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/reblog"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;

    // Alice blocks Bob.
    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/block", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    // Alice views reblogged_by — Bob should not appear.
    let list: Vec<Value> = ctx.api.get(
        &format!("/api/v1/statuses/{id}/reblogged_by"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    assert!(
        !list.iter().any(|a| a["id"].as_str() == Some(ctx.bob_id.as_str())),
        "blocked account appeared in reblogged_by",
    );
}

/// GET /api/v1/statuses/:id/reblogged_by lists accounts that reblogged.
#[tokio::test]
async fn test_reblogged_by_list() {
    let ctx = TestContext::new("rb-by").await;

    let status = ctx.api.post_status(&ctx.alice_token, "reblog me bob", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/reblog"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;

    let resp = ctx.api.get(
        &format!("/api/v1/statuses/{id}/reblogged_by"),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(list.iter().any(|a| a["id"].as_str() == Some(ctx.bob_id.as_str())));
}

/// Unreblog removes the reblog from Bob's statuses.
#[tokio::test]
async fn test_unreblog() {
    let ctx = TestContext::new("unreblog").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice unreblog test", "public").await;
    let id = status["id"].as_str().unwrap();

    let rb: Value = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/reblog"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await.json().await.unwrap();
    let rb_id = rb["id"].as_str().unwrap();

    let unrb_resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/unreblog"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(unrb_resp.status(), StatusCode::OK);

    let get_resp = ctx.api.get(&format!("/api/v1/statuses/{rb_id}"), Some(&ctx.bob_token)).await;
    assert_eq!(get_resp.status(), StatusCode::NOT_FOUND, "reblog should be gone after unreblog");
}

/// Unreblgging a status that was never reblogged returns 200 (idempotent).
#[tokio::test]
async fn test_unreblog_not_reblogged_is_200() {
    let ctx = TestContext::new("unreblog-idem").await;

    let status = ctx.api.post_status(&ctx.alice_token, "never reblogged post", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/unreblog"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK, "unreblog of never-reblogged status should return 200");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["reblogged"].as_bool(), Some(false));
}

// ── favourites list ───────────────────────────────────────────────────────────

/// GET /api/v1/favourites returns statuses the user has favourited.
#[tokio::test]
async fn test_favourites_list() {
    let ctx = TestContext::new("favs-list").await;

    let status = ctx.api.post_status(&ctx.bob_token, "bob's faveable post", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/favourite"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    let resp = ctx.api.get("/api/v1/favourites", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(list.iter().any(|s| s["id"].as_str() == Some(id)));
}

// ── status mute/unmute ────────────────────────────────────────────────────────

/// POST /api/v1/statuses/:id/mute → muted=true; /unmute → muted=false.
#[tokio::test]
async fn test_mute_and_unmute_status() {
    let ctx = TestContext::new("status-mute").await;

    let status = ctx.api.post_status(&ctx.alice_token, "muteable post", "public").await;
    let id = status["id"].as_str().unwrap();

    let mute_resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/mute"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(mute_resp.status(), StatusCode::OK);
    let muted: Value = mute_resp.json().await.unwrap();
    assert_eq!(muted["muted"].as_bool(), Some(true));

    let unmute_resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/unmute"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(unmute_resp.status(), StatusCode::OK);
    let unmuted: Value = unmute_resp.json().await.unwrap();
    assert_eq!(unmuted["muted"].as_bool(), Some(false));
}

// ── polls ──────────────────────────────────────────────────────────────────────

/// POST /api/v1/statuses with a poll returns the poll options in the response.
#[tokio::test]
async fn test_create_status_with_poll() {
    let ctx = TestContext::new("poll-create").await;

    let resp = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "Which do you prefer?",
            "visibility": "public",
            "poll": {
                "options": ["Option A", "Option B"],
                "expires_in": 86400
            }
        }),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let status: Value = resp.json().await.unwrap();
    let poll = &status["poll"];
    assert!(poll.is_object(), "poll field missing");
    assert!(poll["id"].as_str().is_some());
    let options = poll["options"].as_array().unwrap();
    assert_eq!(options.len(), 2);
    assert_eq!(options[0]["title"].as_str(), Some("Option A"));
    assert_eq!(options[1]["title"].as_str(), Some("Option B"));
    assert_eq!(poll["expired"].as_bool(), Some(false));
}

/// GET /api/v1/polls/:id returns poll details.
#[tokio::test]
async fn test_get_poll() {
    let ctx = TestContext::new("poll-get").await;

    let status: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "Which?",
            "visibility": "public",
            "poll": {"options": ["Yes", "No"], "expires_in": 86400}
        }),
    ).await.json().await.unwrap();
    let poll_id = status["poll"]["id"].as_str().unwrap();

    let resp = ctx.api.get(&format!("/api/v1/polls/{poll_id}"), None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let poll: Value = resp.json().await.unwrap();
    assert_eq!(poll["id"].as_str(), Some(poll_id));
}

/// POST /api/v1/polls/:id/votes records the vote and returns voted=true.
#[tokio::test]
async fn test_vote_poll() {
    let ctx = TestContext::new("poll-vote").await;

    let status: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "Vote!",
            "visibility": "public",
            "poll": {"options": ["A", "B", "C"], "expires_in": 86400}
        }),
    ).await.json().await.unwrap();
    let poll_id = status["poll"]["id"].as_str().unwrap();

    let vote_resp = ctx.api.post_json(
        &format!("/api/v1/polls/{poll_id}/votes"),
        Some(&ctx.bob_token),
        &json!({"choices": [1]}),
    ).await;
    assert_eq!(vote_resp.status(), StatusCode::OK);
    let poll: Value = vote_resp.json().await.unwrap();
    assert_eq!(poll["voted"].as_bool(), Some(true));
    assert_eq!(poll["own_votes"].as_array().unwrap(), &[json!(1)]);
}

/// Voting twice on the same poll returns 422.
#[tokio::test]
async fn test_vote_poll_twice_returns_422() {
    let ctx = TestContext::new("poll-vote2").await;

    let status: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "Vote twice?",
            "visibility": "public",
            "poll": {"options": ["A", "B"], "expires_in": 86400}
        }),
    ).await.json().await.unwrap();
    let poll_id = status["poll"]["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/polls/{poll_id}/votes"),
        Some(&ctx.bob_token),
        &json!({"choices": [0]}),
    ).await;

    let second = ctx.api.post_json(
        &format!("/api/v1/polls/{poll_id}/votes"),
        Some(&ctx.bob_token),
        &json!({"choices": [1]}),
    ).await;
    assert_eq!(second.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// Voting on a single-choice poll with multiple choices returns 422.
#[tokio::test]
async fn test_vote_poll_multiple_choices_on_single_poll_returns_422() {
    let ctx = TestContext::new("poll-multi-fail").await;

    let status: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "Single choice!",
            "visibility": "public",
            "poll": {"options": ["A", "B", "C"], "expires_in": 86400, "multiple": false}
        }),
    ).await.json().await.unwrap();
    let poll_id = status["poll"]["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/polls/{poll_id}/votes"),
        Some(&ctx.bob_token),
        &json!({"choices": [0, 1]}),
    ).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// Voting with an out-of-bounds choice index returns 422.
#[tokio::test]
async fn test_vote_poll_invalid_choice_index_returns_422() {
    let ctx = TestContext::new("poll-oob").await;

    let status: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "Two choices",
            "visibility": "public",
            "poll": {"options": ["A", "B"], "expires_in": 86400}
        }),
    ).await.json().await.unwrap();
    let poll_id = status["poll"]["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/polls/{poll_id}/votes"),
        Some(&ctx.bob_token),
        &json!({"choices": [99]}),
    ).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// GET /api/v1/polls/:id for unknown id returns 404.
#[tokio::test]
async fn test_get_poll_not_found() {
    let ctx = TestContext::new("poll-404").await;

    let resp = ctx.api.get(
        "/api/v1/polls/00000000-0000-0000-0000-000000000000",
        None,
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// Voting on a poll updates per-option votes_count and voters_count.
#[tokio::test]
async fn test_poll_vote_counts() {
    let ctx = TestContext::new("poll-counts").await;

    let status: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "Counts poll!",
            "visibility": "public",
            "poll": {"options": ["X", "Y", "Z"], "expires_in": 86400}
        }),
    ).await.json().await.unwrap();
    let poll_id = status["poll"]["id"].as_str().unwrap();

    // Bob votes for option Y (index 1).
    ctx.api.post_json(
        &format!("/api/v1/polls/{poll_id}/votes"),
        Some(&ctx.bob_token),
        &json!({"choices": [1]}),
    ).await;

    let poll: Value = ctx.api.get(
        &format!("/api/v1/polls/{poll_id}"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();

    assert_eq!(poll["votes_count"].as_i64(), Some(1), "votes_count should be 1");
    assert_eq!(poll["voters_count"].as_i64(), Some(1), "voters_count should be 1");

    let options = poll["options"].as_array().unwrap();
    assert_eq!(options[0]["votes_count"].as_i64(), Some(0), "option X should have 0 votes");
    assert_eq!(options[1]["votes_count"].as_i64(), Some(1), "option Y should have 1 vote");
    assert_eq!(options[2]["votes_count"].as_i64(), Some(0), "option Z should have 0 votes");
}

/// A multiple-choice poll allows selecting several options.
#[tokio::test]
async fn test_vote_poll_multiple_choice_allowed() {
    let ctx = TestContext::new("poll-multi-ok").await;

    let status: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "Multi choice!",
            "visibility": "public",
            "poll": {"options": ["A", "B", "C"], "expires_in": 86400, "multiple": true}
        }),
    ).await.json().await.unwrap();
    let poll_id = status["poll"]["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/polls/{poll_id}/votes"),
        Some(&ctx.bob_token),
        &json!({"choices": [0, 2]}),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let poll: Value = resp.json().await.unwrap();
    assert_eq!(poll["voted"].as_bool(), Some(true));
    let own_votes = poll["own_votes"].as_array().unwrap();
    assert!(own_votes.contains(&json!(0)));
    assert!(own_votes.contains(&json!(2)));
}

/// POST /api/v1/statuses with a poll with only 1 option returns 422.
#[tokio::test]
async fn test_poll_with_one_option_returns_422() {
    let ctx = TestContext::new("poll-1opt").await;

    let resp = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "single option poll",
            "visibility": "public",
            "poll": {"options": ["Only one"], "expires_in": 86400}
        }),
    ).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// POST /api/v1/statuses with a poll with 5 options returns 422.
#[tokio::test]
async fn test_poll_with_five_options_returns_422() {
    let ctx = TestContext::new("poll-5opt").await;

    let resp = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "five option poll",
            "visibility": "public",
            "poll": {"options": ["A", "B", "C", "D", "E"], "expires_in": 86400}
        }),
    ).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// POST /api/v1/statuses with a poll with a blank option returns 422.
#[tokio::test]
async fn test_poll_with_blank_option_returns_422() {
    let ctx = TestContext::new("poll-blank").await;

    let resp = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "blank option poll",
            "visibility": "public",
            "poll": {"options": ["Valid", ""], "expires_in": 86400}
        }),
    ).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// ── default posting visibility ────────────────────────────────────────────────

/// When the user has a default_privacy setting and no visibility is given, that default is used.
#[tokio::test]
async fn test_post_status_uses_default_privacy() {
    let ctx = TestContext::new("default-privacy").await;

    // Set default visibility to "unlisted" via update_credentials.
    let form = reqwest::multipart::Form::new()
        .text("source[privacy]", "unlisted");
    ctx.api.http
        .patch(ctx.api.url("/api/v1/accounts/update_credentials"))
        .header("host", &ctx.api.host)
        .bearer_auth(&ctx.alice_token)
        .multipart(form)
        .send()
        .await
        .unwrap();

    // Post a status without specifying visibility.
    let status: Value = ctx.api.http
        .post(ctx.api.url("/api/v1/statuses"))
        .header("host", &ctx.api.host)
        .bearer_auth(&ctx.alice_token)
        .json(&json!({"status": "default visibility post"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(
        status["visibility"].as_str(),
        Some("unlisted"),
        "status should use the account's default visibility",
    );
}

/// When the user has default_sensitive=true and no sensitive flag is given, status is sensitive.
#[tokio::test]
async fn test_post_status_uses_default_sensitive() {
    let ctx = TestContext::new("default-sensitive").await;

    // Set default sensitive via update_credentials.
    let form = reqwest::multipart::Form::new()
        .text("source[sensitive]", "true");
    ctx.api.http
        .patch(ctx.api.url("/api/v1/accounts/update_credentials"))
        .header("host", &ctx.api.host)
        .bearer_auth(&ctx.alice_token)
        .multipart(form)
        .send()
        .await
        .unwrap();

    // Post a status without specifying sensitive.
    let status: Value = ctx.api.http
        .post(ctx.api.url("/api/v1/statuses"))
        .header("host", &ctx.api.host)
        .bearer_auth(&ctx.alice_token)
        .json(&json!({"status": "default sensitive post", "visibility": "public"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(
        status["sensitive"].as_bool(),
        Some(true),
        "status should inherit account's default sensitive setting",
    );
}

// ── status sensitive and language ─────────────────────────────────────────────

/// Status with sensitive=true has sensitive=true in the response.
#[tokio::test]
async fn test_status_sensitive_flag() {
    let ctx = TestContext::new("sensitive").await;

    let resp = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "CW content",
            "visibility": "public",
            "sensitive": true
        }),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let status: Value = resp.json().await.unwrap();
    assert_eq!(status["sensitive"].as_bool(), Some(true));
}

/// Status with language=es has language=es in the response.
#[tokio::test]
async fn test_status_language_field() {
    let ctx = TestContext::new("lang").await;

    let resp = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "En español",
            "visibility": "public",
            "language": "es"
        }),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let status: Value = resp.json().await.unwrap();
    assert_eq!(status["language"].as_str(), Some("es"));
}

// ── scheduled statuses ────────────────────────────────────────────────────────

// ── batch GET statuses ────────────────────────────────────────────────────────

/// GET /api/v1/statuses?id[]=... returns the requested statuses.
#[tokio::test]
async fn test_batch_get_statuses() {
    let ctx = TestContext::new("batch-get").await;

    let s1 = ctx.api.post_status(&ctx.alice_token, "batch status 1", "public").await;
    let s2 = ctx.api.post_status(&ctx.alice_token, "batch status 2", "public").await;
    let id1 = s1["id"].as_str().unwrap();
    let id2 = s2["id"].as_str().unwrap();

    let resp = ctx.api.get(
        &format!("/api/v1/statuses?id[]={id1}&id[]={id2}"),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let statuses: Vec<Value> = resp.json().await.unwrap();
    let ids: Vec<&str> = statuses.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(ids.contains(&id1), "s1 missing from batch result");
    assert!(ids.contains(&id2), "s2 missing from batch result");
}

/// GET /api/v1/statuses?id[]=... skips statuses from blocked accounts.
#[tokio::test]
async fn test_batch_get_statuses_skips_blocked() {
    let ctx = TestContext::new("batch-blocked").await;

    let blocked_status = ctx.api.post_status(&ctx.bob_token, "blocked batch status", "public").await;
    let blocked_id = blocked_status["id"].as_str().unwrap();
    let own_status = ctx.api.post_status(&ctx.alice_token, "own batch status", "public").await;
    let own_id = own_status["id"].as_str().unwrap();

    // Alice blocks Bob.
    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/block", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    let statuses: Vec<Value> = ctx.api.get(
        &format!("/api/v1/statuses?id[]={blocked_id}&id[]={own_id}"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    let ids: Vec<&str> = statuses.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(!ids.contains(&blocked_id), "blocked account's status should be skipped in batch");
    assert!(ids.contains(&own_id), "own status should still appear in batch result");
}

/// GET /api/v1/statuses?id[]=... skips private statuses not visible to viewer.
#[tokio::test]
async fn test_batch_get_statuses_skips_invisible() {
    let ctx = TestContext::new("batch-skip").await;

    let prv = ctx.api.post_status(&ctx.alice_token, "batch private", "private").await;
    let pub_s = ctx.api.post_status(&ctx.alice_token, "batch public", "public").await;
    let prv_id = prv["id"].as_str().unwrap();
    let pub_id = pub_s["id"].as_str().unwrap();

    // Bob is not following Alice, so the private status should be silently skipped.
    let statuses: Vec<Value> = ctx.api.get(
        &format!("/api/v1/statuses?id[]={prv_id}&id[]={pub_id}"),
        Some(&ctx.bob_token),
    ).await.json().await.unwrap();
    let ids: Vec<&str> = statuses.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(!ids.contains(&prv_id), "private status should be silently skipped in batch");
    assert!(ids.contains(&pub_id), "public status should be in batch result");
}

/// GET /api/v1/statuses?id[]=... shows a direct status to the mentioned recipient.
#[tokio::test]
async fn test_batch_get_statuses_shows_direct_to_mentioned() {
    let ctx = TestContext::new("batch-direct-mention").await;

    // Alice sends a DM mentioning Bob.
    let dm: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "@bob direct batch test",
            "visibility": "direct"
        }),
    ).await.json().await.unwrap();
    let dm_id = dm["id"].as_str().unwrap();

    // Bob (the recipient) can see the DM in a batch request.
    let statuses: Vec<Value> = ctx.api.get(
        &format!("/api/v1/statuses?id[]={dm_id}"),
        Some(&ctx.bob_token),
    ).await.json().await.unwrap();
    let ids: Vec<&str> = statuses.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(ids.contains(&dm_id), "mentioned recipient should see direct status in batch");
}

// ── scheduled statuses ────────────────────────────────────────────────────────

/// POST /api/v1/statuses with scheduled_at returns a scheduled status (201).
#[tokio::test]
async fn test_create_scheduled_status() {
    let ctx = TestContext::new("sched-create").await;

    let resp = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "Scheduled post",
            "visibility": "public",
            "scheduled_at": "2099-01-01T00:00:00Z"
        }),
    ).await;
    assert_eq!(resp.status().as_u16(), 201, "expected 201 for scheduled status");
    let body: Value = resp.json().await.unwrap();
    assert!(body["scheduled_at"].as_str().is_some(), "scheduled_at field missing");
    assert!(body["id"].as_str().is_some(), "id field missing");
    assert!(body["params"].is_object(), "params field missing");
}

/// Posting a status increments statuses_count; deleting decrements it.
#[tokio::test]
async fn test_statuses_count_increments_and_decrements() {
    let ctx = TestContext::new("stat-count").await;

    let before: Value = ctx.api.get(&format!("/api/v1/accounts/{}", ctx.alice_id), None)
        .await.json().await.unwrap();
    let count_before = before["statuses_count"].as_i64().unwrap_or(0);

    let status = ctx.api.post_status(&ctx.alice_token, "counting post", "public").await;
    let id = status["id"].as_str().unwrap();

    let after_post: Value = ctx.api.get(&format!("/api/v1/accounts/{}", ctx.alice_id), None)
        .await.json().await.unwrap();
    assert_eq!(
        after_post["statuses_count"].as_i64().unwrap_or(0),
        count_before + 1,
        "statuses_count should increment after posting",
    );

    ctx.api.delete(&format!("/api/v1/statuses/{id}"), &ctx.alice_token).await;

    let after_del: Value = ctx.api.get(&format!("/api/v1/accounts/{}", ctx.alice_id), None)
        .await.json().await.unwrap();
    assert_eq!(
        after_del["statuses_count"].as_i64().unwrap_or(0),
        count_before,
        "statuses_count should decrement after deleting",
    );
}

/// GET /api/v1/scheduled_statuses lists previously scheduled statuses.
#[tokio::test]
async fn test_list_scheduled_statuses() {
    let ctx = TestContext::new("sched-list").await;

    ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "Scheduled for listing",
            "visibility": "public",
            "scheduled_at": "2099-06-01T00:00:00Z"
        }),
    ).await;

    let list: Vec<Value> = ctx.api.get("/api/v1/scheduled_statuses", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(!list.is_empty(), "scheduled status not in list");
    assert!(list[0]["scheduled_at"].as_str().is_some());
}

/// GET /api/v1/scheduled_statuses/:id returns a single scheduled status.
#[tokio::test]
async fn test_get_scheduled_status() {
    let ctx = TestContext::new("sched-get").await;

    let created: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "get scheduled",
            "visibility": "public",
            "scheduled_at": "2099-07-01T00:00:00Z"
        }),
    ).await.json().await.unwrap();
    let id = created["id"].as_str().unwrap();

    let resp = ctx.api.get(
        &format!("/api/v1/scheduled_statuses/{id}"),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["id"].as_str(), Some(id));
    assert!(body["scheduled_at"].as_str().is_some());
}

/// GET /api/v1/scheduled_statuses/:id returns 404 for another user's scheduled status.
#[tokio::test]
async fn test_get_scheduled_status_other_user_is_404() {
    let ctx = TestContext::new("sched-get-other").await;

    let created: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "alice scheduled",
            "visibility": "public",
            "scheduled_at": "2099-08-01T00:00:00Z"
        }),
    ).await.json().await.unwrap();
    let id = created["id"].as_str().unwrap();

    let resp = ctx.api.get(
        &format!("/api/v1/scheduled_statuses/{id}"),
        Some(&ctx.bob_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// PUT /api/v1/scheduled_statuses/:id updates the scheduled_at time.
#[tokio::test]
async fn test_update_scheduled_status() {
    let ctx = TestContext::new("sched-update").await;

    let created: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "update scheduled",
            "visibility": "public",
            "scheduled_at": "2099-09-01T00:00:00Z"
        }),
    ).await.json().await.unwrap();
    let id = created["id"].as_str().unwrap();

    let resp = ctx.api.put_json(
        &format!("/api/v1/scheduled_statuses/{id}"),
        Some(&ctx.alice_token),
        &json!({"scheduled_at": "2099-10-01T00:00:00Z"}),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(body["scheduled_at"].as_str().unwrap().contains("2099-10"), "scheduled_at should be updated");
}

/// DELETE /api/v1/scheduled_statuses/:id cancels a scheduled status.
#[tokio::test]
async fn test_delete_scheduled_status() {
    let ctx = TestContext::new("sched-del").await;

    let created: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "delete scheduled",
            "visibility": "public",
            "scheduled_at": "2099-11-01T00:00:00Z"
        }),
    ).await.json().await.unwrap();
    let id = created["id"].as_str().unwrap();

    let del_resp = ctx.api.delete(
        &format!("/api/v1/scheduled_statuses/{id}"),
        &ctx.alice_token,
    ).await;
    assert_eq!(del_resp.status(), StatusCode::OK);

    let not_found = ctx.api.get(
        &format!("/api/v1/scheduled_statuses/{id}"),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(not_found.status(), StatusCode::NOT_FOUND);
}

// ── mentions and tags in status response ──────────────────────────────────────

/// A status with @username mention includes the mentioned account in the mentions array.
#[tokio::test]
async fn test_status_mentions_field() {
    let ctx = TestContext::new("status-mentions").await;

    let status = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": &format!("@bob hello there"),
            "visibility": "public"
        }),
    ).await.json::<Value>().await.unwrap();

    let mentions = status["mentions"].as_array().expect("mentions field missing");
    assert!(
        mentions.iter().any(|m| m["username"].as_str() == Some("bob")),
        "bob not in mentions: {mentions:?}",
    );
}

/// A status with #hashtag includes the tag in the tags array.
#[tokio::test]
async fn test_status_tags_field() {
    let ctx = TestContext::new("status-tags").await;

    let status = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "testing #uniquetag5555 here",
            "visibility": "public"
        }),
    ).await.json::<Value>().await.unwrap();

    let tags = status["tags"].as_array().expect("tags field missing");
    assert!(
        tags.iter().any(|t| t["name"].as_str() == Some("uniquetag5555")),
        "uniquetag5555 not in tags: {tags:?}",
    );
    // Each tag should have a url field.
    for tag in tags {
        assert!(tag["url"].as_str().is_some(), "tag missing url: {tag:?}");
    }
}

/// GET /api/v1/notifications?since_id=X excludes the anchor and older notifications.
#[tokio::test]
async fn test_notifications_since_id_excludes_anchor() {
    let ctx = TestContext::new("notif-since-anchor").await;

    // First notification: alice follows bob.
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let first_notifs: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(!first_notifs.is_empty());
    let first_id = first_notifs.last().unwrap()["id"].as_str().unwrap().to_string();

    // Second notification: alice favourites bob's status.
    let status = ctx.api.post_status(&ctx.bob_token, "since-anchor target", "public").await;
    let sid = status["id"].as_str().unwrap();
    ctx.api.post_json(
        &format!("/api/v1/statuses/{sid}/favourite"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    // since_id=first_id excludes first_id and anything older.
    let since_notifs: Vec<Value> = ctx.api.get(
        &format!("/api/v1/notifications?since_id={first_id}"),
        Some(&ctx.bob_token),
    ).await.json().await.unwrap();

    assert!(
        !since_notifs.iter().any(|n| n["id"].as_str() == Some(&first_id)),
        "since_id anchor itself should be excluded",
    );
    assert!(
        since_notifs.iter().any(|n| n["type"].as_str() == Some("favourite")),
        "favourite notification should appear after since_id anchor",
    );
}

/// Reply to a non-existent status returns 422.
#[tokio::test]
async fn test_reply_to_nonexistent_status_returns_422() {
    let ctx = TestContext::new("reply-nonexist").await;

    let resp = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "this is a reply",
            "visibility": "public",
            "in_reply_to_id": "999999999999"
        }),
    ).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "replying to non-existent status should return 422",
    );
}

/// Status with no text, no media, and no poll returns 422.
#[tokio::test]
async fn test_post_status_empty_returns_422() {
    let ctx = TestContext::new("status-empty").await;

    let resp = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({"status": "", "visibility": "public"}),
    ).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "posting empty status should return 422",
    );
}

/// replies_count increments when someone replies and decrements when the reply is deleted.
#[tokio::test]
async fn test_replies_count_increments_and_decrements() {
    let ctx = TestContext::new("replies-count").await;

    let parent = ctx.api.post_status(&ctx.alice_token, "parent post", "public").await;
    let parent_id = parent["id"].as_str().unwrap();
    let count_before = parent["replies_count"].as_i64().unwrap_or(0);

    // Bob replies to Alice's status.
    let reply: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.bob_token),
        &json!({
            "status": "replying here",
            "visibility": "public",
            "in_reply_to_id": parent_id
        }),
    ).await.json().await.unwrap();
    let reply_id = reply["id"].as_str().unwrap();

    let after_reply: Value = ctx.api.get(
        &format!("/api/v1/statuses/{parent_id}"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    assert_eq!(
        after_reply["replies_count"].as_i64().unwrap_or(0),
        count_before + 1,
        "replies_count should increment after a reply",
    );

    // Delete the reply.
    ctx.api.delete(&format!("/api/v1/statuses/{reply_id}"), &ctx.bob_token).await;

    let after_delete: Value = ctx.api.get(
        &format!("/api/v1/statuses/{parent_id}"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    assert_eq!(
        after_delete["replies_count"].as_i64().unwrap_or(0),
        count_before,
        "replies_count should decrement after deleting the reply",
    );
}

/// Status exceeding 500 characters returns 422.
#[tokio::test]
async fn test_post_status_over_char_limit_returns_422() {
    let ctx = TestContext::new("status-toolong").await;

    let long_text = "a".repeat(501);
    let resp = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({"status": long_text, "visibility": "public"}),
    ).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "status over 500 chars should return 422",
    );
}

/// Status of exactly 500 characters succeeds.
#[tokio::test]
async fn test_post_status_at_char_limit_succeeds() {
    let ctx = TestContext::new("status-at-limit").await;

    let exact_text = "a".repeat(500);
    let resp = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({"status": exact_text, "visibility": "public"}),
    ).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "status of exactly 500 chars should succeed",
    );
}

/// Status response has a filtered field that is an array (not null).
#[tokio::test]
async fn test_status_filtered_field_is_array() {
    let ctx = TestContext::new("status-filtered").await;

    let status = ctx.api.post_status(&ctx.alice_token, "check filtered field", "public").await;
    assert!(
        status["filtered"].is_array(),
        "filtered field should be an array, got: {:?}",
        status["filtered"],
    );
}

/// Pinning a reblog returns 422.
#[tokio::test]
async fn test_pin_reblog_returns_422() {
    let ctx = TestContext::new("pin-reblog-422").await;

    let original = ctx.api.post_status(&ctx.alice_token, "original to boost and pin", "public").await;
    let original_id = original["id"].as_str().unwrap();

    let rb: Value = ctx.api.post_json(
        &format!("/api/v1/statuses/{original_id}/reblog"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await.json().await.unwrap();
    let reblog_id = rb["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{reblog_id}/pin"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY, "pinning a reblog should return 422");
}

/// Pinning more than 5 statuses returns 422.
#[tokio::test]
async fn test_pin_limit_is_five() {
    let ctx = TestContext::new("pin-limit").await;

    let mut ids = Vec::new();
    for i in 0..5 {
        let s = ctx.api.post_status(&ctx.alice_token, &format!("pin #{i}"), "public").await;
        ids.push(s["id"].as_str().unwrap().to_string());
    }

    // Pin all 5
    for id in &ids {
        let resp = ctx.api.post_json(
            &format!("/api/v1/statuses/{id}/pin"),
            Some(&ctx.alice_token),
            &json!({}),
        ).await;
        assert_eq!(resp.status(), StatusCode::OK, "pinning status {id} should succeed");
    }

    // Pin a 6th — should fail
    let s6 = ctx.api.post_status(&ctx.alice_token, "pin #5 (over limit)", "public").await;
    let id6 = s6["id"].as_str().unwrap();
    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id6}/pin"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "pinning a 6th status should return 422",
    );
}

/// Private statuses in a thread context are hidden from non-followers.
#[tokio::test]
async fn test_context_hides_private_status_from_non_follower() {
    let ctx = TestContext::new("ctx-prv-nonfollower").await;

    // Alice posts a public status
    let public_s = ctx.api.post_status(&ctx.alice_token, "public root", "public").await;
    let public_id = public_s["id"].as_str().unwrap();

    // Alice replies privately
    let private_reply: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "private reply",
            "visibility": "private",
            "in_reply_to_id": public_id,
        }),
    ).await.json().await.unwrap();
    let private_id = private_reply["id"].as_str().unwrap();

    // Bob (not a follower) requests context of the public root
    let context: Value = ctx.api.get(
        &format!("/api/v1/statuses/{public_id}/context"),
        Some(&ctx.bob_token),
    ).await.json().await.unwrap();

    let descendant_ids: Vec<&str> = context["descendants"]
        .as_array().unwrap()
        .iter()
        .filter_map(|s| s["id"].as_str())
        .collect();

    assert!(
        !descendant_ids.contains(&private_id),
        "private reply should not appear in context for non-follower",
    );
}

/// Private statuses in a thread context are visible to followers.
#[tokio::test]
async fn test_context_shows_private_status_to_follower() {
    let ctx = TestContext::new("ctx-prv-follower").await;

    // Bob follows Alice
    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/follow", ctx.alice_id),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;

    // Alice posts a public root
    let public_s = ctx.api.post_status(&ctx.alice_token, "public root", "public").await;
    let public_id = public_s["id"].as_str().unwrap();

    // Alice replies privately
    let private_reply: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "private reply",
            "visibility": "private",
            "in_reply_to_id": public_id,
        }),
    ).await.json().await.unwrap();
    let private_id = private_reply["id"].as_str().unwrap();

    // Bob (a follower) requests context
    let context: Value = ctx.api.get(
        &format!("/api/v1/statuses/{public_id}/context"),
        Some(&ctx.bob_token),
    ).await.json().await.unwrap();

    let descendant_ids: Vec<&str> = context["descendants"]
        .as_array().unwrap()
        .iter()
        .filter_map(|s| s["id"].as_str())
        .collect();

    assert!(
        descendant_ids.contains(&private_id),
        "private reply should appear in context for a follower",
    );
}

/// Unauthenticated context request hides private statuses.
#[tokio::test]
async fn test_context_hides_private_status_unauthenticated() {
    let ctx = TestContext::new("ctx-prv-unauth").await;

    let public_s = ctx.api.post_status(&ctx.alice_token, "public root", "public").await;
    let public_id = public_s["id"].as_str().unwrap();

    let private_reply: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({
            "status": "private reply",
            "visibility": "private",
            "in_reply_to_id": public_id,
        }),
    ).await.json().await.unwrap();
    let private_id = private_reply["id"].as_str().unwrap();

    let context: Value = ctx.api.get(
        &format!("/api/v1/statuses/{public_id}/context"),
        None,
    ).await.json().await.unwrap();

    let descendant_ids: Vec<&str> = context["descendants"]
        .as_array().unwrap()
        .iter()
        .filter_map(|s| s["id"].as_str())
        .collect();

    assert!(
        !descendant_ids.contains(&private_id),
        "private reply should not appear in context for unauthenticated viewer",
    );
}

/// Boosting a status increments the booster's statuses_count; unboosting decrements it.
#[tokio::test]
async fn test_reblog_increments_booster_statuses_count() {
    let ctx = TestContext::new("boost-stat-count").await;

    let status = ctx.api.post_status(&ctx.alice_token, "to boost for count", "public").await;
    let id = status["id"].as_str().unwrap();

    let before: Value = ctx.api.get(&format!("/api/v1/accounts/{}", ctx.bob_id), None)
        .await.json().await.unwrap();
    let count_before = before["statuses_count"].as_i64().unwrap_or(0);

    ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/reblog"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;

    let after_boost: Value = ctx.api.get(&format!("/api/v1/accounts/{}", ctx.bob_id), None)
        .await.json().await.unwrap();
    assert_eq!(
        after_boost["statuses_count"].as_i64().unwrap_or(0),
        count_before + 1,
        "statuses_count should increment when boosting",
    );

    ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/unreblog"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;

    let after_unboost: Value = ctx.api.get(&format!("/api/v1/accounts/{}", ctx.bob_id), None)
        .await.json().await.unwrap();
    assert_eq!(
        after_unboost["statuses_count"].as_i64().unwrap_or(0),
        count_before,
        "statuses_count should decrement when unboosting",
    );
}

/// A "hide" filter with context=thread removes matching statuses from /context descendants.
#[tokio::test]
async fn test_context_thread_filter_hides_matching_descendant() {
    let ctx = TestContext::new("ctx-thread-filter").await;

    // Alice creates a "thread" hide filter for "spamword".
    ctx.api.post_json(
        "/api/v2/filters",
        Some(&ctx.alice_token),
        &json!({
            "title": "Thread spam filter",
            "context": ["thread"],
            "filter_action": "hide",
            "keywords_attributes": [{"keyword": "spamword", "whole_word": false}]
        }),
    ).await;

    // Bob posts a public root.
    let root = ctx.api.post_status(&ctx.bob_token, "root post", "public").await;
    let root_id = root["id"].as_str().unwrap();

    // Bob replies with a clean reply.
    let clean_reply: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.bob_token),
        &json!({"status": "clean reply", "in_reply_to_id": root_id}),
    ).await.json().await.unwrap();
    let clean_id = clean_reply["id"].as_str().unwrap();

    // Bob replies with a spammy reply.
    let spam_reply: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.bob_token),
        &json!({"status": "this contains spamword", "in_reply_to_id": root_id}),
    ).await.json().await.unwrap();
    let spam_id = spam_reply["id"].as_str().unwrap();

    // Alice fetches context — spam reply should be hidden.
    let context: Value = ctx.api.get(
        &format!("/api/v1/statuses/{root_id}/context"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();

    let desc_ids: Vec<&str> = context["descendants"]
        .as_array().unwrap()
        .iter()
        .filter_map(|s| s["id"].as_str())
        .collect();

    assert!(
        desc_ids.contains(&clean_id),
        "clean reply should still appear in thread context",
    );
    assert!(
        !desc_ids.contains(&spam_id),
        "spam reply should be hidden by thread filter",
    );
}

/// Thread context hides statuses from blocked accounts.
#[tokio::test]
async fn test_context_hides_blocked_account_statuses() {
    let ctx = TestContext::new("ctx-blocked-acct").await;

    // Alice posts a public root.
    let root = ctx.api.post_status(&ctx.alice_token, "public root for block test", "public").await;
    let root_id = root["id"].as_str().unwrap();

    // Bob replies.
    let bob_reply: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.bob_token),
        &json!({"status": "bob's reply", "in_reply_to_id": root_id}),
    ).await.json().await.unwrap();
    let bob_reply_id = bob_reply["id"].as_str().unwrap();

    // Alice blocks Bob.
    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/block", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    // Alice fetches context — Bob's reply should be hidden.
    let context: Value = ctx.api.get(
        &format!("/api/v1/statuses/{root_id}/context"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();

    let desc_ids: Vec<&str> = context["descendants"]
        .as_array().unwrap()
        .iter()
        .filter_map(|s| s["id"].as_str())
        .collect();

    assert!(
        !desc_ids.contains(&bob_reply_id),
        "blocked account's reply should be hidden in thread context",
    );
}

/// POST /api/v1/statuses with Idempotency-Key is idempotent — same key returns the same status.
#[tokio::test]
async fn test_post_status_idempotency_key() {
    let ctx = TestContext::new("idempotency-key").await;

    let key = "test-idempotency-key-abc123";
    let client = reqwest::Client::new();

    // First request — creates the status.
    let resp1: Value = client
        .post(format!("{}/api/v1/statuses", ctx.api.base_url))
        .header("Host", &ctx.domain)
        .bearer_auth(&ctx.alice_token)
        .header("Idempotency-Key", key)
        .json(&json!({"status": "idempotency test post"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let id1 = resp1["id"].as_str().expect("first response must have id");

    // Second request with the same key — must return the same status, not create a new one.
    let resp2: Value = client
        .post(format!("{}/api/v1/statuses", ctx.api.base_url))
        .header("Host", &ctx.domain)
        .bearer_auth(&ctx.alice_token)
        .header("Idempotency-Key", key)
        .json(&json!({"status": "idempotency test post"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let id2 = resp2["id"].as_str().expect("second response must have id");

    assert_eq!(id1, id2, "idempotent post should return the same status id");
}

/// A "warn" filter in thread context keeps the status but sets the filtered field.
#[tokio::test]
async fn test_context_thread_filter_warn_annotates_status() {
    let ctx = TestContext::new("ctx-thread-warn").await;

    // Alice creates a "thread" warn filter for "warnword".
    ctx.api.post_json(
        "/api/v2/filters",
        Some(&ctx.alice_token),
        &json!({
            "title": "Thread warn filter",
            "context": ["thread"],
            "filter_action": "warn",
            "keywords_attributes": [{"keyword": "warnword", "whole_word": false}]
        }),
    ).await;

    let root = ctx.api.post_status(&ctx.bob_token, "root post for warn", "public").await;
    let root_id = root["id"].as_str().unwrap();

    let warned_reply: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.bob_token),
        &json!({"status": "reply with warnword", "in_reply_to_id": root_id}),
    ).await.json().await.unwrap();
    let warned_id = warned_reply["id"].as_str().unwrap();

    let context: Value = ctx.api.get(
        &format!("/api/v1/statuses/{root_id}/context"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();

    let descendants = context["descendants"].as_array().unwrap();
    let warned_status = descendants.iter().find(|s| s["id"].as_str() == Some(warned_id));

    assert!(warned_status.is_some(), "warned reply should still appear in thread context");
    let filtered = warned_status.unwrap()["filtered"].as_array().unwrap();
    assert!(!filtered.is_empty(), "warned reply should have non-empty filtered array");
    assert_eq!(
        filtered[0]["filter"]["filter_action"].as_str(),
        Some("warn"),
        "filtered entry should reference the warn filter",
    );
}

/// GET /api/v1/statuses/:id/card returns null when no preview card exists.
#[tokio::test]
async fn test_status_card_null_when_no_url() {
    let ctx = TestContext::new("status-card-null").await;

    let status = ctx.api.post_status(&ctx.alice_token, "no links here at all", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.get(&format!("/api/v1/statuses/{id}/card"), None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(body.is_null(), "card should be null when status has no link, got: {body}");
}

/// GET /api/v1/statuses/:id/card returns 404 for a non-existent status.
#[tokio::test]
async fn test_status_card_not_found() {
    let ctx = TestContext::new("status-card-404").await;

    let resp = ctx.api.get("/api/v1/statuses/999999999/card", None).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// GET /api/v1/statuses/:id/card returns 404 for a private status viewed without auth.
#[tokio::test]
async fn test_status_card_private_unauthenticated_is_404() {
    let ctx = TestContext::new("status-card-priv").await;

    let status = ctx.api.post_status(&ctx.alice_token, "private status for card", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.get(&format!("/api/v1/statuses/{id}/card"), None).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// POST /api/v1/statuses/:id/translate returns 503 when translation is disabled.
#[tokio::test]
async fn test_status_translate_returns_503() {
    let ctx = TestContext::new("status-translate").await;

    let status = ctx.api.post_status(&ctx.alice_token, "hola mundo", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/translate"),
        Some(&ctx.alice_token),
        &json!({"lang": "en"}),
    ).await;
    assert_eq!(resp.status().as_u16(), 503, "translate should return 503 when not supported");
}

/// POST /api/v1/statuses response includes the application field for the author.
#[tokio::test]
async fn test_post_status_includes_application_field() {
    let ctx = TestContext::new("status-app-field").await;

    let status = ctx.api.post_status(&ctx.alice_token, "application field test", "public").await;

    // The application field should be present (and have a name).
    let app = &status["application"];
    assert!(
        app.is_object() && app["name"].as_str().is_some(),
        "status application field should be an object with a name, got: {status:?}",
    );
}

/// Deleting the original status cascade-deletes its reblogs.
///
/// Mastodon: when an original post is removed the server also removes all boosts
/// of that post.  After deletion GET /api/v1/statuses/:reblog_id should return 404.
#[tokio::test]
async fn test_delete_original_cascades_to_reblogs() {
    let ctx = TestContext::new("delete-cascade-reblog").await;

    // Alice posts; Bob boosts it.
    let original = ctx.api.post_status(&ctx.alice_token, "original to cascade-delete", "public").await;
    let original_id = original["id"].as_str().unwrap().to_string();

    let boost_resp = ctx.api
        .post_json(&format!("/api/v1/statuses/{original_id}/reblog"), Some(&ctx.bob_token), &json!({}))
        .await;
    assert_eq!(boost_resp.status(), StatusCode::OK);
    let boost: Value = boost_resp.json().await.unwrap();
    let reblog_id = boost["id"].as_str().unwrap().to_string();

    // Sanity: reblog is visible before deletion.
    let before = ctx.api.get(&format!("/api/v1/statuses/{reblog_id}"), Some(&ctx.bob_token)).await;
    assert_eq!(before.status(), StatusCode::OK, "reblog should be visible before original is deleted");

    // Alice deletes the original.
    let del = ctx.api
        .delete(&format!("/api/v1/statuses/{original_id}"), &ctx.alice_token)
        .await;
    assert_eq!(del.status(), StatusCode::OK, "delete original should succeed");

    // Bob's reblog should now be gone (410 because it existed then was deleted).
    let after = ctx.api.get(&format!("/api/v1/statuses/{reblog_id}"), Some(&ctx.bob_token)).await;
    assert_eq!(
        after.status(),
        StatusCode::GONE,
        "reblog should be 410 after original is deleted (cascade)",
    );
}
