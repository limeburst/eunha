use axum::{
    extract::{Extension, Query, State},
    http::{header, HeaderMap, Uri},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use crate::{
    db::models::{Account, Status as DbStatus},
    error::AppResult,
    middleware::{AuthenticatedUser, ResolvedInstance},
    state::AppState,
};
use super::{
    accounts::fetch_status_media,
    convert::status_from_db,
    types::{PaginationParams, Status},
};

#[derive(Debug, Deserialize)]
pub struct PublicTimelineQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    pub local: Option<bool>,
    pub remote: Option<bool>,
    pub only_media: Option<bool>,
}

// ── GET /api/v1/timelines/public ──────────────────────────────────────────

pub async fn public_timeline(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    uri: Uri,
    req_headers: HeaderMap,
    Query(q): Query<PublicTimelineQuery>,
) -> AppResult<impl IntoResponse> {
    let limit = q.pagination.limit_clamped(20, 40);
    let max_id = q.pagination.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = q.pagination.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let local_only = q.local.unwrap_or(false);

    let statuses = sqlx::query_as!(
        DbStatus,
        r#"SELECT s.*
           FROM statuses s
           WHERE s.visibility = 'public'
             AND s.deleted_at IS NULL
             AND s.reblog_of_id IS NULL
             AND ($1::bool IS FALSE OR s.instance_id = $2)
             AND ($3::bigint IS NULL OR s.id < $3)
             AND ($4::bigint IS NULL OR s.id > $4)
             AND (s.text != '' OR s.content != ''
                  OR s.reblog_of_id IS NOT NULL
                  OR EXISTS (SELECT 1 FROM media_attachments WHERE status_id = s.id))
           ORDER BY s.id DESC
           LIMIT $5"#,
        local_only,
        instance.id,
        max_id,
        since_id,
        limit,
    )
    .fetch_all(&state.db)
    .await?;

    let result = build_status_list(&state, statuses, None).await?;
    let resp = with_pagination_link(&req_headers, &uri, result);
    Ok(resp)
}

// ── GET /api/v1/timelines/home ────────────────────────────────────────────

pub async fn home_timeline(
    State(state): State<AppState>,
    uri: Uri,
    req_headers: HeaderMap,
    Query(q): Query<PaginationParams>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<impl IntoResponse> {
    let limit = q.limit_clamped(20, 40);
    let max_id = q.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = q.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let statuses = sqlx::query_as!(
        DbStatus,
        r#"SELECT s.*
           FROM statuses s
           WHERE s.account_id IN (
               SELECT target_account_id FROM follows
               WHERE account_id = $1 AND state = 'accepted'
               UNION ALL SELECT $1
           )
           AND s.deleted_at IS NULL
           AND ($2::bigint IS NULL OR s.id < $2)
           AND ($3::bigint IS NULL OR s.id > $3)
           AND (s.text != '' OR s.content != ''
                OR s.reblog_of_id IS NOT NULL
                OR EXISTS (SELECT 1 FROM media_attachments WHERE status_id = s.id))
           ORDER BY s.id DESC
           LIMIT $4"#,
        auth.account_id,
        max_id,
        since_id,
        limit,
    )
    .fetch_all(&state.db)
    .await?;

    let result = build_status_list(&state, statuses, Some(auth.account_id)).await?;
    let resp = with_pagination_link(&req_headers, &uri, result);
    Ok(resp)
}

// ── Helpers ────────────────────────────────────────────────────────────────

async fn build_status_list(
    state: &AppState,
    statuses: Vec<DbStatus>,
    _viewer_id: Option<uuid::Uuid>,
) -> AppResult<Vec<Status>> {
    let mut result = Vec::with_capacity(statuses.len());
    for s in &statuses {
        let account = sqlx::query_as!(
            Account,
            "SELECT * FROM accounts WHERE id = $1",
            s.account_id
        )
        .fetch_one(&state.db)
        .await?;
        let media = fetch_status_media(state, s.id).await?;
        result.push(status_from_db(s, &account, media, None, None));
    }
    Ok(result)
}

fn with_pagination_link(
    req_headers: &HeaderMap,
    uri: &Uri,
    statuses: Vec<Status>,
) -> impl IntoResponse {
    let link = statuses.first().zip(statuses.last()).map(|(newest, oldest)| {
        let extra = super::non_pagination_query(uri.query());
        super::link_header(req_headers, uri.path(), &extra, &newest.id, &oldest.id)
    });
    let mut headers = HeaderMap::new();
    if let Some(v) = link {
        if let Ok(val) = v.parse() {
            headers.insert(header::LINK, val);
        }
    }
    (headers, Json(statuses))
}
