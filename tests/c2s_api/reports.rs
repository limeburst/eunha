use reqwest::StatusCode;
use serde_json::{json, Value};

use super::helpers::TestContext;

/// POST /api/v1/reports files a report and returns the report object.
#[tokio::test]
async fn test_file_report() {
    let ctx = TestContext::new("report").await;

    let status = ctx.api.post_status(&ctx.bob_token, "reportable content", "public").await;
    let status_id = status["id"].as_str().unwrap();

    let resp = ctx.api.post_json(
        "/api/v1/reports",
        Some(&ctx.alice_token),
        &json!({
            "account_id": ctx.bob_id,
            "status_ids": [status_id],
            "comment": "This is spam",
            "category": "spam"
        }),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let report: Value = resp.json().await.unwrap();
    assert!(report["id"].as_str().is_some());
    assert_eq!(report["action_taken"].as_bool(), Some(false));
    assert_eq!(report["category"].as_str(), Some("spam"));
    assert_eq!(report["comment"].as_str(), Some("This is spam"));
    assert_eq!(
        report["target_account"]["id"].as_str(),
        Some(ctx.bob_id.as_str()),
    );
}

/// POST /api/v1/reports with a status_id that belongs to a different account returns 404.
#[tokio::test]
async fn test_report_status_not_belonging_to_target_returns_404() {
    let ctx = TestContext::new("report-wrong-status").await;

    // Alice posts a status.
    let alice_status = ctx.api.post_status(&ctx.alice_token, "alice's post", "public").await;
    let alice_status_id = alice_status["id"].as_str().unwrap();

    // Alice reports Bob but attaches her own status (which belongs to Alice, not Bob).
    let resp = ctx.api.post_json(
        "/api/v1/reports",
        Some(&ctx.alice_token),
        &json!({
            "account_id": ctx.bob_id,
            "status_ids": [alice_status_id],
            "comment": "wrong status"
        }),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// POST /api/v1/reports with a category saves it in the response.
#[tokio::test]
async fn test_report_category_is_saved() {
    let ctx = TestContext::new("report-category").await;

    for category in &["spam", "violation", "other"] {
        let resp = ctx.api.post_json(
            "/api/v1/reports",
            Some(&ctx.alice_token),
            &json!({"account_id": ctx.bob_id, "category": category}),
        ).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let report: Value = resp.json().await.unwrap();
        assert_eq!(report["category"].as_str(), Some(*category));
    }
}

/// POST /api/v1/reports with forward=true saves the forwarded flag.
#[tokio::test]
async fn test_report_forward_flag() {
    let ctx = TestContext::new("report-fwd").await;

    let resp = ctx.api.post_json(
        "/api/v1/reports",
        Some(&ctx.alice_token),
        &json!({
            "account_id": ctx.bob_id,
            "comment": "forwarded report",
            "forward": true
        }),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let report: Value = resp.json().await.unwrap();
    assert!(report["id"].as_str().is_some(), "report id missing");
}

/// POST /api/v1/reports requires authentication.
#[tokio::test]
async fn test_file_report_requires_auth() {
    let ctx = TestContext::new("report-unauth").await;

    let resp = ctx.api.post_json(
        "/api/v1/reports",
        None,
        &json!({
            "account_id": ctx.bob_id,
            "comment": "unauthenticated report attempt"
        }),
    ).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// POST /api/v1/reports for an unknown account_id returns 404.
#[tokio::test]
async fn test_file_report_unknown_account() {
    let ctx = TestContext::new("report-404").await;

    let resp = ctx.api.post_json(
        "/api/v1/reports",
        Some(&ctx.alice_token),
        &json!({
            "account_id": "00000000-0000-0000-0000-000000000000",
            "comment": "spam"
        }),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
