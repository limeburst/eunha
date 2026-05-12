use axum::{
    http::StatusCode,
    response::IntoResponse,
    Json,
};

// Push notifications are not implemented; return 404 for subscription operations
// so clients gracefully degrade to polling.

pub async fn get_subscription() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Push subscription not found"})))
}

pub async fn create_subscription() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Push notifications not supported"})))
}

pub async fn update_subscription() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Push subscription not found"})))
}

pub async fn delete_subscription() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Push subscription not found"})))
}
