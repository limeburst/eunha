mod api;
mod config;
mod console_frontend;
mod db;
mod elk;
mod error;
mod media;
mod middleware;
mod state;
mod streaming;
mod templates;
mod well_known;

use axum::{extract::Request, middleware as axum_middleware, Router};
use std::sync::Arc;
use sqlx::postgres::PgPoolOptions;
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            "eunha=debug,tower_http=info,sqlx=warn".into()
        }))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = config::Config::from_env()?;

    let db = PgPoolOptions::new()
        .max_connections(20)
        .connect(&config.database_url)
        .await?;

    sqlx::migrate!("./migrations").run(&db).await?;

    let state = state::AppState::new(db, config.clone());

    let app = Router::new()
        .merge(well_known::router())
        .merge(api::mastodon::router(state.clone()))
        .merge(api::ap::router())
        .fallback({
            let console_domain = Arc::new(config.console_domain.clone());
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
                    let headers = req.headers().clone();
                    if host == console_domain.as_str() {
                        console_frontend::serve(uri, headers).await
                    } else {
                        elk::serve(uri, headers).await
                    }
                }
            })
        })
        .layer(
            axum_middleware::from_fn_with_state(state.clone(), middleware::authenticate)
        )
        .layer(
            axum_middleware::from_fn_with_state(state.clone(), middleware::resolve_instance)
        )
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&config.bind_address).await?;
    tracing::info!("listening on {}", config.bind_address);
    axum::serve(listener, app).await?;

    Ok(())
}
