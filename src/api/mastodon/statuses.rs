use axum::{
    extract::{Extension, Path, State},
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    db::models::{Account, Status as DbStatus},
    error::{AppError, AppResult},
    middleware::{AuthenticatedUser, ResolvedInstance},
    state::AppState,
    streaming::Event,
};
use super::{
    accounts::fetch_status_media,
    convert::status_from_db,
    types::{Status, StatusContext, StatusSource},
};

#[derive(Debug, Deserialize)]
pub struct PostStatusForm {
    pub status: Option<String>,
    pub in_reply_to_id: Option<String>,
    pub spoiler_text: Option<String>,
    pub sensitive: Option<bool>,
    pub language: Option<String>,
    pub visibility: Option<String>,
    pub media_ids: Option<Vec<String>>,
}

// ── POST /api/v1/statuses ──────────────────────────────────────────────────

pub async fn post_status(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<PostStatusForm>,
) -> AppResult<Json<Status>> {
    let account = fetch_account(&state, auth.account_id).await?;
    let text = form.status.unwrap_or_default();
    if text.is_empty() && form.media_ids.as_ref().map_or(true, |m| m.is_empty()) {
        return Err(AppError::Unprocessable("Status must have text or media".into()));
    }

    let visibility = form.visibility.as_deref().unwrap_or("public");
    let in_reply_to_id = form.in_reply_to_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let content = render_markdown(&text);

    let status = sqlx::query_as!(
        DbStatus,
        r#"INSERT INTO statuses
             (instance_id, account_id, text, content, spoiler_text, visibility,
              language, sensitive, in_reply_to_id)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
           RETURNING *"#,
        instance.id,
        account.id,
        text,
        content,
        form.spoiler_text.unwrap_or_default(),
        visibility,
        form.language,
        form.sensitive.unwrap_or(false),
        in_reply_to_id,
    )
    .fetch_one(&state.db)
    .await?;

    // Attach URI now that we have the ID
    let uri = format!("https://{}/users/{}/statuses/{}", instance.domain, account.username, status.id);
    let url = uri.clone();
    sqlx::query!(
        "UPDATE statuses SET uri = $1, url = $2 WHERE id = $3",
        uri, url, status.id
    )
    .execute(&state.db)
    .await?;

    // Increment statuses count
    sqlx::query!(
        "UPDATE accounts SET statuses_count = statuses_count + 1 WHERE id = $1",
        account.id
    )
    .execute(&state.db)
    .await?;

    // Attach media
    if let Some(media_ids) = &form.media_ids {
        for id_str in media_ids {
            if let Ok(media_id) = id_str.parse::<i64>() {
                sqlx::query!(
                    "UPDATE media_attachments SET status_id = $1
                     WHERE id = $2 AND account_id = $3 AND status_id IS NULL",
                    status.id, media_id, account.id
                )
                .execute(&state.db)
                .await?;
            }
        }
    }

    let mut status = status;
    status.uri = Some(uri);

    let media = fetch_status_media(&state, status.id).await?;
    let api_status = status_from_db(&status, &account, media, None, None);

    if matches!(visibility, "public" | "unlisted") {
        if let Ok(payload) = serde_json::to_string(&api_status) {
            state.streaming.publish(Event::NewStatus {
                instance_id: instance.id,
                author_id: account.id,
                is_public: true,
                payload: std::sync::Arc::new(payload),
            });
        }
    }

    Ok(Json(api_status))
}

// ── GET /api/v1/statuses/:id ───────────────────────────────────────────────

