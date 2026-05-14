use reqwest::StatusCode;
use serde_json::{json, Value};
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

/// Admin can resolve and reopen a report.
#[tokio::test]
async fn test_admin_resolve_and_reopen_report() {
    let ctx = TestContext::new("admin-report-res").await;
    make_admin(&ctx).await;

    // Bob files a report against alice.
    let report_resp = ctx.api.post_json(
        "/api/v1/reports",
        Some(&ctx.bob_token),
        &json!({
            "account_id": ctx.alice_id,
            "comment": "test report for admin resolve"
        }),
    ).await;
    assert_eq!(report_resp.status(), StatusCode::OK);
    let report: Value = report_resp.json().await.unwrap();
    let report_id = report["id"].as_str().expect("report id missing");

    // Admin resolves it.
    let resolve_resp = ctx.api.post_json(
        &format!("/api/v1/admin/reports/{report_id}/resolve"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(resolve_resp.status(), StatusCode::OK);
    let resolved: Value = resolve_resp.json().await.unwrap();
    assert!(resolved["action_taken"].as_bool().unwrap_or(false), "action_taken should be true after resolve");

    // Reopen it.
    let reopen_resp = ctx.api.post_json(
        &format!("/api/v1/admin/reports/{report_id}/reopen"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(reopen_resp.status(), StatusCode::OK);
    let reopened: Value = reopen_resp.json().await.unwrap();
    assert!(!reopened["action_taken"].as_bool().unwrap_or(true), "action_taken should be false after reopen");
}

/// GET /api/v1/admin/reports/:id returns the specific report.
#[tokio::test]
async fn test_admin_get_report() {
    let ctx = TestContext::new("admin-get-report").await;
    make_admin(&ctx).await;

    let report: Value = ctx.api.post_json(
        "/api/v1/reports",
        Some(&ctx.bob_token),
        &json!({"account_id": ctx.alice_id, "comment": "admin get report test"}),
    ).await.json().await.unwrap();
    let report_id = report["id"].as_str().unwrap();

    let resp = ctx.api.get(
        &format!("/api/v1/admin/reports/{report_id}"),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["id"].as_str(), Some(report_id));
}

/// Admin domain allows: create, list, delete.
#[tokio::test]
async fn test_admin_domain_allows_crud() {
    let ctx = TestContext::new("admin-dallow").await;
    make_admin(&ctx).await;

    let create_resp = ctx.api.post_json(
        "/api/v1/admin/domain_allows",
        Some(&ctx.alice_token),
        &json!({"domain": "trusted.example.com"}),
    ).await;
    assert_eq!(create_resp.status(), StatusCode::OK);
    let allow: Value = create_resp.json().await.unwrap();
    let allow_id = allow["id"].as_str().expect("id missing");
    assert_eq!(allow["domain"].as_str(), Some("trusted.example.com"));

    let list: Vec<Value> = ctx.api.get("/api/v1/admin/domain_allows", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(list.iter().any(|a| a["id"].as_str() == Some(allow_id)), "created allow not in list");

    let del = ctx.api.delete(
        &format!("/api/v1/admin/domain_allows/{allow_id}"),
        &ctx.alice_token,
    ).await;
    assert_eq!(del.status(), StatusCode::OK);

    let after: Vec<Value> = ctx.api.get("/api/v1/admin/domain_allows", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(!after.iter().any(|a| a["id"].as_str() == Some(allow_id)), "deleted allow still in list");
}

/// Admin domain blocks: create, list, delete.
#[tokio::test]
async fn test_admin_domain_blocks_crud() {
    let ctx = TestContext::new("admin-dblock").await;
    make_admin(&ctx).await;

    let create_resp = ctx.api.post_json(
        "/api/v1/admin/domain_blocks",
        Some(&ctx.alice_token),
        &json!({
            "domain": "spam.example.com",
            "severity": "silence",
            "reject_media": true
        }),
    ).await;
    assert_eq!(create_resp.status(), StatusCode::OK);
    let block: Value = create_resp.json().await.unwrap();
    let block_id = block["id"].as_str().expect("id missing");
    assert_eq!(block["domain"].as_str(), Some("spam.example.com"));
    assert_eq!(block["severity"].as_str(), Some("silence"));
    assert_eq!(block["reject_media"].as_bool(), Some(true));

    let list: Vec<Value> = ctx.api.get("/api/v1/admin/domain_blocks", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(list.iter().any(|b| b["id"].as_str() == Some(block_id)), "created block not in list");

    let del = ctx.api.delete(
        &format!("/api/v1/admin/domain_blocks/{block_id}"),
        &ctx.alice_token,
    ).await;
    assert_eq!(del.status(), StatusCode::OK);

    let after: Vec<Value> = ctx.api.get("/api/v1/admin/domain_blocks", Some(&ctx.alice_token))
        .await.json().await.unwrap();
    assert!(!after.iter().any(|b| b["id"].as_str() == Some(block_id)), "deleted block still in list");
}
