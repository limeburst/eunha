use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};

use crate::{
    error::AppResult,
    middleware::{AuthenticatedUser, ResolvedInstance},
    state::AppState,
};
use super::types::{Announcement, AnnouncementReaction};

// ── GET /api/v1/announcements ─────────────────────────────────────────────

pub async fn get_announcements(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    auth: Option<Extension<AuthenticatedUser>>,
) -> AppResult<Json<Vec<Announcement>>> {
    let viewer_id = auth.map(|Extension(a)| a.account_id);

    let rows = sqlx::query!(
        r#"SELECT id, text, published, all_day, starts_at, ends_at,
                  published_at, created_at, updated_at
           FROM announcements
           WHERE instance_id = $1
             AND published = true
             AND (ends_at IS NULL OR ends_at > now())
           ORDER BY published_at DESC"#,
        instance.id,
    )
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::with_capacity(rows.len());
    for r in &rows {
        let read = if let Some(vid) = viewer_id {
            sqlx::query_scalar!(
                "SELECT 1 FROM announcement_dismissals WHERE announcement_id = $1 AND account_id = $2",
                r.id, vid,
            )
            .fetch_optional(&state.db)
            .await?
            .is_some()
        } else {
            false
        };

        let reactions = fetch_reactions(&state, r.id, viewer_id).await?;

        result.push(Announcement {
            id: r.id.to_string(),
            text: r.text.clone(),
            published: r.published,
            all_day: r.all_day,
            created_at: r.created_at.to_rfc3339(),
            updated_at: r.updated_at.to_rfc3339(),
            published_at: r.published_at.to_rfc3339(),
            starts_at: r.starts_at.map(|t| t.to_rfc3339()),
            ends_at: r.ends_at.map(|t| t.to_rfc3339()),
            read,
            reactions,
            statuses: vec![],
            tags: vec![],
            emojis: vec![],
            mentions: vec![],
        });
    }

    Ok(Json(result))
}

async fn fetch_reactions(
    state: &AppState,
    announcement_id: i64,
    viewer_id: Option<i64>,
) -> AppResult<Vec<AnnouncementReaction>> {
    let rows = sqlx::query!(
        r#"SELECT ar.name,
                  COUNT(*) AS "count!",
                  ce.image_url,
                  ce.static_image_url
           FROM announcement_reactions ar
           LEFT JOIN custom_emojis ce ON ce.id = ar.custom_emoji_id
           WHERE ar.announcement_id = $1
           GROUP BY ar.name, ce.image_url, ce.static_image_url
           ORDER BY ar.name"#,
        announcement_id,
    )
    .fetch_all(&state.db)
    .await?;

    let mut reactions = Vec::with_capacity(rows.len());
    for row in rows {
        let me = if let Some(vid) = viewer_id {
            sqlx::query_scalar!(
                "SELECT 1 FROM announcement_reactions WHERE announcement_id = $1 AND account_id = $2 AND name = $3",
                announcement_id, vid, row.name,
            )
            .fetch_optional(&state.db)
            .await?
            .is_some()
        } else {
            false
        };

        reactions.push(AnnouncementReaction {
            name: row.name,
            count: row.count,
            me,
            url: Some(row.image_url),
            static_url: row.static_image_url,
        });
    }

    Ok(reactions)
}

// ── POST /api/v1/announcements/:id/dismiss ────────────────────────────────

pub async fn dismiss_announcement(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<StatusCode> {
    sqlx::query!(
        "INSERT INTO announcement_dismissals (announcement_id, account_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        id, auth.account_id,
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::OK)
}

// ── PUT /api/v1/announcements/:id/reactions/:name ─────────────────────────

pub async fn add_reaction(
    State(state): State<AppState>,
    Path((id, name)): Path<(i64, String)>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<StatusCode> {
    let custom_emoji_id = sqlx::query_scalar!(
        r#"SELECT id FROM custom_emojis
           WHERE shortcode = $1
             AND NOT disabled"#,
        name,
    )
    .fetch_optional(&state.db)
    .await?;

    sqlx::query!(
        r#"INSERT INTO announcement_reactions (announcement_id, account_id, name, custom_emoji_id)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (announcement_id, account_id, name) DO NOTHING"#,
        id, auth.account_id, name, custom_emoji_id,
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::OK)
}

// ── DELETE /api/v1/announcements/:id/reactions/:name ─────────────────────

pub async fn remove_reaction(
    State(state): State<AppState>,
    Path((id, name)): Path<(i64, String)>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<StatusCode> {
    sqlx::query!(
        "DELETE FROM announcement_reactions WHERE announcement_id = $1 AND account_id = $2 AND name = $3",
        id, auth.account_id, name,
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::OK)
}
