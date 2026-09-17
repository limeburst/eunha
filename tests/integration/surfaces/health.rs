use reqwest::StatusCode;
use serde_json::Value;

use crate::helpers::TestContext;

/// The health endpoint answers without a token: a control plane polling it has
/// no account on the instance it is checking.
#[tokio::test]
async fn test_health_needs_no_auth() {
    let ctx = TestContext::new("health-anon").await;
    let resp = ctx.api.get("/api/eunha/v1/health", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
}
