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

    let ann_ids: Vec<i64> = rows.iter().map(|r| r.id).collect();

    // Batch-fetch dismissed announcements for the viewer
    let dismissed_set: std::collections::HashSet<i64> = if let Some(vid) = viewer_id {
        sqlx::query_scalar!(
            "SELECT announcement_id FROM announcement_dismissals WHERE account_id = $1 AND announcement_id = ANY($2::bigint[])",
            vid, &ann_ids,
        )
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .collect()
    } else {
        std::collections::HashSet::new()
    };

    // Batch-fetch all reactions for all announcements
    let all_reactions = sqlx::query!(
        r#"SELECT ar.announcement_id, ar.name,
                  COUNT(*) AS "count!",
                  ce.image_url,
                  ce.static_image_url
           FROM announcement_reactions ar
           LEFT JOIN custom_emojis ce ON ce.id = ar.custom_emoji_id
           WHERE ar.announcement_id = ANY($1::bigint[])
           GROUP BY ar.announcement_id, ar.name, ce.image_url, ce.static_image_url
           ORDER BY ar.announcement_id, ar.name"#,
        &ann_ids,
    )
    .fetch_all(&state.db)
    .await?;

    // Batch-fetch the viewer's own reactions
    let my_reactions: std::collections::HashSet<(i64, String)> = if let Some(vid) = viewer_id {
        sqlx::query!(
            "SELECT announcement_id, name FROM announcement_reactions WHERE account_id = $1 AND announcement_id = ANY($2::bigint[])",
            vid, &ann_ids,
        )
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|r| (r.announcement_id, r.name))
        .collect()
    } else {
        std::collections::HashSet::new()
    };

    // Group reactions by announcement_id
    let mut reactions_by_ann: std::collections::HashMap<i64, Vec<AnnouncementReaction>> =
        std::collections::HashMap::new();
    for row in all_reactions {
        let me = my_reactions.contains(&(row.announcement_id, row.name.clone()));
        reactions_by_ann.entry(row.announcement_id).or_default().push(AnnouncementReaction {
            name: row.name,
            count: row.count,
            me,
            url: Some(row.image_url),
            static_url: row.static_image_url,
        });
    }

    let mut result = Vec::with_capacity(rows.len());
    for r in &rows {
        result.push(Announcement {
            id: r.id.to_string(),
            text: r.text.clone(),
            published: r.published,
            all_day: r.all_day,
            created_at: super::convert::mastodon_date(r.created_at),
            updated_at: super::convert::mastodon_date(r.updated_at),
            published_at: super::convert::mastodon_date(r.published_at),
            starts_at: r.starts_at.map(super::convert::mastodon_date),
            ends_at: r.ends_at.map(super::convert::mastodon_date),
            read: dismissed_set.contains(&r.id),
            reactions: reactions_by_ann.remove(&r.id).unwrap_or_default(),
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
