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

/// Reblogging a direct status → 403.
#[tokio::test]
async fn test_reblog_direct_returns_403() {
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
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
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
async fn test_deleted_status_returns_404() {
    let ctx = TestContext::new("del-404").await;

    let status = ctx.api.post_status(&ctx.alice_token, "to be deleted", "public").await;
    let id = status["id"].as_str().unwrap();

    let del_resp = ctx.api.delete(&format!("/api/v1/statuses/{id}"), &ctx.alice_token).await;
    assert_eq!(del_resp.status(), StatusCode::OK);

    let get_resp = ctx.api.get(&format!("/api/v1/statuses/{id}"), None).await;
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
