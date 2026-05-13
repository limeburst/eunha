use reqwest::StatusCode;
use serde_json::{json, Value};

use super::helpers::TestContext;

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
