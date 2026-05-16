use reqwest::StatusCode;
use serde_json::{json, Value};

use super::helpers::TestContext;
use sqlx::Executor as _;

/// Public timeline must only contain statuses with visibility == "public".
#[tokio::test]
async fn test_public_timeline_only_shows_public() {
    let ctx = TestContext::new("pub-timeline").await;

    let pub_s = ctx.api.post_status(&ctx.alice_token, "public post", "public").await;
    ctx.api.post_status(&ctx.alice_token, "unlisted post", "unlisted").await;
    ctx.api.post_status(&ctx.alice_token, "private post", "private").await;
    ctx.api.post_status(&ctx.alice_token, "direct post", "direct").await;

    let pub_id = pub_s["id"].as_str().unwrap();
    let timeline = ctx.api.public_timeline().await;

    for status in &timeline {
        let vis = status["visibility"].as_str().unwrap();
        assert_eq!(vis, "public", "public timeline contained status with visibility={vis}");
    }
    assert!(
        timeline.iter().any(|s| s["id"].as_str() == Some(pub_id)),
        "public status not found in public timeline"
    );
}

/// Unlisted statuses must not appear on the public timeline.
#[tokio::test]
async fn test_unlisted_absent_from_public_timeline() {
    let ctx = TestContext::new("unlisted-timeline").await;

    let status = ctx.api.post_status(&ctx.alice_token, "unlisted post visible", "unlisted").await;
    let id = status["id"].as_str().unwrap().to_string();

    let timeline = ctx.api.public_timeline().await;
    let ids: Vec<&str> = timeline.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(!ids.contains(&id.as_str()), "unlisted status appeared in public timeline");
}

/// Home timeline includes all visibility levels from followed accounts.
#[tokio::test]
async fn test_home_timeline_shows_all_visibility_from_follows() {
    let ctx = TestContext::new("home-visibility").await;

    // Alice follows Bob.
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    // Bob posts in every visibility.
    let pub_s = ctx.api.post_status(&ctx.bob_token, "bob public", "public").await;
    let unl_s = ctx.api.post_status(&ctx.bob_token, "bob unlisted", "unlisted").await;
    let prv_s = ctx.api.post_status(&ctx.bob_token, "bob private", "private").await;

    let home = ctx.api.home_timeline(&ctx.alice_token).await;
    let ids: Vec<&str> = home.iter().filter_map(|s| s["id"].as_str()).collect();

    assert!(ids.contains(&pub_s["id"].as_str().unwrap()), "public status missing from home timeline");
    assert!(ids.contains(&unl_s["id"].as_str().unwrap()), "unlisted status missing from home timeline");
    assert!(ids.contains(&prv_s["id"].as_str().unwrap()), "private status missing from home timeline for accepted follower");
}

/// Home timeline must not include posts from accounts Alice doesn't follow.
#[tokio::test]
async fn test_home_timeline_excludes_non_followed_accounts() {
    let ctx = TestContext::new("home-exclude").await;

    // Bob posts but Alice does NOT follow him.
    let status = ctx.api.post_status(&ctx.bob_token, "bob public unfollowed", "public").await;
    let id = status["id"].as_str().unwrap().to_string();

    let home = ctx.api.home_timeline(&ctx.alice_token).await;
    let ids: Vec<&str> = home.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(!ids.contains(&id.as_str()), "non-followed account's post appeared in home timeline");
}

/// Own posts always appear on the home timeline regardless of visibility.
#[tokio::test]
async fn test_home_timeline_shows_own_posts_all_visibility() {
    let ctx = TestContext::new("home-own").await;

    let pub_s = ctx.api.post_status(&ctx.alice_token, "own public", "public").await;
    let prv_s = ctx.api.post_status(&ctx.alice_token, "own private", "private").await;
    let dir_s = ctx.api.post_status(&ctx.alice_token, "own direct", "direct").await;

    let home = ctx.api.home_timeline(&ctx.alice_token).await;
    let ids: Vec<&str> = home.iter().filter_map(|s| s["id"].as_str()).collect();

    assert!(ids.contains(&pub_s["id"].as_str().unwrap()));
    assert!(ids.contains(&prv_s["id"].as_str().unwrap()));
    assert!(ids.contains(&dir_s["id"].as_str().unwrap()));
}