pub async fn get_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    auth: Option<Extension<AuthenticatedUser>>,
) -> AppResult<Json<Status>> {
    let (status, account) = fetch_status_with_account(&state, id).await?;

    let viewer_id = auth.as_ref().map(|Extension(a)| a.account_id);

    match status.visibility.as_str() {
        "private" => {
            let is_author = viewer_id == Some(status.account_id);
            let is_follower = if let Some(vid) = viewer_id {
                sqlx::query_scalar!(
                    "SELECT 1 as e FROM follows WHERE account_id = $1 AND target_account_id = $2 AND state = 'accepted'",
                    vid, status.account_id
                )
                .fetch_optional(&state.db)
                .await?
                .is_some()
            } else {
                false
            };
            if !is_author && !is_follower {
                return Err(AppError::NotFound);
            }
        }
        "direct" => {
            if viewer_id != Some(status.account_id) {
                return Err(AppError::NotFound);
            }
        }
        _ => {}
    }

    let media = fetch_status_media(&state, id).await?;
    let viewer_ctx = if let Some(Extension(auth)) = auth {
        Some(build_viewer_context(&state, auth.account_id, id).await?)
    } else {
        None
    };

    Ok(Json(status_from_db(&status, &account, media, None, viewer_ctx)))
}

// ── DELETE /api/v1/statuses/:id ────────────────────────────────────────────

pub async fn delete_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Status>> {
    let (status, account) = fetch_status_with_account(&state, id).await?;
    if status.account_id != auth.account_id {
        return Err(AppError::Forbidden);
    }

    sqlx::query!(
        "UPDATE statuses SET deleted_at = now() WHERE id = $1",
        id
    )
    .execute(&state.db)
    .await?;

    sqlx::query!(
        "UPDATE accounts SET statuses_count = GREATEST(statuses_count - 1, 0) WHERE id = $1",
        account.id
    )
    .execute(&state.db)
    .await?;

    state.streaming.publish(Event::DeleteStatus {
        instance_id: status.instance_id,
        status_id: id,
    });

    let media = fetch_status_media(&state, id).await?;
    Ok(Json(status_from_db(&status, &account, media, None, None)))
}

// ── POST /api/v1/statuses/:id/favourite ───────────────────────────────────

pub async fn favourite_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Status>> {
    let (status, account) = fetch_status_with_account(&state, id).await?;

    sqlx::query!(
        "INSERT INTO favourites (account_id, status_id) VALUES ($1,$2) ON CONFLICT DO NOTHING",
        auth.account_id, id
    )
    .execute(&state.db)
    .await?;

    sqlx::query!(
        "UPDATE statuses SET favourites_count = (SELECT COUNT(*) FROM favourites WHERE status_id = $1) WHERE id = $1",
        id
    )
    .execute(&state.db)
    .await?;

    let (status, _) = fetch_status_with_account(&state, id).await?;
    let media = fetch_status_media(&state, id).await?;
    let ctx = build_viewer_context(&state, auth.account_id, id).await?;
    Ok(Json(status_from_db(&status, &account, media, None, Some(ctx))))
}

// ── POST /api/v1/statuses/:id/unfavourite ─────────────────────────────────

pub async fn unfavourite_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Status>> {
    let (_, account) = fetch_status_with_account(&state, id).await?;

    sqlx::query!(
        "DELETE FROM favourites WHERE account_id = $1 AND status_id = $2",
        auth.account_id, id
    )
    .execute(&state.db)
    .await?;

    sqlx::query!(
        "UPDATE statuses SET favourites_count = (SELECT COUNT(*) FROM favourites WHERE status_id = $1) WHERE id = $1",
        id
    )
    .execute(&state.db)
    .await?;

    let (status, _) = fetch_status_with_account(&state, id).await?;
    let media = fetch_status_media(&state, id).await?;
    let ctx = build_viewer_context(&state, auth.account_id, id).await?;
    Ok(Json(status_from_db(&status, &account, media, None, Some(ctx))))
}

// ── POST /api/v1/statuses/:id/reblog ──────────────────────────────────────

