/// Serves the Elk Mastodon web client for instance domains.
///
/// For each request, `index.html` is patched at serve time:
/// - The `defaultServer` value inside the `window.__NUXT__` inline script is replaced
///   with the instance domain from the Host header.
/// - The SHA384 hash of the modified script is computed and added to the CSP `script-src`
///   so the browser accepts the changed script.
///
/// This means no Elk source patches are needed — Elk's own plugin (`0.setup-users.ts`)
/// reads `defaultServer` from runtime config and configures itself correctly.
use axum::{
    http::{header, HeaderMap, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
};
use base64::Engine;
use sha2::{Digest, Sha384};

const DIST: &str = "elk/.output/public";
const DEFAULT_SERVER: &str = "m.webtoo.ls";

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
        return (StatusCode::SERVICE_UNAVAILABLE, "Elk is not built yet.").into_response();
    };

    Html(patch_index(&html, &domain)).into_response()
}

/// Replaces `defaultServer` in the `window.__NUXT__` inline script and updates the CSP hash.
fn patch_index(html: &str, domain: &str) -> String {
    // Find the window.__NUXT__ script (it contains the Nuxt runtime config).
    let Some(tag_start) = html.find("<script>window.__NUXT__=") else {
        return html.to_string();
    };
    let content_start = tag_start + "<script>".len();
    let Some(rel_end) = html[content_start..].find("</script>") else {
        return html.to_string();
    };
    let content_end = content_start + rel_end;

    let old_content = &html[content_start..content_end];
    let new_content = old_content.replace(
        &format!(r#""defaultServer":"{}""#, DEFAULT_SERVER),
        &format!(r#""defaultServer":"{}""#, domain),
    );

    if old_content == new_content {
        return html.to_string();
    }

    // Compute SHA384 of the new script content and add it to the CSP.
    let hash = Sha384::digest(new_content.as_bytes());
    let hash_b64 = base64::engine::general_purpose::STANDARD.encode(hash);
    let new_hash_src = format!("'sha384-{hash_b64}'");

    let patched = format!(
        "{}<script>{}</script>{}",
        &html[..tag_start],
        new_content,
        &html[content_end + "</script>".len()..],
    );

    add_csp_script_hash(&patched, &new_hash_src)
}

/// Appends `hash_src` to the `script-src` directive in the CSP `<meta>` tag.
fn add_csp_script_hash(html: &str, hash_src: &str) -> String {
    let Some(csp_pos) = html.find("http-equiv=\"Content-Security-Policy\"") else {
        return html.to_string();
    };
    let Some(rel_content) = html[csp_pos..].find("content=\"") else {
        return html.to_string();
    };
    let val_start = csp_pos + rel_content + "content=\"".len();
    let Some(rel_end) = html[val_start..].find('"') else {
        return html.to_string();
    };
    let val_end = val_start + rel_end;

    let csp = &html[val_start..val_end];
    let Some(ss_pos) = csp.find("script-src ") else {
        return html.to_string();
    };
    let after_ss = ss_pos + "script-src ".len();
    // Insert before the semicolon that ends script-src (or at end if none).
    let insert_at = csp[after_ss..]
        .find(';')
        .map(|i| after_ss + i)
        .unwrap_or(csp.len());

    let new_csp = format!("{} {}{}", &csp[..insert_at], hash_src, &csp[insert_at..]);

    format!("{}{}{}", &html[..val_start], new_csp, &html[val_end..])
}

fn domain_from_headers(headers: &HeaderMap) -> String {
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.split(':').next())
        .unwrap_or("localhost")
        .to_string()
}
