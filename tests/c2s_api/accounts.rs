use reqwest::StatusCode;
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

use super::helpers::TestContext;

// ── account statuses visibility ──────────────────────────────────────────────

/// Private statuses are hidden from unauthenticated viewers.
#[tokio::test]
async fn test_account_statuses_hides_private_from_unauthenticated() {
    let ctx = TestContext::new("acct-stat-unauth").await;

    let prv = ctx.api.post_status(&ctx.alice_token, "alice private acct", "private").await;
    let pub_s = ctx.api.post_status(&ctx.alice_token, "alice public acct", "public").await;

    let resp = ctx
        .api
        .get(&format!("/api/v1/accounts/{}/statuses", ctx.alice_id), None)
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let statuses: Vec<Value> = resp.json().await.unwrap();

    let ids: Vec<&str> = statuses.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(!ids.contains(&prv["id"].as_str().unwrap()), "private status visible to unauthenticated user");
    assert!(ids.contains(&pub_s["id"].as_str().unwrap()), "public status missing from unauthenticated view");
}

/// Private statuses are hidden from non-followers.
#[tokio::test]
async fn test_account_statuses_hides_private_from_non_follower() {
    let ctx = TestContext::new("acct-stat-stranger").await;

    let prv = ctx.api.post_status(&ctx.alice_token, "alice prv stranger", "private").await;

    let resp = ctx
        .api
        .get(
            &format!("/api/v1/accounts/{}/statuses", ctx.alice_id),
            Some(&ctx.bob_token),
        )
        .await;
    let statuses: Vec<Value> = resp.json().await.unwrap();
    let ids: Vec<&str> = statuses.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(!ids.contains(&prv["id"].as_str().unwrap()), "private status visible to non-follower");
}

/// Private statuses appear in account statuses for accepted followers.
#[tokio::test]
async fn test_account_statuses_shows_private_to_follower() {
    let ctx = TestContext::new("acct-stat-follower").await;

    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;

    let prv = ctx.api.post_status(&ctx.alice_token, "alice prv follower", "private").await;

    let resp = ctx
        .api
        .get(
            &format!("/api/v1/accounts/{}/statuses", ctx.alice_id),
            Some(&ctx.bob_token),
        )
        .await;
    let statuses: Vec<Value> = resp.json().await.unwrap();
    let ids: Vec<&str> = statuses.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(ids.contains(&prv["id"].as_str().unwrap()), "private status hidden from accepted follower");
}

/// Account statuses shows all visibilities to the account owner.
#[tokio::test]
async fn test_account_statuses_shows_all_to_self() {
    let ctx = TestContext::new("acct-stat-self").await;

    let pub_s = ctx.api.post_status(&ctx.alice_token, "self public", "public").await;
    let prv_s = ctx.api.post_status(&ctx.alice_token, "self private", "private").await;
    let dir_s = ctx.api.post_status(&ctx.alice_token, "self direct", "direct").await;

    let resp = ctx
        .api
        .get(
            &format!("/api/v1/accounts/{}/statuses", ctx.alice_id),
            Some(&ctx.alice_token),
        )
        .await;
    let statuses: Vec<Value> = resp.json().await.unwrap();
    let ids: Vec<&str> = statuses.iter().filter_map(|s| s["id"].as_str()).collect();

    assert!(ids.contains(&pub_s["id"].as_str().unwrap()));
    assert!(ids.contains(&prv_s["id"].as_str().unwrap()));
    assert!(ids.contains(&dir_s["id"].as_str().unwrap()));
}

// ── account statuses filters ───────────────────────────────────────────────────

/// ?exclude_replies=true omits replies from account statuses.
#[tokio::test]
async fn test_account_statuses_exclude_replies() {
    let ctx = TestContext::new("acct-excl-reply").await;

    let parent = ctx.api.post_status(&ctx.alice_token, "parent status", "public").await;
    let parent_id = parent["id"].as_str().unwrap();
    let reply: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({"status": "reply", "in_reply_to_id": parent_id, "visibility": "public"}),
    ).await.json().await.unwrap();
    let reply_id = reply["id"].as_str().unwrap();

    let resp = ctx.api.get(
        &format!("/api/v1/accounts/{}/statuses?exclude_replies=true", ctx.alice_id),
        Some(&ctx.alice_token),
    ).await;
    let statuses: Vec<Value> = resp.json().await.unwrap();
    assert!(
        !statuses.iter().any(|s| s["id"].as_str() == Some(reply_id)),
        "reply should be excluded from results",
    );
    assert!(
        statuses.iter().any(|s| s["id"].as_str() == Some(parent_id)),
        "parent should still appear",
    );
}