pub async fn reblog_status(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Status>> {
    let (original, _) = fetch_status_with_account(&state, id).await?;
    if original.visibility == "private" || original.visibility == "direct" {
        return Err(AppError::Forbidden);
    }

    let boost_account = fetch_account(&state, auth.account_id).await?;
    let content = String::new();
    let boost = sqlx::query_as!(
        DbStatus,
        r#"INSERT INTO statuses (instance_id, account_id, text, content, visibility, reblog_of_id)
           VALUES ($1,$2,'','',$3,$4)
           RETURNING *"#,
        instance.id,
        auth.account_id,
        original.visibility,
        id,
    )
    .fetch_one(&state.db)
    .await?;

    sqlx::query!(
        "UPDATE statuses SET reblogs_count = reblogs_count + 1 WHERE id = $1",
        id
    )
    .execute(&state.db)
    .await?;

    let media = fetch_status_media(&state, boost.id).await?;
    Ok(Json(status_from_db(&boost, &boost_account, media, None, None)))
}

// ── GET /api/v1/statuses/:id/context ──────────────────────────────────────

pub async fn get_status_context(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    auth: Option<Extension<AuthenticatedUser>>,
) -> AppResult<Json<StatusContext>> {
    sqlx::query!("SELECT id FROM statuses WHERE id = $1 AND deleted_at IS NULL", id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let viewer_id = auth.map(|Extension(a)| a.account_id);

    let ancestor_rows = sqlx::query_as::<_, DbStatus>(
        r#"WITH RECURSIVE ancestor_chain AS (
             SELECT * FROM statuses WHERE id = $1 AND deleted_at IS NULL
             UNION ALL
             SELECT s.* FROM statuses s
               JOIN ancestor_chain a ON s.id = a.in_reply_to_id
             WHERE s.deleted_at IS NULL
           )
           SELECT * FROM ancestor_chain WHERE id != $1 ORDER BY id ASC"#
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    let descendant_rows = sqlx::query_as::<_, DbStatus>(
        r#"WITH RECURSIVE reply_tree AS (
             SELECT * FROM statuses WHERE in_reply_to_id = $1 AND deleted_at IS NULL
             UNION ALL
             SELECT s.* FROM statuses s
               JOIN reply_tree r ON s.in_reply_to_id = r.id
             WHERE s.deleted_at IS NULL
           )
           SELECT * FROM reply_tree ORDER BY id ASC LIMIT 100"#
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    let mut ancestors = Vec::with_capacity(ancestor_rows.len());
    for s in &ancestor_rows {
        let acct = fetch_account(&state, s.account_id).await?;
        let media = fetch_status_media(&state, s.id).await?;
        let ctx = if let Some(vid) = viewer_id {
            Some(build_viewer_context(&state, vid, s.id).await?)
        } else {
            None
        };
        ancestors.push(status_from_db(s, &acct, media, None, ctx));
    }

    let mut descendants = Vec::with_capacity(descendant_rows.len());
    for s in &descendant_rows {
        let acct = fetch_account(&state, s.account_id).await?;
        let media = fetch_status_media(&state, s.id).await?;
        let ctx = if let Some(vid) = viewer_id {
            Some(build_viewer_context(&state, vid, s.id).await?)
        } else {
            None
        };
        descendants.push(status_from_db(s, &acct, media, None, ctx));
    }

    Ok(Json(StatusContext { ancestors, descendants }))
}

// ── POST /api/v1/statuses/:id/unreblog ────────────────────────────────────

pub async fn unreblog_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Status>> {
    let (original, account) = fetch_status_with_account(&state, id).await?;

    let deleted = sqlx::query!(
        "DELETE FROM statuses WHERE account_id = $1 AND reblog_of_id = $2 AND deleted_at IS NULL RETURNING id",
        auth.account_id, id
    )
    .fetch_optional(&state.db)
    .await?;

    if deleted.is_some() {
        sqlx::query!(
            "UPDATE statuses SET reblogs_count = GREATEST(reblogs_count - 1, 0) WHERE id = $1",
            id
        )
        .execute(&state.db)
        .await?;
    }

    let (status, _) = fetch_status_with_account(&state, id).await?;
    let media = fetch_status_media(&state, id).await?;
    let ctx = build_viewer_context(&state, auth.account_id, id).await?;
    Ok(Json(status_from_db(&status, &account, media, None, Some(ctx))))
}

// ── POST /api/v1/statuses/:id/bookmark ────────────────────────────────────

pub async fn bookmark_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Status>> {
    let (_, account) = fetch_status_with_account(&state, id).await?;

    sqlx::query!(
        "INSERT INTO bookmarks (account_id, status_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        auth.account_id, id
    )
    .execute(&state.db)
    .await?;

    let (status, _) = fetch_status_with_account(&state, id).await?;
    let media = fetch_status_media(&state, id).await?;
    let ctx = build_viewer_context(&state, auth.account_id, id).await?;
    Ok(Json(status_from_db(&status, &account, media, None, Some(ctx))))
}

// ── POST /api/v1/statuses/:id/unbookmark ──────────────────────────────────

pub async fn unbookmark_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Status>> {
    let (_, account) = fetch_status_with_account(&state, id).await?;

    sqlx::query!(
        "DELETE FROM bookmarks WHERE account_id = $1 AND status_id = $2",
        auth.account_id, id
    )
    .execute(&state.db)
    .await?;

    let (status, _) = fetch_status_with_account(&state, id).await?;
    let media = fetch_status_media(&state, id).await?;
    let ctx = build_viewer_context(&state, auth.account_id, id).await?;
    Ok(Json(status_from_db(&status, &account, media, None, Some(ctx))))
}

// ── GET /api/v1/statuses/:id/source ───────────────────────────────────────

pub async fn get_status_source(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<StatusSource>> {
    let status = sqlx::query_as!(
        DbStatus,
        "SELECT * FROM statuses WHERE id = $1 AND deleted_at IS NULL",
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    if status.account_id != auth.account_id {
        return Err(AppError::Forbidden);
    }

    Ok(Json(StatusSource {
        id: status.id.to_string(),
        text: status.text,
        spoiler_text: status.spoiler_text,
    }))
}

// ── Helpers ────────────────────────────────────────────────────────────────

async fn fetch_status_with_account(state: &AppState, id: i64) -> AppResult<(DbStatus, Account)> {
    let status = sqlx::query_as!(
        DbStatus,
        "SELECT * FROM statuses WHERE id = $1 AND deleted_at IS NULL",
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let account = sqlx::query_as!(
        Account,
        "SELECT * FROM accounts WHERE id = $1",
        status.account_id
    )
    .fetch_one(&state.db)
    .await?;

    Ok((status, account))
}

async fn fetch_account(state: &AppState, id: Uuid) -> AppResult<Account> {
    sqlx::query_as!(Account, "SELECT * FROM accounts WHERE id = $1", id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

async fn build_viewer_context(
    state: &AppState,
    viewer_id: Uuid,
    status_id: i64,
) -> AppResult<super::convert::StatusViewerContext> {
    let favourited = sqlx::query!(
        "SELECT 1 as e FROM favourites WHERE account_id = $1 AND status_id = $2",
        viewer_id, status_id
    )
    .fetch_optional(&state.db)
    .await?
    .is_some();

    let reblogged = sqlx::query!(
        "SELECT 1 as e FROM statuses WHERE account_id = $1 AND reblog_of_id = $2 AND deleted_at IS NULL",
        viewer_id, status_id
    )
    .fetch_optional(&state.db)
    .await?
    .is_some();

    let bookmarked = sqlx::query!(
        "SELECT 1 as e FROM bookmarks WHERE account_id = $1 AND status_id = $2",
        viewer_id, status_id
    )
    .fetch_optional(&state.db)
    .await?
    .is_some();

    Ok(super::convert::StatusViewerContext {
        favourited,
        reblogged,
        muted: false,
        bookmarked,
    })
}

fn render_markdown(text: &str) -> String {
    // Minimal rendering: wrap paragraphs in <p>, linkify mentions/hashtags/URLs
    // A real implementation would use a proper parser
    let escaped = ammonia::clean_text(text);
    let paragraphs: Vec<String> = escaped
        .split("\n\n")
        .map(|p| format!("<p>{}</p>", p.replace('\n', "<br />")))
        .collect();
    paragraphs.join("")
}
