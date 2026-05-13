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
use super::types::Announcement;

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
            reactions: vec![],
            statuses: vec![],
            tags: vec![],
            emojis: vec![],
            mentions: vec![],
        });
    }

    Ok(Json(result))
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
