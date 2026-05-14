use axum::{
    extract::{Extension, Query, State},
    http::{header, HeaderMap},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use crate::{
    error::AppResult,
    middleware::AuthenticatedUser,
    state::AppState,
};
use super::types::PaginationParams;

// ── GET /api/v1/domain_blocks ─────────────────────────────────────────────

pub async fn get_domain_blocks(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Query(q): Query<PaginationParams>,
) -> AppResult<impl IntoResponse> {
    auth.require_scope("read:blocks")?;
    let limit = q.limit_clamped(100, 200);
    let max_id = q.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let rows = sqlx::query!(
        r#"SELECT id, domain FROM user_domain_blocks
           WHERE account_id = $1
             AND ($2::bigint IS NULL OR id < $2)
           ORDER BY id DESC LIMIT $3"#,
        auth.account_id,
        max_id,
        limit,
    )
    .fetch_all(&state.db)
    .await?;

    let domains: Vec<String> = rows.iter().map(|r| r.domain.clone()).collect();

    let mut resp_headers = HeaderMap::new();
    if let (Some(first), Some(last)) = (rows.first(), rows.last()) {
        let link = format!(
            r#"</api/v1/domain_blocks?max_id={}>; rel="next", </api/v1/domain_blocks?since_id={}>; rel="prev""#,
            last.id, first.id
        );
        if let Ok(val) = link.parse() {
            resp_headers.insert(header::LINK, val);
        }
    }

    Ok((resp_headers, Json(domains)))
}

// ── POST /api/v1/domain_blocks ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DomainBlockForm {
    pub domain: String,
}

pub async fn block_domain(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<DomainBlockForm>,
) -> AppResult<Json<serde_json::Value>> {
    auth.require_scope("write:blocks")?;
    sqlx::query!(
        r#"INSERT INTO user_domain_blocks (account_id, domain) VALUES ($1, $2)
           ON CONFLICT (account_id, domain) DO NOTHING"#,
        auth.account_id,
        form.domain.to_lowercase(),
    )
    .execute(&state.db)
    .await?;

    Ok(Json(serde_json::json!({})))
}

// ── DELETE /api/v1/domain_blocks ─────────────────────────────────────────

pub async fn unblock_domain(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<DomainBlockForm>,
) -> AppResult<Json<serde_json::Value>> {
    auth.require_scope("write:blocks")?;
    sqlx::query!(
        "DELETE FROM user_domain_blocks WHERE account_id = $1 AND domain = $2",
        auth.account_id,
        form.domain.to_lowercase(),
    )
    .execute(&state.db)
    .await?;

    Ok(Json(serde_json::json!({})))
}