/// GET /api/v1/timelines/home without a token → 401.
#[tokio::test]
async fn test_home_timeline_requires_auth() {
    let ctx = TestContext::new("auth-home").await;

    let resp = ctx.api.get("/api/v1/timelines/home", None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// The federated public timeline must be scoped to the requesting instance.
///
/// Regression test: previously the query omitted the `instance_id` filter when
/// `?local=false`, causing every instance's statuses to bleed into every other
/// instance's timeline.
#[tokio::test]
async fn test_public_timeline_scoped_to_instance() {
    let ctx_a = TestContext::new("scope-a").await;
    let ctx_b = TestContext::new("scope-b").await;

    // Post publicly on instance B.
    let b_status = ctx_b.api.post_status(&ctx_b.alice_token, "from instance B only", "public").await;
    let b_id = b_status["id"].as_str().unwrap().to_string();

    // Also post on instance A so the timeline is non-empty.
    let a_status = ctx_a.api.post_status(&ctx_a.alice_token, "from instance A only", "public").await;
    let a_id = a_status["id"].as_str().unwrap().to_string();

    // Federated timeline (no ?local) for instance A.
    let timeline: Vec<Value> = ctx_a.api
        .get("/api/v1/timelines/public", None)
        .await
        .json()
        .await
        .unwrap();

    let ids: Vec<&str> = timeline.iter().filter_map(|s| s["id"].as_str()).collect();

    assert!(
        ids.contains(&a_id.as_str()),
        "instance A's own status not found in its federated timeline"
    );
    assert!(
        !ids.contains(&b_id.as_str()),
        "instance B's status leaked into instance A's federated timeline"
    );
}

#[tokio::test]
async fn test_public_timeline_max_id_pagination() {
    let ctx = TestContext::new("paginate-max").await;

    let s1 = ctx.api.post_status(&ctx.alice_token, "paginate-a", "public").await;
    let s2 = ctx.api.post_status(&ctx.alice_token, "paginate-b", "public").await;
    let s3 = ctx.api.post_status(&ctx.alice_token, "paginate-c", "public").await;

    let s2_id = s2["id"].as_str().unwrap();
    let s1_id = s1["id"].as_str().unwrap();
    let s3_id = s3["id"].as_str().unwrap();

    // max_id=s2 should return only s1 (older), not s2 or s3.
    let timeline: Vec<Value> = ctx.api.get(
        &format!("/api/v1/timelines/public?local=true&max_id={s2_id}"),
        None,
    ).await.json().await.unwrap();

    let ids: Vec<&str> = timeline.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(ids.contains(&s1_id), "s1 missing from max_id page");
    assert!(!ids.contains(&s2_id), "s2 should not appear with max_id=s2");
    assert!(!ids.contains(&s3_id), "s3 should not appear with max_id=s2");
}

/// Paginated response includes a Link header with rel="next" and rel="prev".
#[tokio::test]
async fn test_public_timeline_link_header() {
    let ctx = TestContext::new("paginate-link").await;

    // Post enough statuses to trigger pagination.
    for i in 0..5 {
        ctx.api.post_status(&ctx.alice_token, &format!("link-header-test {i}"), "public").await;
    }

    let resp = ctx.api.get("/api/v1/timelines/public?local=true&limit=2", None).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let link = resp.headers().get("link").and_then(|v| v.to_str().ok()).unwrap_or("");
    assert!(link.contains("rel=\"next\""), "Link header missing rel=next: {link}");
    assert!(link.contains("rel=\"prev\""), "Link header missing rel=prev: {link}");
}

/// since_id returns statuses newer than the given id (exclusive).
#[tokio::test]
async fn test_public_timeline_since_id_pagination() {
    let ctx = TestContext::new("paginate-since").await;

    let s1 = ctx.api.post_status(&ctx.alice_token, "since-a", "public").await;
    let s2 = ctx.api.post_status(&ctx.alice_token, "since-b", "public").await;
    let s3 = ctx.api.post_status(&ctx.alice_token, "since-c", "public").await;

    let s1_id = s1["id"].as_str().unwrap();
    let s2_id = s2["id"].as_str().unwrap();
    let s3_id = s3["id"].as_str().unwrap();

    let timeline: Vec<Value> = ctx.api.get(
        &format!("/api/v1/timelines/public?local=true&since_id={s1_id}"),
        None,
    ).await.json().await.unwrap();

    let ids: Vec<&str> = timeline.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(ids.contains(&s2_id), "s2 missing from since_id page");
    assert!(ids.contains(&s3_id), "s3 missing from since_id page");
    assert!(!ids.contains(&s1_id), "s1 should not appear with since_id=s1");
}

/// GET /api/v1/timelines/tag/:hashtag returns statuses tagged with that hashtag.
#[tokio::test]
async fn test_tag_timeline() {
    let ctx = TestContext::new("tag-timeline").await;

    let tagged = ctx.api.post_status(&ctx.alice_token, "This has #tagtest456 in it", "public").await;
    let tagged_id = tagged["id"].as_str().unwrap();

    let timeline: Vec<Value> = ctx.api.get(
        "/api/v1/timelines/tag/tagtest456",
        None,
    ).await.json().await.unwrap();

    assert!(
        timeline.iter().any(|s| s["id"].as_str() == Some(tagged_id)),
        "tagged status not in tag timeline",
    );
}

/// Tag timeline any[] returns statuses that contain the primary tag AND at least one of the any tags.
#[tokio::test]
async fn test_tag_timeline_any_filter() {
    let ctx = TestContext::new("tag-any").await;

    // Status with both tags.
    let both = ctx.api.post_status(&ctx.alice_token, "has #anyfilter and #secondary", "public").await;
    let both_id = both["id"].as_str().unwrap();

    // Status with only the primary tag.
    let primary_only = ctx.api.post_status(&ctx.alice_token, "has #anyfilter only", "public").await;
    let primary_only_id = primary_only["id"].as_str().unwrap();

    // any[]=secondary → only status with both tags.
    let timeline: Vec<Value> = ctx.api.get(
        "/api/v1/timelines/tag/anyfilter?any[]=secondary",
        None,
    ).await.json().await.unwrap();

    let ids: Vec<&str> = timeline.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(ids.contains(&both_id), "status with both tags should be in any[] result");
    assert!(!ids.contains(&primary_only_id), "status with only primary tag should be excluded by any[]");
}

/// Tag timeline all[] returns only statuses that contain ALL of the specified additional tags.
#[tokio::test]
async fn test_tag_timeline_all_filter() {
    let ctx = TestContext::new("tag-all").await;

    // Status with primary tag and both required tags.
    let both = ctx.api.post_status(
        &ctx.alice_token,
        "has #allfilter #required1 #required2",
        "public",
    ).await;
    let both_id = both["id"].as_str().unwrap();

    // Status with primary tag and only one required tag.
    let partial = ctx.api.post_status(
        &ctx.alice_token,
        "has #allfilter #required1 only",
        "public",
    ).await;
    let partial_id = partial["id"].as_str().unwrap();

    // all[]=required1&all[]=required2 → only status with both additional tags.
    let timeline: Vec<Value> = ctx.api.get(
        "/api/v1/timelines/tag/allfilter?all[]=required1&all[]=required2",
        None,
    ).await.json().await.unwrap();

    let ids: Vec<&str> = timeline.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(ids.contains(&both_id), "status with all required tags should be in result");
    assert!(!ids.contains(&partial_id), "status missing a required tag should be excluded by all[]");
}

/// Tag timeline none[] excludes statuses that contain any of the none tags.
#[tokio::test]
async fn test_tag_timeline_none_filter() {
    let ctx = TestContext::new("tag-none").await;

    // Status with both the primary tag and a banned tag.
    let with_banned = ctx.api.post_status(&ctx.alice_token, "has #nonefilter and #banned", "public").await;
    let with_banned_id = with_banned["id"].as_str().unwrap();

    // Status with only the primary tag.
    let clean = ctx.api.post_status(&ctx.alice_token, "has #nonefilter only", "public").await;
    let clean_id = clean["id"].as_str().unwrap();

    // none[]=banned → only the clean status.
    let timeline: Vec<Value> = ctx.api.get(
        "/api/v1/timelines/tag/nonefilter?none[]=banned",
        None,
    ).await.json().await.unwrap();

    let ids: Vec<&str> = timeline.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(ids.contains(&clean_id), "status without banned tag should appear");
    assert!(!ids.contains(&with_banned_id), "status with banned tag should be excluded by none[]");
}

/// A reblog by a followed account appears in the home timeline.
#[tokio::test]
async fn test_reblog_appears_in_home_timeline() {
    let ctx = TestContext::new("home-reblog").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let alice_status = ctx.api.post_status(&ctx.alice_token, "alice original for reblog", "public").await;
    let alice_status_id = alice_status["id"].as_str().unwrap();

    let reblog: Value = ctx.api.post_json(
        &format!("/api/v1/statuses/{alice_status_id}/reblog"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await.json().await.unwrap();
    let reblog_id = reblog["id"].as_str().unwrap();

    let home = ctx.api.home_timeline(&ctx.alice_token).await;
    assert!(
        home.iter().any(|s| s["id"].as_str() == Some(reblog_id)),
        "Bob's reblog should appear in Alice's home timeline",
    );
}

// ── min_id pagination ──────────────────────────────────────────────────────────

/// Public timeline: min_id returns statuses *after* that id in ascending order.
#[tokio::test]
async fn test_public_timeline_min_id_pagination() {
    let ctx = TestContext::new("pub-min-id").await;

    let s1 = ctx.api.post_status(&ctx.alice_token, "public min_id 1", "public").await;
    let s2 = ctx.api.post_status(&ctx.alice_token, "public min_id 2", "public").await;
    let s3 = ctx.api.post_status(&ctx.alice_token, "public min_id 3", "public").await;

    let min_id = s1["id"].as_str().unwrap();

    let resp = ctx.api.get(
        &format!("/api/v1/timelines/public?local=true&min_id={min_id}"),
        None,
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let timeline: Vec<Value> = resp.json().await.unwrap();
    let ids: Vec<&str> = timeline.iter().filter_map(|s| s["id"].as_str()).collect();

    assert!(!ids.contains(&min_id), "min_id anchor itself should not appear");
    assert!(ids.contains(&s2["id"].as_str().unwrap()), "s2 should appear after min_id");
    assert!(ids.contains(&s3["id"].as_str().unwrap()), "s3 should appear after min_id");

    // Results should be in ascending order (oldest first).
    let s2_pos = ids.iter().position(|&id| id == s2["id"].as_str().unwrap()).unwrap();
    let s3_pos = ids.iter().position(|&id| id == s3["id"].as_str().unwrap()).unwrap();
    assert!(s2_pos < s3_pos, "min_id results should be in ascending order");
}

/// Home timeline: min_id returns statuses after that id in ascending order.
#[tokio::test]
async fn test_home_timeline_min_id_pagination() {
    let ctx = TestContext::new("home-min-id").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let s1 = ctx.api.post_status(&ctx.bob_token, "home min_id 1", "public").await;
    let s2 = ctx.api.post_status(&ctx.bob_token, "home min_id 2", "public").await;
    let s3 = ctx.api.post_status(&ctx.bob_token, "home min_id 3", "public").await;

    let min_id = s1["id"].as_str().unwrap();

    let resp = ctx.api.get(
        &format!("/api/v1/timelines/home?min_id={min_id}"),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let timeline: Vec<Value> = resp.json().await.unwrap();
    let ids: Vec<&str> = timeline.iter().filter_map(|s| s["id"].as_str()).collect();

    assert!(!ids.contains(&min_id), "min_id anchor itself should not appear");
    assert!(ids.contains(&s2["id"].as_str().unwrap()), "s2 should appear");
    assert!(ids.contains(&s3["id"].as_str().unwrap()), "s3 should appear");

    let s2_pos = ids.iter().position(|&id| id == s2["id"].as_str().unwrap()).unwrap();
    let s3_pos = ids.iter().position(|&id| id == s3["id"].as_str().unwrap()).unwrap();
    assert!(s2_pos < s3_pos, "home min_id results should be in ascending order");
}

/// Tag timeline: min_id returns statuses after that id in ascending order.
#[tokio::test]
async fn test_tag_timeline_min_id_pagination() {
    let ctx = TestContext::new("tag-min-id").await;

    let s1 = ctx.api.post_status(&ctx.alice_token, "tag min_id #minidtest 1", "public").await;
    let s2 = ctx.api.post_status(&ctx.alice_token, "tag min_id #minidtest 2", "public").await;
    let s3 = ctx.api.post_status(&ctx.alice_token, "tag min_id #minidtest 3", "public").await;

    let min_id = s1["id"].as_str().unwrap();

    let resp = ctx.api.get(
        &format!("/api/v1/timelines/tag/minidtest?min_id={min_id}"),
        None,
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let timeline: Vec<Value> = resp.json().await.unwrap();
    let ids: Vec<&str> = timeline.iter().filter_map(|s| s["id"].as_str()).collect();

    assert!(!ids.contains(&min_id), "min_id anchor itself should not appear");
    assert!(ids.contains(&s2["id"].as_str().unwrap()), "s2 should appear after tag min_id");
    assert!(ids.contains(&s3["id"].as_str().unwrap()), "s3 should appear after tag min_id");

    let s2_pos = ids.iter().position(|&id| id == s2["id"].as_str().unwrap()).unwrap();
    let s3_pos = ids.iter().position(|&id| id == s3["id"].as_str().unwrap()).unwrap();
    assert!(s2_pos < s3_pos, "tag min_id results should be in ascending order");
}

/// Home timeline hides reblogs from accounts followed with show_reblogs=false.
#[tokio::test]
async fn test_home_timeline_hides_reblogs_when_show_reblogs_false() {
    let ctx = TestContext::new("home-no-reblogs").await;

    // Alice follows Bob with show_reblogs=false.
    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/follow", ctx.bob_id),
        Some(&ctx.alice_token),
        &serde_json::json!({"reblogs": false}),
    ).await;

    // Bob posts a status, then Alice reblogs it (Bob reblogs his own).
    let original = ctx.api.post_status(&ctx.alice_token, "original for reblog test", "public").await;
    let original_id = original["id"].as_str().unwrap();

    // Bob reblogs alice's status.
    ctx.api.post_json(
        &format!("/api/v1/statuses/{original_id}/reblog"),
        Some(&ctx.bob_token),
        &serde_json::json!({}),
    ).await;

    // Alice's home timeline should not contain the reblog from Bob.
    let timeline: Vec<Value> = ctx.api.get("/api/v1/timelines/home", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    let has_bob_reblog = timeline.iter().any(|s| {
        s["account"]["id"].as_str() == Some(&ctx.bob_id)
        && s["reblog"].is_object()
    });
    assert!(!has_bob_reblog, "reblog from bob should be hidden when show_reblogs=false");
}

/// Home timeline hides statuses from muted accounts.
#[tokio::test]
async fn test_home_timeline_hides_muted_accounts() {
    let ctx = TestContext::new("home-muted").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let status = ctx.api.post_status(&ctx.bob_token, "bob status before mute", "public").await;
    let status_id = status["id"].as_str().unwrap();

    // Verify it appears before mute.
    let before: Vec<Value> = ctx.api.get("/api/v1/timelines/home", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(
        before.iter().any(|s| s["id"].as_str() == Some(status_id)),
        "bob's status should appear before mute",
    );

    // Alice mutes Bob.
    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/mute", ctx.bob_id),
        Some(&ctx.alice_token),
        &serde_json::json!({}),
    ).await;

    let after: Vec<Value> = ctx.api.get("/api/v1/timelines/home", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(
        !after.iter().any(|s| s["account"]["id"].as_str() == Some(&ctx.bob_id)),
        "bob's statuses should be hidden from home timeline after mute",
    );
}

/// Public timeline returns favourited=true for statuses already liked by the authenticated viewer.
#[tokio::test]
async fn test_public_timeline_viewer_context() {
    let ctx = TestContext::new("pub-tl-viewer").await;

    let status = ctx.api.post_status(&ctx.alice_token, "public for viewer context", "public").await;
    let status_id = status["id"].as_str().unwrap();

    // Bob favourites it.
    ctx.api.post_json(
        &format!("/api/v1/statuses/{status_id}/favourite"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;

    // Fetch public timeline as Bob — the status should show favourited=true.
    let timeline: Vec<Value> = ctx.api.get("/api/v1/timelines/public", Some(&ctx.bob_token))
        .await.json().await.unwrap();

    let found = timeline.iter().find(|s| s["id"].as_str() == Some(status_id));
    assert!(found.is_some(), "status not found in public timeline");
    assert_eq!(
        found.unwrap()["favourited"].as_bool(),
        Some(true),
        "authenticated viewer should see favourited=true for a status they liked",
    );
}

/// Public statuses with a followed tag appear on the home timeline even from non-followed accounts.
#[tokio::test]
async fn test_home_timeline_includes_followed_tag_statuses() {
    let ctx = TestContext::new("home-tl-tag").await;

    // Alice follows #rustlang, Bob does not follow Alice.
    ctx.api.post_json(
        "/api/v1/tags/rustlang/follow",
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    // Bob posts a public status with #rustlang.
    let status = ctx.api.post_status(&ctx.bob_token, "I love #rustlang today", "public").await;
    let status_id = status["id"].as_str().unwrap();

    // The status should appear on Alice's home timeline even though she doesn't follow Bob.
    let home: Vec<Value> = ctx.api.get("/api/v1/timelines/home", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(
        home.iter().any(|s| s["id"].as_str() == Some(status_id)),
        "status with followed tag should appear in home timeline",
    );
}

/// Authenticated viewer does not see blocked account's statuses on public timeline.
#[tokio::test]
async fn test_public_timeline_hides_blocked_accounts() {
    let ctx = TestContext::new("pub-tl-block").await;

    let status = ctx.api.post_status(&ctx.bob_token, "block-me-public post", "public").await;
    let status_id = status["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/block", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    let timeline: Vec<Value> = ctx.api.get("/api/v1/timelines/public", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(
        !timeline.iter().any(|s| s["id"].as_str() == Some(status_id)),
        "blocked account's statuses should be hidden from public timeline",
    );
}

/// Authenticated viewer does not see blocking account's statuses on public timeline.
#[tokio::test]
async fn test_public_timeline_hides_accounts_that_blocked_viewer() {
    let ctx = TestContext::new("pub-tl-blocked-by").await;

    let status = ctx.api.post_status(&ctx.bob_token, "bob-blocked-alice post", "public").await;
    let status_id = status["id"].as_str().unwrap();

    // Bob blocks Alice.
    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/block", ctx.alice_id),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;

    let timeline: Vec<Value> = ctx.api.get("/api/v1/timelines/public", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(
        !timeline.iter().any(|s| s["id"].as_str() == Some(status_id)),
        "statuses from accounts that blocked the viewer should be hidden from public timeline",
    );
}

/// Statuses with a followed tag do NOT appear on home timeline if the account is muted.
#[tokio::test]
async fn test_home_timeline_followed_tag_muted_account_excluded() {
    let ctx = TestContext::new("home-tl-tag-mute").await;

    ctx.api.post_json(
        "/api/v1/tags/mutedtag42/follow",
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    // Alice mutes Bob.
    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/mute", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    // Bob posts a public status with #mutedtag42.
    let status = ctx.api.post_status(&ctx.bob_token, "muted but tagged #mutedtag42", "public").await;
    let status_id = status["id"].as_str().unwrap();

    let home: Vec<Value> = ctx.api.get("/api/v1/timelines/home", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(
        !home.iter().any(|s| s["id"].as_str() == Some(status_id)),
        "muted account's status should not appear in home timeline even if it has a followed tag",
    );
}

/// A "hide" filter removes matching statuses from the home timeline.
#[tokio::test]
async fn test_home_timeline_hide_filter_excludes_matching_status() {
    let ctx = TestContext::new("filter-hide-home").await;

    // Create a hide filter for the word "badword"
    let filter_resp: Value = ctx.api.post_json(
        "/api/v2/filters",
        Some(&ctx.alice_token),
        &json!({
            "title": "Hide bad words",
            "context": ["home"],
            "filter_action": "hide",
            "keywords_attributes": [{"keyword": "badword", "whole_word": false}]
        }),
    ).await.json().await.unwrap();

    let filter_id = filter_resp["id"].as_str().unwrap();
    assert!(!filter_id.is_empty(), "filter should be created");

    // Bob posts a status containing "badword"
    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let bad_status = ctx.api.post_status(&ctx.bob_token, "this has badword in it", "public").await;
    let bad_id = bad_status["id"].as_str().unwrap();

    // Bob posts a clean status
    let clean_status = ctx.api.post_status(&ctx.bob_token, "this is fine", "public").await;
    let clean_id = clean_status["id"].as_str().unwrap();

    let home: Vec<Value> = ctx.api.get("/api/v1/timelines/home", Some(&ctx.alice_token))
        .await.json().await.unwrap();

    let ids: Vec<&str> = home.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(
        !ids.contains(&bad_id),
        "status with filtered word should be excluded from home timeline",
    );
    assert!(
        ids.contains(&clean_id),
        "clean status should still appear in home timeline",
    );
}

/// A direct (DM) status from a followed account should NOT appear in the home
/// timeline unless the viewer is mentioned in it.
#[tokio::test]
async fn test_home_timeline_excludes_unaddressed_dms() {
    let ctx = TestContext::new("home-dm-filter").await;

    // Alice follows Bob.
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    // Bob sends a DM to himself (Alice is NOT mentioned).
    let dm = ctx.api.post_status(&ctx.bob_token, "private thought", "direct").await;
    let dm_id = dm["id"].as_str().unwrap();

    let home: Vec<Value> = ctx.api.get("/api/v1/timelines/home", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    let ids: Vec<&str> = home.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(
        !ids.contains(&dm_id),
        "DM not addressed to viewer should not appear in home timeline",
    );
}

/// A direct (DM) status mentioning the viewer should appear in their home timeline.
#[tokio::test]
async fn test_home_timeline_includes_addressed_dms() {
    let ctx = TestContext::new("home-dm-mention").await;

    // Alice follows Bob.
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    // Bob DMs Alice (mentions her).
    let dm = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.bob_token),
        &json!({
            "status": "@alice hello",
            "visibility": "direct",
        }),
    ).await.json::<Value>().await.unwrap();
    let dm_id = dm["id"].as_str().unwrap();

    let home: Vec<Value> = ctx.api.get("/api/v1/timelines/home", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    let ids: Vec<&str> = home.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(
        ids.contains(&dm_id),
        "DM addressed to viewer should appear in home timeline",
    );
}

/// Home timeline excludes statuses from blocked accounts (even via hashtag follows).
#[tokio::test]
async fn test_home_timeline_hides_blocked_accounts() {
    let ctx = TestContext::new("home-tl-blocked").await;

    // Alice follows Bob, and also follows #blocktest tag.
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;
    ctx.api.post_json("/api/v1/tags/blocktest/follow", Some(&ctx.alice_token), &json!({})).await;

    // Bob posts a public status with the followed tag.
    let status = ctx.api.post_status(&ctx.bob_token, "blocking test #blocktest", "public").await;
    let status_id = status["id"].as_str().unwrap();

    // Verify the status appears before the block.
    let before: Vec<Value> = ctx.api.get("/api/v1/timelines/home", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(before.iter().any(|s| s["id"].as_str() == Some(status_id)),
        "status should appear in home timeline before block");

    // Alice blocks Bob.
    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/block", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    // Bob's status should no longer appear in Alice's home timeline.
    let after: Vec<Value> = ctx.api.get("/api/v1/timelines/home", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(!after.iter().any(|s| s["account"]["id"].as_str() == Some(ctx.bob_id.as_str())),
        "blocked account's status should be hidden from home timeline");
}

/// A boost of a blocked account's status does not appear in the home timeline.
#[tokio::test]
async fn test_home_timeline_hides_boosts_of_blocked_account() {
    let ctx = TestContext::new("home-tl-boost-blocked").await;

    // Seed a third user: Charlie.
    let (charlie_uuid, charlie_token) =
        super::helpers::seed_user(&ctx.db, &ctx.domain, "charlie", "charlie@test.invalid").await;
    let charlie_id = charlie_uuid.to_string();

    // Alice follows Bob.
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    // Charlie posts a public status.
    let charlie_status = ctx.api.post_status(&charlie_token, "charlies post to boost", "public").await;
    let charlie_status_id = charlie_status["id"].as_str().unwrap();

    // Bob boosts Charlie's status.
    let boost: Value = ctx.api.post_json(
        &format!("/api/v1/statuses/{charlie_status_id}/reblog"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await.json().await.unwrap();
    let boost_id = boost["id"].as_str().unwrap();

    // Before block: Bob's boost should appear in Alice's home timeline.
    let before: Vec<Value> = ctx.api.get("/api/v1/timelines/home", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(before.iter().any(|s| s["id"].as_str() == Some(boost_id)),
        "Bob's boost should appear before Alice blocks Charlie");

    // Alice blocks Charlie.
    ctx.api.post_json(
        &format!("/api/v1/accounts/{charlie_id}/block"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    // After block: the boost should be hidden even though it's from Bob (who is not blocked).
    let after: Vec<Value> = ctx.api.get("/api/v1/timelines/home", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(!after.iter().any(|s| s["id"].as_str() == Some(boost_id)),
        "boost of blocked account's status should be hidden from home timeline");
}

/// Authenticated viewer does not see muted account's statuses on public timeline.
#[tokio::test]
async fn test_public_timeline_hides_muted_accounts() {
    let ctx = TestContext::new("pub-tl-muted").await;

    let status = ctx.api.post_status(&ctx.bob_token, "mute-me-public post", "public").await;
    let status_id = status["id"].as_str().unwrap();

    // Status is visible before mute.
    let before: Vec<Value> = ctx.api.get("/api/v1/timelines/public", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(
        before.iter().any(|s| s["id"].as_str() == Some(status_id)),
        "bob's status should appear in public timeline before mute",
    );

    ctx.api.post_json(
        &format!("/api/v1/accounts/{}/mute", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    let after: Vec<Value> = ctx.api.get("/api/v1/timelines/public", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(
        !after.iter().any(|s| s["id"].as_str() == Some(status_id)),
        "muted account's statuses should be hidden from public timeline",
    );
}

/// A "warn" filter in home context keeps the status but populates the filtered field.
#[tokio::test]
async fn test_home_timeline_warn_filter_annotates_status() {
    let ctx = TestContext::new("home-tl-warn").await;

    // Alice follows Bob.
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    // Alice creates a "home" warn filter for "warnableword".
    ctx.api.post_json(
        "/api/v2/filters",
        Some(&ctx.alice_token),
        &json!({
            "title": "Home warn filter",
            "context": ["home"],
            "filter_action": "warn",
            "keywords_attributes": [{"keyword": "warnableword", "whole_word": false}]
        }),
    ).await;

    // Bob posts a status containing the filtered word.
    let warned_status = ctx.api.post_status(&ctx.bob_token, "post about warnableword stuff", "public").await;
    let warned_id = warned_status["id"].as_str().unwrap();

    let home: Vec<Value> = ctx.api.get("/api/v1/timelines/home", Some(&ctx.alice_token))
        .await.json().await.unwrap();

    let found = home.iter().find(|s| s["id"].as_str() == Some(warned_id));
    assert!(found.is_some(), "warned status should still appear in home timeline");
    let filtered = found.unwrap()["filtered"].as_array().unwrap();
    assert!(!filtered.is_empty(), "warned status should have non-empty filtered array");
    assert_eq!(
        filtered[0]["filter"]["filter_action"].as_str(),
        Some("warn"),
        "filtered entry should reference the warn filter",
    );
}

/// Statuses with reply=true and a NULL in_reply_to_id (parent hard-deleted, as in
/// Mastodon migrations) must not appear in the public timeline.
///
/// Regression: the old filter `in_reply_to_id IS NULL OR in_reply_to_account_id = account_id`
/// incorrectly included such statuses because after FK cascade in_reply_to_id becomes NULL.
/// The new filter uses the persistent `reply` boolean.
#[tokio::test]
async fn test_public_timeline_excludes_reply_with_deleted_parent() {
    let ctx = TestContext::new("pub-reply-deleted-parent").await;

    // Bob posts a public status that will become an "orphaned reply".
    let status = ctx.api.post_status(&ctx.bob_token, "orphaned reply post", "public").await;
    let status_id: i64 = status["id"].as_str().unwrap().parse().unwrap();

    // The status currently has reply=false and appears in the timeline.
    let before: Vec<Value> = ctx.api.get("/api/v1/timelines/public", None)
        .await.json().await.unwrap();
    assert!(
        before.iter().any(|s| s["id"].as_str().map(|id| id.parse::<i64>().ok()) == Some(Some(status_id))),
        "status should appear in public timeline before reply flag is set",
    );

    // Simulate a migrated status whose parent was hard-deleted in Mastodon:
    // reply=true but in_reply_to_id=NULL (FK cascade already happened).
    ctx.db.execute(
        sqlx::query("UPDATE statuses SET reply = true, in_reply_to_id = NULL WHERE id = $1")
            .bind(status_id),
    ).await.unwrap();

    let after: Vec<Value> = ctx.api.get("/api/v1/timelines/public", None)
        .await.json().await.unwrap();
    assert!(
        !after.iter().any(|s| s["id"].as_str().map(|id| id.parse::<i64>().ok()) == Some(Some(status_id))),
        "status with reply=true and deleted parent should be excluded from public timeline",
    );
}