/// ?exclude_reblogs=true omits reblogs from account statuses.
#[tokio::test]
async fn test_account_statuses_exclude_reblogs() {
    let ctx = TestContext::new("acct-excl-rb").await;

    let original = ctx.api.post_status(&ctx.bob_token, "rebloggable", "public").await;
    let orig_id = original["id"].as_str().unwrap();
    let reblog: Value = ctx.api.post_json(
        &format!("/api/v1/statuses/{orig_id}/reblog"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await.json().await.unwrap();
    let reblog_id = reblog["id"].as_str().unwrap();

    let statuses: Vec<Value> = ctx.api.get(
        &format!("/api/v1/accounts/{}/statuses?exclude_reblogs=true", ctx.alice_id),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    assert!(
        !statuses.iter().any(|s| s["id"].as_str() == Some(reblog_id)),
        "reblog should be excluded",
    );
}

/// ?pinned=true returns only pinned statuses.
#[tokio::test]
async fn test_account_statuses_pinned() {
    let ctx = TestContext::new("acct-pinned").await;

    let status = ctx.api.post_status(&ctx.alice_token, "to pin", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/pin"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    let statuses: Vec<Value> = ctx.api.get(
        &format!("/api/v1/accounts/{}/statuses?pinned=true", ctx.alice_id),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    assert!(statuses.iter().any(|s| s["id"].as_str() == Some(id)));
    for s in &statuses {
        assert_eq!(s["pinned"].as_bool(), Some(true));
    }
}

/// ?tagged=<name> endpoint returns 200 (filtering implementation tracked separately).
#[tokio::test]
async fn test_account_statuses_tagged_returns_200() {
    let ctx = TestContext::new("acct-tagged-ok").await;

    ctx.api.post_status(&ctx.alice_token, "post with #tagxyz888", "public").await;

    let resp = ctx.api.get(
        &format!("/api/v1/accounts/{}/statuses?tagged=tagxyz888", ctx.alice_id),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let _: Vec<Value> = resp.json().await.unwrap();
}

// ── follow lifecycle ──────────────────────────────────────────────────────────

/// Following an unlocked account is immediately accepted.
#[tokio::test]
async fn test_follow_unlocked_account_is_accepted() {
    let ctx = TestContext::new("follow-unlocked").await;

    let rel = ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;
    assert_eq!(rel["following"].as_bool(), Some(true));
    assert_eq!(rel["requested"].as_bool(), Some(false));
}

/// Following a locked account creates a pending follow request.
#[tokio::test]
async fn test_follow_locked_account_is_pending() {
    let ctx = TestContext::new("follow-locked").await;

    // Lock Bob's account directly in the DB.
    let db_url = std::env::var("DATABASE_URL").unwrap();
    let db = PgPoolOptions::new().max_connections(2).connect(&db_url).await.unwrap();
    let bob_uuid: Uuid = ctx.bob_id.parse().unwrap();
    sqlx::query!("UPDATE accounts SET locked = true WHERE id = $1", bob_uuid)
        .execute(&db)
        .await
        .unwrap();

    let rel = ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;
    assert_eq!(rel["following"].as_bool(), Some(false));
    assert_eq!(rel["requested"].as_bool(), Some(true));
}

// ── verify credentials ────────────────────────────────────────────────────────

/// GET /api/v1/accounts/verify_credentials returns the current user's account.
#[tokio::test]
async fn test_verify_credentials() {
    let ctx = TestContext::new("verify-creds").await;

    let resp = ctx.api.get("/api/v1/accounts/verify_credentials", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    assert_eq!(body["username"].as_str(), Some("alice"));
    assert!(body["id"].as_str().is_some(), "id field missing");
    assert!(body["acct"].as_str().is_some(), "acct field missing");
    assert!(body["source"].is_object(), "source field missing from verify_credentials");
}

/// GET /api/v1/accounts/verify_credentials without token → 401.
#[tokio::test]
async fn test_verify_credentials_requires_auth() {
    let ctx = TestContext::new("verify-unauth").await;

    let resp = ctx.api.get("/api/v1/accounts/verify_credentials", None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── account lookup ────────────────────────────────────────────────────────────

/// GET /api/v1/accounts/:id returns account data.
#[tokio::test]
async fn test_get_account() {
    let ctx = TestContext::new("get-acct").await;

    let resp = ctx.api.get(&format!("/api/v1/accounts/{}", ctx.alice_id), None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    assert_eq!(body["id"].as_str(), Some(ctx.alice_id.as_str()));
    assert_eq!(body["username"].as_str(), Some("alice"));
}

/// GET /api/v1/accounts/:id for unknown id → 404.
#[tokio::test]
async fn test_get_account_not_found() {
    let ctx = TestContext::new("get-acct-404").await;

    let resp = ctx.api.get("/api/v1/accounts/00000000-0000-0000-0000-000000000000", None).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// GET /api/v1/accounts/lookup?acct=alice returns Alice's account.
#[tokio::test]
async fn test_lookup_account() {
    let ctx = TestContext::new("lookup").await;

    let resp = ctx.api.get("/api/v1/accounts/lookup?acct=alice", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    assert_eq!(body["username"].as_str(), Some("alice"));
}

/// GET /api/v1/accounts/:id/followers returns a list after a follow.
#[tokio::test]
async fn test_get_account_followers() {
    let ctx = TestContext::new("acct-followers").await;

    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;

    let resp = ctx.api.get(
        &format!("/api/v1/accounts/{}/followers", ctx.alice_id),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(list.iter().any(|a| a["id"].as_str() == Some(ctx.bob_id.as_str())));
}

/// GET /api/v1/accounts/:id/following returns a list after a follow.
#[tokio::test]
async fn test_get_account_following() {
    let ctx = TestContext::new("acct-following").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let resp = ctx.api.get(
        &format!("/api/v1/accounts/{}/following", ctx.alice_id),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(list.iter().any(|a| a["id"].as_str() == Some(ctx.bob_id.as_str())));
}

// ── relationships ─────────────────────────────────────────────────────────────

/// GET /api/v1/accounts/relationships reflects follow state.
#[tokio::test]
async fn test_get_relationships() {
    let ctx = TestContext::new("rel-basic").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let resp = ctx.api.get(
        &format!("/api/v1/accounts/relationships?id[]={}", ctx.bob_id),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["following"].as_bool(), Some(true));
    assert_eq!(list[0]["id"].as_str(), Some(ctx.bob_id.as_str()));
}

/// Unfollowing sets following=false in the relationship.
#[tokio::test]
async fn test_unfollow_updates_relationship() {
    let ctx = TestContext::new("rel-unfollow").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let resp = ctx.api.post_json(
        &format!("/api/v1/accounts/{}/unfollow", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let rel: Value = resp.json().await.unwrap();
    assert_eq!(rel["following"].as_bool(), Some(false));
}

/// Blocking sets blocking=true; unblocking sets it back to false.
#[tokio::test]
async fn test_block_and_unblock() {
    let ctx = TestContext::new("block").await;

    let block_resp = ctx.api.post_json(
        &format!("/api/v1/accounts/{}/block", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(block_resp.status(), StatusCode::OK);
    let rel: Value = block_resp.json().await.unwrap();
    assert_eq!(rel["blocking"].as_bool(), Some(true));

    let unblock_resp = ctx.api.post_json(
        &format!("/api/v1/accounts/{}/unblock", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(unblock_resp.status(), StatusCode::OK);
    let rel2: Value = unblock_resp.json().await.unwrap();
    assert_eq!(rel2["blocking"].as_bool(), Some(false));
}

/// Muting sets muting=true; unmuting sets it back to false.
#[tokio::test]
async fn test_mute_and_unmute() {
    let ctx = TestContext::new("mute").await;

    let mute_resp = ctx.api.post_json(
        &format!("/api/v1/accounts/{}/mute", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(mute_resp.status(), StatusCode::OK);
    let rel: Value = mute_resp.json().await.unwrap();
    assert_eq!(rel["muting"].as_bool(), Some(true));

    let unmute_resp = ctx.api.post_json(
        &format!("/api/v1/accounts/{}/unmute", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(unmute_resp.status(), StatusCode::OK);
    let rel2: Value = unmute_resp.json().await.unwrap();
    assert_eq!(rel2["muting"].as_bool(), Some(false));
}

// ── follow requests ───────────────────────────────────────────────────────────

/// Accepting a pending follow request changes the relationship to following=true.
#[tokio::test]
async fn test_authorize_follow_request() {
    let ctx = TestContext::new("follow-req-accept").await;

    let db_url = std::env::var("DATABASE_URL").unwrap();
    let db = PgPoolOptions::new().max_connections(2).connect(&db_url).await.unwrap();
    let bob_uuid: Uuid = ctx.bob_id.parse().unwrap();
    sqlx::query!("UPDATE accounts SET locked = true WHERE id = $1", bob_uuid)
        .execute(&db).await.unwrap();

    // Alice follows locked Bob → pending.
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    // Bob authorises Alice's follow request.
    let requests_resp = ctx.api.get("/api/v1/follow_requests", Some(&ctx.bob_token)).await;
    let requests: Vec<Value> = requests_resp.json().await.unwrap();
    assert!(!requests.is_empty(), "no pending follow requests");
    let requester_id = requests[0]["id"].as_str().unwrap().to_string();

    let accept_resp = ctx.api.post_json(
        &format!("/api/v1/follow_requests/{requester_id}/authorize"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(accept_resp.status(), StatusCode::OK);

    // Alice is now following Bob.
    let rels: Vec<Value> = ctx.api.get(
        &format!("/api/v1/accounts/relationships?id[]={}", ctx.bob_id),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    assert_eq!(rels[0]["following"].as_bool(), Some(true));
    assert_eq!(rels[0]["requested"].as_bool(), Some(false));
}

/// Rejecting a pending follow request leaves following=false, requested=false.
#[tokio::test]
async fn test_reject_follow_request() {
    let ctx = TestContext::new("follow-req-reject").await;

    let db_url = std::env::var("DATABASE_URL").unwrap();
    let db = PgPoolOptions::new().max_connections(2).connect(&db_url).await.unwrap();
    let bob_uuid: Uuid = ctx.bob_id.parse().unwrap();
    sqlx::query!("UPDATE accounts SET locked = true WHERE id = $1", bob_uuid)
        .execute(&db).await.unwrap();

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let requests: Vec<Value> = ctx.api.get("/api/v1/follow_requests", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    let requester_id = requests[0]["id"].as_str().unwrap().to_string();

    let reject_resp = ctx.api.post_json(
        &format!("/api/v1/follow_requests/{requester_id}/reject"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(reject_resp.status(), StatusCode::OK);

    let rels: Vec<Value> = ctx.api.get(
        &format!("/api/v1/accounts/relationships?id[]={}", ctx.bob_id),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    assert_eq!(rels[0]["following"].as_bool(), Some(false));
    assert_eq!(rels[0]["requested"].as_bool(), Some(false));
}

// ── blocks and mutes lists ────────────────────────────────────────────────────

/// After blocking Bob, GET /api/v1/blocks includes him.
#[tokio::test]
async fn test_blocks_list_includes_blocked() {
    let ctx = TestContext::new("blocks-list").await;

    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/block", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    let resp = ctx.api.get("/api/v1/blocks", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(list.iter().any(|a| a["id"].as_str() == Some(ctx.bob_id.as_str())));
}

/// After muting Bob, GET /api/v1/mutes includes him.
#[tokio::test]
async fn test_mutes_list_includes_muted() {
    let ctx = TestContext::new("mutes-list").await;

    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/mute", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    let resp = ctx.api.get("/api/v1/mutes", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(list.iter().any(|a| a["id"].as_str() == Some(ctx.bob_id.as_str())));
}

// ── preferences ───────────────────────────────────────────────────────────────

/// GET /api/v1/preferences returns colon-separated keys expected by clients.
#[tokio::test]
async fn test_get_preferences() {
    let ctx = TestContext::new("prefs").await;

    let resp = ctx.api.get("/api/v1/preferences", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["posting:default:visibility"].as_str().is_some(),
        "posting:default:visibility missing: {body}",
    );
    assert!(
        body.get("reading:expand:media").is_some(),
        "reading:expand:media missing: {body}",
    );
}

// ── endorse / unendorse ───────────────────────────────────────────────────────

/// Endorsing Bob sets endorsed=true; unendorsing reverts it.
#[tokio::test]
async fn test_endorse_and_unendorse() {
    let ctx = TestContext::new("endorse").await;

    let endorse_resp = ctx.api.post_json(
        &format!("/api/v1/accounts/{}/endorse", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(endorse_resp.status(), StatusCode::OK);
    let rel: Value = endorse_resp.json().await.unwrap();
    assert_eq!(rel["endorsed"].as_bool(), Some(true));

    let unendorse_resp = ctx.api.post_json(
        &format!("/api/v1/accounts/{}/unendorse", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(unendorse_resp.status(), StatusCode::OK);
    let rel2: Value = unendorse_resp.json().await.unwrap();
    assert_eq!(rel2["endorsed"].as_bool(), Some(false));
}

/// GET /api/v1/accounts/:id/endorsements returns endorsed accounts.
#[tokio::test]
async fn test_get_endorsements_list() {
    let ctx = TestContext::new("endorse-list").await;

    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/endorse", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    let resp = ctx.api.get(
        &format!("/api/v1/accounts/{}/endorsements", ctx.alice_id),
        None,
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(list.iter().any(|a| a["id"].as_str() == Some(ctx.bob_id.as_str())));
}

// ── account note ──────────────────────────────────────────────────────────────

/// Setting an account note is reflected in the relationship.
#[tokio::test]
async fn test_set_account_note() {
    let ctx = TestContext::new("acct-note").await;

    let resp = ctx.api.post_json(
        &format!("/api/v1/accounts/{}/note", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({"comment": "Note about Bob"}),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let rel: Value = resp.json().await.unwrap();
    assert_eq!(rel["note"].as_str(), Some("Note about Bob"));
}

// ── remove from followers ─────────────────────────────────────────────────────

/// After Alice removes Bob from her followers, Bob's relationship shows following=false.
#[tokio::test]
async fn test_remove_from_followers() {
    let ctx = TestContext::new("rm-follower").await;

    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;

    let resp = ctx.api.post_json(
        &format!("/api/v1/accounts/{}/remove_from_followers", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let rels: Vec<Value> = ctx.api.get(
        &format!("/api/v1/accounts/relationships?id[]={}", ctx.alice_id),
        Some(&ctx.bob_token),
    ).await.json().await.unwrap();
    assert_eq!(rels[0]["following"].as_bool(), Some(false));
}

// ── profile settings ──────────────────────────────────────────────────────────

/// PUT /api/v1/profile returns 200 with the account object.
#[tokio::test]
async fn test_update_profile_settings() {
    let ctx = TestContext::new("profile-settings").await;

    let resp = ctx.api.put_json(
        "/api/v1/profile",
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(body["id"].as_str().is_some());
}

// ── update_credentials ────────────────────────────────────────────────────────

/// PATCH /api/v1/accounts/update_credentials (multipart) updates display_name.
#[tokio::test]
async fn test_update_credentials_display_name() {
    let ctx = TestContext::new("update-creds").await;

    let form = reqwest::multipart::Form::new()
        .text("display_name", "Alice Updated");

    let resp = ctx.api.http
        .patch(ctx.api.url("/api/v1/accounts/update_credentials"))
        .header("host", &ctx.api.host)
        .bearer_auth(&ctx.alice_token)
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["display_name"].as_str(), Some("Alice Updated"));
}

/// PATCH /api/v1/accounts/update_credentials updates bio note.
#[tokio::test]
async fn test_update_credentials_note() {
    let ctx = TestContext::new("update-note").await;

    let form = reqwest::multipart::Form::new()
        .text("note", "This is my bio");

    let resp = ctx.api.http
        .patch(ctx.api.url("/api/v1/accounts/update_credentials"))
        .header("host", &ctx.api.host)
        .bearer_auth(&ctx.alice_token)
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["source"]["note"].as_str().unwrap_or("").contains("This is my bio"),
        "note not updated: {body}",
    );
}

/// PATCH /api/v1/accounts/update_credentials with locked=true makes account locked.
#[tokio::test]
async fn test_update_credentials_locked() {
    let ctx = TestContext::new("update-locked").await;

    let form = reqwest::multipart::Form::new()
        .text("locked", "true");

    let resp = ctx.api.http
        .patch(ctx.api.url("/api/v1/accounts/update_credentials"))
        .header("host", &ctx.api.host)
        .bearer_auth(&ctx.alice_token)
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["locked"].as_bool(), Some(true));

    // Follow from Bob should now be pending.
    let rel = ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;
    assert_eq!(rel["requested"].as_bool(), Some(true));
}

// ── familiar followers ────────────────────────────────────────────────────────

/// GET /api/v1/accounts/familiar_followers returns an array of familiar-followers objects.
#[tokio::test]
async fn test_familiar_followers_returns_array() {
    let ctx = TestContext::new("familiar").await;

    let resp = ctx.api.get(
        &format!("/api/v1/accounts/familiar_followers?id[]={}", ctx.bob_id),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"].as_str(), Some(ctx.bob_id.as_str()));
    assert!(list[0]["accounts"].is_array());
}

// ── suggestions ───────────────────────────────────────────────────────────────

/// GET /api/v1/suggestions returns a JSON array.
#[tokio::test]
async fn test_get_suggestions() {
    let ctx = TestContext::new("suggest").await;

    let resp = ctx.api.get("/api/v1/suggestions", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let _: Vec<Value> = resp.json().await.unwrap();
}

/// DELETE /api/v1/suggestions/:id returns 200.
#[tokio::test]
async fn test_dismiss_suggestion() {
    let ctx = TestContext::new("suggest-dismiss").await;

    let resp = ctx.api.delete(
        &format!("/api/v1/suggestions/{}", ctx.bob_id),
        &ctx.alice_token,
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

/// GET /api/v2/suggestions returns suggestions with a source field.
#[tokio::test]
async fn test_get_suggestions_v2() {
    let ctx = TestContext::new("suggest-v2").await;

    let resp = ctx.api.get("/api/v2/suggestions", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let _: Vec<Value> = resp.json().await.unwrap();
}

// ── directory ─────────────────────────────────────────────────────────────────

/// GET /api/v1/directory returns local accounts (includes alice).
#[tokio::test]
async fn test_get_directory() {
    let ctx = TestContext::new("directory").await;

    let resp = ctx.api.get("/api/v1/directory", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(
        list.iter().any(|a| a["username"].as_str() == Some("alice")),
        "alice not found in directory",
    );
}

// ── account search endpoint ───────────────────────────────────────────────────

/// GET /api/v1/accounts/search returns matching accounts.
#[tokio::test]
async fn test_accounts_search_endpoint() {
    let ctx = TestContext::new("acct-search").await;

    let resp = ctx.api.get(
        "/api/v1/accounts/search?q=bob",
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(list.iter().any(|a| a["username"].as_str() == Some("bob")));
}

// ── block effects ─────────────────────────────────────────────────────────────

/// Blocking removes the follow relationship in both directions.
#[tokio::test]
async fn test_block_removes_follow() {
    let ctx = TestContext::new("block-rm-follow").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;
    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;

    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/block", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    let rels: Vec<Value> = ctx.api.get(
        &format!("/api/v1/accounts/relationships?id[]={}", ctx.bob_id),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    assert_eq!(rels[0]["following"].as_bool(), Some(false), "alice should not follow bob after block");
    assert_eq!(rels[0]["followed_by"].as_bool(), Some(false), "bob should not follow alice after block");
}

// ── account lists ─────────────────────────────────────────────────────────────

/// GET /api/v1/accounts/:id/lists returns lists that include the given account.
#[tokio::test]
async fn test_get_account_lists() {
    let ctx = TestContext::new("acct-lists").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let list: Value = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.alice_token),
        &json!({"title": "Bob's List"}),
    ).await.json().await.unwrap();
    let list_id = list["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/lists/{list_id}/accounts"),
        Some(&ctx.alice_token),
        &json!({"account_ids": [ctx.bob_id]}),
    ).await;

    let lists: Vec<Value> = ctx.api.get(
        &format!("/api/v1/accounts/{}/lists", ctx.bob_id),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    assert!(lists.iter().any(|l| l["id"].as_str() == Some(list_id)));
}

// ── domain blocks ─────────────────────────────────────────────────────────────

/// Block a domain, list it, unblock it.
#[tokio::test]
async fn test_domain_block_lifecycle() {
    let ctx = TestContext::new("domain-block").await;

    let block_resp = ctx.api.post_json(
        "/api/v1/domain_blocks",
        Some(&ctx.alice_token),
        &json!({"domain": "evil.example.com"}),
    ).await;
    assert_eq!(block_resp.status(), StatusCode::OK);

    let domains: Vec<String> = ctx.api.get("/api/v1/domain_blocks", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(domains.contains(&"evil.example.com".to_string()));

    let unblock_resp = ctx.api.http
        .delete(ctx.api.url("/api/v1/domain_blocks"))
        .header("host", &ctx.api.host)
        .bearer_auth(&ctx.alice_token)
        .json(&json!({"domain": "evil.example.com"}))
        .send().await.unwrap();
    assert_eq!(unblock_resp.status(), StatusCode::OK);

    let after: Vec<String> = ctx.api.get("/api/v1/domain_blocks", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(!after.contains(&"evil.example.com".to_string()));
}
