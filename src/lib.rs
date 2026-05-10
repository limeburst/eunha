pub mod api;
pub mod config;
pub mod console_frontend;
pub mod crypto;
pub mod db;
pub mod elk;
pub mod email;
pub mod error;
pub mod locale;
pub mod media;
pub mod middleware;
pub mod state;
pub mod streaming;
pub mod templates;
pub mod well_known;

use axum::{extract::Request, middleware as axum_middleware, response::IntoResponse, Router};
use std::sync::Arc;
use tower_http::{compression::CompressionLayer, cors::CorsLayer, trace::TraceLayer};

pub fn build_app(state: state::AppState) -> Router {
    let compressed = Router::new()
        .merge(well_known::router())
        .merge(api::mastodon::router(state.clone()))
        .merge(api::console::router(state.clone()))
        .merge(api::account::router(state.clone()))
        .merge(api::ap::router())
        .fallback({
            let console_domain = Arc::new(state.config.console_domain.clone());
            axum::routing::any(move |req: Request| {
                let console_domain = console_domain.clone();
                async move {
                    let host = req
                        .headers()
                        .get(axum::http::header::HOST)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|h| h.split(':').next())
                        .unwrap_or("");
                    let uri = req.uri().clone();
                    if host == console_domain.as_str() {
                        console_frontend::serve(uri).await
                    } else if uri.path().starts_with("/api/") {
                        (
                            axum::http::StatusCode::NOT_FOUND,
                            axum::Json(serde_json::json!({"error": "not found"})),
                        )
                            .into_response()
                    } else {
                        elk::serve(uri).await
                    }
                }
            })
        })
        .layer(CompressionLayer::new());

    Router::new()
        .merge(compressed)
        // Streaming WebSocket must be outside CompressionLayer to avoid body wrapping.
        .merge(api::mastodon::streaming_router())
        .layer(axum_middleware::from_fn(middleware::log_failures))
        .layer(axum_middleware::from_fn_with_state(state.clone(), middleware::authenticate))
        .layer(axum_middleware::from_fn_with_state(state.clone(), middleware::resolve_instance))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
