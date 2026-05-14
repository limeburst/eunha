use reqwest::StatusCode;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

use super::helpers::TestContext;

/// Elevate alice to admin role for tests that need admin privileges.
async fn make_admin(ctx: &TestContext) {
    let alice_uuid: Uuid = ctx.alice_id.parse().unwrap();
    let db_url = std::env::var("DATABASE_URL").unwrap();
    let db = PgPoolOptions::new().max_connections(2).connect(&db_url).await.unwrap();
    sqlx::query!(
        "UPDATE users SET role = 'admin' WHERE account_id = $1",
        alice_uuid,
    )
    .execute(&db)
    .await
    .unwrap();
}

/// Non-admin token gets 403 from admin endpoints.
#[tokio::test]
async fn test_admin_requires_admin_role() {
    let ctx = TestContext::new("admin-403").await;

    let resp = ctx.api.get("/api/v1/admin/accounts", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "non-admin should get 403");
}

/// GET /api/v1/admin/accounts returns all accounts in the instance.
#[tokio::test]
async fn test_admin_list_accounts() {
    let ctx = TestContext::new("admin-list").await;
    make_admin(&ctx).await;

    let resp = ctx.api.get("/api/v1/admin/accounts", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(!list.is_empty(), "expected at least alice and bob in admin accounts");
    // All entries should have an id and username.
    for acc in &list {
        assert!(acc["id"].as_str().is_some(), "admin account missing id: {acc}");
        assert!(acc["username"].as_str().is_some(), "admin account missing username: {acc}");
    }
}

/// GET /api/v1/admin/accounts/:id returns a specific account.
#[tokio::test]
async fn test_admin_get_account() {
    let ctx = TestContext::new("admin-get").await;
    make_admin(&ctx).await;

    let resp = ctx.api.get(
        &format!("/api/v1/admin/accounts/{}", ctx.bob_id),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let acc: Value = resp.json().await.unwrap();
    assert_eq!(acc["id"].as_str(), Some(ctx.bob_id.as_str()));
    assert_eq!(acc["username"].as_str(), Some("bob"));
    assert!(acc["account"].is_object(), "nested account object missing");
}

/// POST /api/v1/admin/accounts/:id/silence and unsilence toggle silenced state.
#[tokio::test]
async fn test_admin_silence_and_unsilence() {
    let ctx = TestContext::new("admin-silence").await;
    make_admin(&ctx).await;

    let silence_resp = ctx.api.post_json(
        &format!("/api/v1/admin/accounts/{}/silence", ctx.bob_id),
        Some(&ctx.alice_token),
        &serde_json::json!({}),
    ).await;
    assert_eq!(silence_resp.status(), StatusCode::OK);
    let silenced: Value = silence_resp.json().await.unwrap();
    assert_eq!(silenced["silenced"].as_bool(), Some(true), "silenced should be true after silence");

    let unsilence_resp = ctx.api.post_json(
        &format!("/api/v1/admin/accounts/{}/unsilence", ctx.bob_id),
        Some(&ctx.alice_token),
        &serde_json::json!({}),
    ).await;
    assert_eq!(unsilence_resp.status(), StatusCode::OK);
    let unsilenced: Value = unsilence_resp.json().await.unwrap();
    assert_eq!(unsilenced["silenced"].as_bool(), Some(false), "silenced should be false after unsilence");
}

/// POST /api/v1/admin/accounts/:id/suspend and unsuspend toggle suspended state.
#[tokio::test]
async fn test_admin_suspend_and_unsuspend() {
    let ctx = TestContext::new("admin-suspend").await;
    make_admin(&ctx).await;

    let suspend_resp = ctx.api.post_json(
        &format!("/api/v1/admin/accounts/{}/suspend", ctx.bob_id),
        Some(&ctx.alice_token),
        &serde_json::json!({}),
    ).await;
    assert_eq!(suspend_resp.status(), StatusCode::OK);
    let suspended: Value = suspend_resp.json().await.unwrap();
    assert_eq!(suspended["suspended"].as_bool(), Some(true));

    let unsuspend_resp = ctx.api.post_json(
        &format!("/api/v1/admin/accounts/{}/unsuspend", ctx.bob_id),
        Some(&ctx.alice_token),
        &serde_json::json!({}),
    ).await;
    assert_eq!(unsuspend_resp.status(), StatusCode::OK);
    let unsuspended: Value = unsuspend_resp.json().await.unwrap();
    assert_eq!(unsuspended["suspended"].as_bool(), Some(false));
}

/// GET /api/v1/admin/reports returns a list (empty when no reports filed).
#[tokio::test]
async fn test_admin_list_reports_empty() {
    let ctx = TestContext::new("admin-reports").await;
    make_admin(&ctx).await;

    let resp = ctx.api.get("/api/v1/admin/reports", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let _: Vec<Value> = resp.json().await.unwrap();
}

/// GET /api/v1/admin/roles returns the standard roles list.
#[tokio::test]
async fn test_admin_list_roles() {
    let ctx = TestContext::new("admin-roles").await;
    make_admin(&ctx).await;

    let resp = ctx.api.get("/api/v1/admin/roles", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let roles: Vec<Value> = resp.json().await.unwrap();
    assert!(!roles.is_empty(), "expected at least one role");
}
