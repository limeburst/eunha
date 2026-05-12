use axum::{
    extract::{Path, State},
    response::Json,
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

pub async fn list_followed_tags(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<Tag>>> {
    let domain = instance.custom_domain.as_deref().unwrap_or(&instance.domain);

    let rows = sqlx::query!(
        r#"SELECT t.id, t.name
           FROM tag_follows tf
           JOIN tags t ON t.id = tf.tag_id
           WHERE tf.account_id = $1
           ORDER BY t.name"#,
        auth.account_id,
    )
    .fetch_all(&state.db)
    .await?;

    let tags = rows
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

    Ok(Json(tags))
}

pub async fn follow_tag(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Extension(auth): Extension<AuthenticatedUser>,
    Path(name): Path<String>,
) -> AppResult<Json<Tag>> {
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
