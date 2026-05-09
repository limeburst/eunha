use axum::{
    http::{header, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
};

const DIST: &str = "console/dist";

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    if !path.is_empty() && !path.contains("..") {
        let file_path = format!("{DIST}/{path}");
        if let Ok(bytes) = tokio::fs::read(&file_path).await {
            let mime = mime_guess::from_path(&file_path)
                .first_or_octet_stream()
                .to_string();
            return ([(header::CONTENT_TYPE, mime)], bytes).into_response();
        }
    }

    let Ok(html) = tokio::fs::read_to_string(format!("{DIST}/index.html")).await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Console is not built yet. Run `mise run console` to build it.",
        )
            .into_response();
    };

    Html(html).into_response()
}
