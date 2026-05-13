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
