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
    let domain = form.domain.to_lowercase();
    sqlx::query!(
        r#"INSERT INTO user_domain_blocks (account_id, domain) VALUES ($1, $2)
           ON CONFLICT (account_id, domain) DO NOTHING"#,
        auth.account_id,
        domain,
    )
    .execute(&state.db)
    .await?;

    // Remove follows to and from accounts on the blocked domain
    let removed = sqlx::query!(
        r#"DELETE FROM follows
           WHERE (account_id = $1 AND target_account_id IN (
               SELECT id FROM accounts WHERE domain = $2
           ))
           OR (target_account_id = $1 AND account_id IN (
               SELECT id FROM accounts WHERE domain = $2
           ))
           AND state = 'accepted'
           RETURNING account_id, target_account_id"#,
        auth.account_id,
        domain,
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    for row in &removed {
        let _ = sqlx::query!(
            "UPDATE accounts SET following_count = GREATEST(following_count - 1, 0) WHERE id = $1",
            row.account_id
        ).execute(&state.db).await;
        let _ = sqlx::query!(
            "UPDATE accounts SET followers_count = GREATEST(followers_count - 1, 0) WHERE id = $1",
            row.target_account_id
        ).execute(&state.db).await;
    }

    Ok(Json(serde_json::json!({})))
}

// ── GET /api/v1/domain_blocks/preview ────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DomainPreviewQuery {
    pub domain: Option<String>,
}

pub async fn preview_domain_block(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Query(q): Query<DomainPreviewQuery>,
) -> AppResult<Json<serde_json::Value>> {
    auth.require_scope("write:blocks")?;
    let domain = q.domain.as_deref().unwrap_or("").to_lowercase();

    let following_count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) FROM follows f
           JOIN accounts a ON a.id = f.target_account_id
           WHERE f.account_id = $1 AND a.domain = $2 AND f.state = 'accepted'"#,
        auth.account_id, domain,
    )
    .fetch_one(&state.db)
    .await?
    .unwrap_or(0);

    let followers_count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) FROM follows f
           JOIN accounts a ON a.id = f.account_id
           WHERE f.target_account_id = $1 AND a.domain = $2 AND f.state = 'accepted'"#,
        auth.account_id, domain,
    )
    .fetch_one(&state.db)
    .await?
    .unwrap_or(0);

    Ok(Json(serde_json::json!({
        "following_count": following_count,
        "followers_count": followers_count,
    })))
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
