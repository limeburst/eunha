/// Serves the Elk Mastodon web client for instance domains.
///
/// - Static assets (JS, CSS, images) are served directly from `elk/.output/public/`.
/// - All other paths fall back to `index.html` (SPA client-side routing).
/// - `index.html` has `window.__eunha_instance = "<domain>"` injected before `</head>`
///   so the `eunha.client.ts` Elk plugin can auto-configure the instance without
///   showing the server selector.
use axum::{
    http::{header, HeaderMap, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
};

const DIST: &str = "elk/.output/public";

pub async fn serve(uri: Uri, headers: HeaderMap) -> Response {
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

    serve_index(&headers).await
}

async fn serve_index(headers: &HeaderMap) -> Response {
    let domain = domain_from_headers(headers);

    let Ok(html) = tokio::fs::read_to_string(format!("{DIST}/index.html")).await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Elk is not built yet. Run `make elk` to build it.",
        )
            .into_response();
    };

    let script = format!(r#"<script>window.__eunha_instance="{domain}"</script>"#);
    Html(html.replacen("</head>", &format!("{script}</head>"), 1)).into_response()
}

fn domain_from_headers(headers: &HeaderMap) -> String {
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.split(':').next())
        .unwrap_or("localhost")
        .to_string()
}
