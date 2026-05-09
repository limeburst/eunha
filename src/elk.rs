/// Serves the Elk Mastodon web client for instance domains.
///
/// Elk itself is a fork at elk/ with source patches:
/// - `singleInstance: true` baked into nuxt.config.ts
/// - `defaultServer` resolved from `window.location.hostname` at runtime
/// - An "Account settings" entry in the /settings nav that links to the server-rendered /account pages
use axum::{
    http::{header, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
};

const DIST: &str = "elk/.output/public";

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Serve static assets (JS, CSS, fonts, images, manifest, etc.) directly.
    // Path traversal guard: reject anything with "..".
    if !path.is_empty() && !path.contains("..") {
        let file_path = format!("{DIST}/{path}");
        if let Ok(bytes) = tokio::fs::read(&file_path).await {
            let mime = mime_guess::from_path(&file_path)
                .first_or_octet_stream()
                .to_string();
            return ([(header::CONTENT_TYPE, mime)], bytes).into_response();
        }
    }

    serve_index().await
}

async fn serve_index() -> Response {
    let Ok(html) = tokio::fs::read_to_string(format!("{DIST}/index.html")).await else {
        return (StatusCode::SERVICE_UNAVAILABLE, "Elk is not built yet.").into_response();
    };

    Html(html).into_response()
}
