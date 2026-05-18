use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, Uri},
    response::{IntoResponse, Json},
    Extension,
};
use crate::{
    error::{AppError, AppResult},
    middleware::{AuthenticatedUser, ResolvedInstance},
    state::AppState,
};
use super::types::Tag;

fn tag_url(domain: &str, name: &str) -> String {
    format!("https://{domain}/tags/{name}")
}

// ── GET /api/v1/tags/:name ────────────────────────────────────────────────

pub async fn get_tag(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Path(name): Path<String>,
    auth: Option<Extension<AuthenticatedUser>>,
) -> AppResult<Json<Tag>> {
    let domain = instance.custom_domain.as_deref().unwrap_or(&instance.domain);
    let name = name.to_lowercase();

    let tag = sqlx::query!(
        "SELECT id FROM tags WHERE name = $1",
        name,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound)?;

    let (following, featuring) = if let Some(Extension(auth)) = auth {
        let following = sqlx::query_scalar!(
            r#"SELECT EXISTS(
               SELECT 1 FROM tag_follows tf
               JOIN tags t ON t.id = tf.tag_id
               WHERE tf.account_id = $1 AND t.name = $2
            )"#,
            auth.account_id,
            name,
        )
        .fetch_one(&state.db)
        .await?
        .unwrap_or(false);

        let featuring = sqlx::query_scalar!(
            r#"SELECT EXISTS(
               SELECT 1 FROM featured_tags ft
               WHERE ft.account_id = $1 AND ft.tag_id = $2
            )"#,
            auth.account_id,
            tag.id,
        )
        .fetch_one(&state.db)
        .await?
        .unwrap_or(false);

        (Some(following), Some(featuring))
    } else {
        (None, None)
    };

    Ok(Json(Tag {
        id: tag.id.to_string(),
        url: tag_url(domain, &name),
        name,
        history: vec![],
        following,
        featuring,
    }))
}

#[derive(Debug, serde::Deserialize)]
pub struct FollowedTagsParams {
    limit: Option<i64>,
    max_id: Option<String>,
    since_id: Option<String>,
    min_id: Option<String>,
}

pub async fn list_followed_tags(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Extension(auth): Extension<AuthenticatedUser>,
    Query(params): Query<FollowedTagsParams>,
    uri: Uri,
    req_headers: HeaderMap,
) -> AppResult<impl IntoResponse> {
    auth.require_scope("read:follows")?;
    let domain = instance.custom_domain.as_deref().unwrap_or(&instance.domain);
    let limit = params.limit.unwrap_or(100).min(200).max(1) as i64;
    let max_id = params.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = params.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = params.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let rows = sqlx::query!(
        r#"SELECT tf.id AS follow_id, t.id, t.name
           FROM tag_follows tf
           JOIN tags t ON t.id = tf.tag_id
           WHERE tf.account_id = $1
             AND ($2::bigint IS NULL OR tf.id < $2)
             AND ($3::bigint IS NULL OR tf.id > $3)
             AND ($4::bigint IS NULL OR tf.id > $4)
           ORDER BY tf.id DESC
           LIMIT $5"#,
        auth.account_id,
        max_id,
        since_id,
        min_id,
        limit,
    )
    .fetch_all(&state.db)
    .await?;

    // Use tag_follow id (bigint) as the pagination cursor, not the tag UUID.
    let first_follow_id = rows.first().map(|r| r.follow_id.to_string());
    let last_follow_id = rows.last().map(|r| r.follow_id.to_string());

    let result: Vec<Tag> = rows
        .into_iter()
        .map(|r| Tag {
            id: r.id.to_string(),
            url: tag_url(domain, &r.name),
            name: r.name,
            history: vec![],
            following: Some(true),
            featuring: None,
        })
        .collect();

    let link = first_follow_id.zip(last_follow_id).map(|(newest_fid, oldest_fid)| {
        let extra = super::non_pagination_query(uri.query());
        super::link_header(&req_headers, uri.path(), &extra, &newest_fid, &oldest_fid)
    });
    let mut resp_headers = HeaderMap::new();
    if let Some(v) = link {
        if let Ok(val) = v.parse() {
            resp_headers.insert(header::LINK, val);
        }
    }
    Ok((resp_headers, Json(result)))
}

pub async fn follow_tag(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(name): Path<String>,
) -> AppResult<Json<Tag>> {
    auth.require_scope("write:follows")?;
    let domain = instance.custom_domain.as_deref().unwrap_or(&instance.domain);
    let name = name.to_lowercase();

    let tag_id = sqlx::query_scalar!(
        "SELECT id FROM tags WHERE name = $1",
        name,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound)?;

    sqlx::query!(
        "INSERT INTO tag_follows (account_id, tag_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        auth.account_id,
        tag_id,
    )
    .execute(&state.db)
    .await?;

    Ok(Json(Tag {
        id: tag_id.to_string(),
        url: tag_url(domain, &name),
        name,
        history: vec![],
        following: Some(true),
        featuring: Some(false),
    }))
}

pub async fn unfollow_tag(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(name): Path<String>,
) -> AppResult<Json<Tag>> {
    auth.require_scope("write:follows")?;
    let domain = instance.custom_domain.as_deref().unwrap_or(&instance.domain);
    let name = name.to_lowercase();

    let tag_id = sqlx::query_scalar!(
        "SELECT id FROM tags WHERE name = $1",
        name,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound)?;

    sqlx::query!(
        "DELETE FROM tag_follows WHERE account_id = $1 AND tag_id = $2",
        auth.account_id,
        tag_id,
    )
    .execute(&state.db)
    .await?;

    Ok(Json(Tag {
        id: tag_id.to_string(),
        url: tag_url(domain, &name),
        name,
        history: vec![],
        following: Some(false),
        featuring: Some(false),
    }))
}
