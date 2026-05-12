use axum::{
    extract::{Extension, FromRequest, Multipart, Path, State},
    http::header,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    db::models::{Account, Status as DbStatus},
    error::{AppError, AppResult},
    middleware::{AuthenticatedUser, ResolvedInstance},
    push,
    state::AppState,
    streaming::Event,
};
use super::{
    accounts::{fetch_reblog_data, fetch_status_media},
    convert::{account_from_db, status_from_db},
    types::{Status, StatusContext, StatusEdit, StatusSource},
};

#[derive(Debug, Deserialize, Default)]
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
    request: axum::extract::Request,
) -> AppResult<Json<Status>> {
    let form = extract_post_status_form(request).await?;
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

    if matches!(visibility, "public" | "unlisted" | "private") {
        if let Ok(payload) = serde_json::to_string(&api_status) {
            state.streaming.publish(Event::NewStatus {
                instance_id: instance.id,
                author_id: account.id,
                is_public: visibility == "public",
                status_id: status.id,
                payload: std::sync::Arc::new(payload),
            });
        }
    }

    // Notify the author of the parent status if this is a reply
    if let Some(parent_id) = in_reply_to_id {
        if let Ok(Some(parent)) = sqlx::query!(
            "SELECT account_id FROM statuses WHERE id = $1 AND deleted_at IS NULL",
            parent_id,
        )
        .fetch_optional(&state.db)
        .await
        {
            push::create_and_push(
                &state,
                parent.account_id,
                account.id,
                "mention",
                Some(status.id),
                format!("{} mentioned you", account.display_name),
                account.acct().clone(),
                account.avatar.clone().unwrap_or_default(),
            ).await;
        }
    }

    Ok(Json(api_status))
}

async fn extract_post_status_form(request: axum::extract::Request) -> AppResult<PostStatusForm> {
    let ct = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if ct.contains("application/json") {
        return axum::extract::Json::<PostStatusForm>::from_request(request, &())
            .await
            .map(|axum::extract::Json(f)| f)
            .map_err(|e| AppError::Unprocessable(e.to_string()));
    }

    if ct.contains("multipart/form-data") {
        let mut multipart = Multipart::from_request(request, &())
            .await
            .map_err(|e| AppError::Unprocessable(e.to_string()))?;
        let mut form = PostStatusForm::default();
        let mut media_ids: Vec<String> = Vec::new();
        while let Some(field) = multipart
            .next_field()
            .await
            .map_err(|e| AppError::Unprocessable(e.to_string()))?
        {
            let name = field.name().unwrap_or("").to_string();
            let text = field
                .text()
                .await
                .map_err(|e| AppError::Unprocessable(e.to_string()))?;
            match name.as_str() {
                "status" => form.status = Some(text),
                "in_reply_to_id" => form.in_reply_to_id = if text.is_empty() { None } else { Some(text) },
                "spoiler_text" => form.spoiler_text = if text.is_empty() { None } else { Some(text) },
                "visibility" => form.visibility = Some(text),
                "language" => form.language = if text.is_empty() { None } else { Some(text) },
                "sensitive" => form.sensitive = Some(text == "true" || text == "1"),
                "media_ids[]" | "media_ids" => {
                    if !text.is_empty() {
                        media_ids.push(text);
                    }
                }
                _ => {}
            }
        }
        if !media_ids.is_empty() {
            form.media_ids = Some(media_ids);
        }
        return Ok(form);
    }

    // Fall back to URL-encoded form
    axum::extract::Form::<PostStatusForm>::from_request(request, &())
        .await
        .map(|axum::extract::Form(f)| f)
        .map_err(|e| AppError::Unprocessable(e.to_string()))
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
    let reblog = fetch_reblog_data(&state, &status).await?;
    let viewer_ctx = if let Some(Extension(auth)) = auth {
        Some(build_viewer_context(&state, auth.account_id, id).await?)
    } else {
        None
    };

    Ok(Json(status_from_db(&status, &account, media, reblog, viewer_ctx)))
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
    let reblog = fetch_reblog_data(&state, &status).await?;
    let mut s = status_from_db(&status, &account, media, reblog, None);
    s.text = Some(status.text.clone());
    Ok(Json(s))
}

// ── POST /api/v1/statuses/:id/favourite ───────────────────────────────────

pub async fn favourite_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Status>> {
    fetch_status_with_account(&state, id).await?;

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

    let (status, account) = fetch_status_with_account(&state, id).await?;
    let media = fetch_status_media(&state, id).await?;
    let reblog = fetch_reblog_data(&state, &status).await?;
    let ctx = build_viewer_context(&state, auth.account_id, id).await?;

    // Notify status author
    let from_display = {
        let from = fetch_account(&state, auth.account_id).await?;
        from.display_name.clone()
    };
    push::create_and_push(
        &state,
        status.account_id,
        auth.account_id,
        "favourite",
        Some(id),
        format!("{} favourited your post", from_display),
        account_from_db(&account).acct.clone(),
        account.avatar.clone().unwrap_or_default(),
    ).await;

    Ok(Json(status_from_db(&status, &account, media, reblog, Some(ctx))))
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
    let reblog = fetch_reblog_data(&state, &status).await?;
    let ctx = build_viewer_context(&state, auth.account_id, id).await?;
    Ok(Json(status_from_db(&status, &account, media, reblog, Some(ctx))))
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

    // Notify original author
    push::create_and_push(
        &state,
        original.account_id,
        auth.account_id,
        "reblog",
        Some(id),
        format!("{} boosted your post", boost_account.display_name),
        boost_account.acct().clone(),
        boost_account.avatar.clone().unwrap_or_default(),
    ).await;

    let media = fetch_status_media(&state, boost.id).await?;
    let reblog = fetch_reblog_data(&state, &boost).await?;
    Ok(Json(status_from_db(&boost, &boost_account, media, reblog, None)))
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
        let reblog = fetch_reblog_data(&state, s).await?;
        let ctx = if let Some(vid) = viewer_id {
            Some(build_viewer_context(&state, vid, s.id).await?)
        } else {
            None
        };
        ancestors.push(status_from_db(s, &acct, media, reblog, ctx));
    }

    let mut descendants = Vec::with_capacity(descendant_rows.len());
    for s in &descendant_rows {
        let acct = fetch_account(&state, s.account_id).await?;
        let media = fetch_status_media(&state, s.id).await?;
        let reblog = fetch_reblog_data(&state, s).await?;
        let ctx = if let Some(vid) = viewer_id {
            Some(build_viewer_context(&state, vid, s.id).await?)
        } else {
            None
        };
        descendants.push(status_from_db(s, &acct, media, reblog, ctx));
    }

    Ok(Json(StatusContext { ancestors, descendants }))
}

// ── POST /api/v1/statuses/:id/unreblog ────────────────────────────────────

pub async fn unreblog_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Status>> {
    fetch_status_with_account(&state, id).await?;

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

    let (status, account) = fetch_status_with_account(&state, id).await?;
    let media = fetch_status_media(&state, id).await?;
    let reblog = fetch_reblog_data(&state, &status).await?;
    let ctx = build_viewer_context(&state, auth.account_id, id).await?;
    Ok(Json(status_from_db(&status, &account, media, reblog, Some(ctx))))
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
    let reblog = fetch_reblog_data(&state, &status).await?;
    let ctx = build_viewer_context(&state, auth.account_id, id).await?;
    Ok(Json(status_from_db(&status, &account, media, reblog, Some(ctx))))
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
    let reblog = fetch_reblog_data(&state, &status).await?;
    let ctx = build_viewer_context(&state, auth.account_id, id).await?;
    Ok(Json(status_from_db(&status, &account, media, reblog, Some(ctx))))
}

// ── POST /api/v1/statuses/:id/pin ─────────────────────────────────────────

pub async fn pin_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Status>> {
    let (status, account) = fetch_status_with_account(&state, id).await?;
    if status.account_id != auth.account_id {
        return Err(AppError::Forbidden);
    }
    sqlx::query!(
        "INSERT INTO status_pins (account_id, status_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        auth.account_id, id
    )
    .execute(&state.db)
    .await?;
    let media = fetch_status_media(&state, id).await?;
    let reblog = fetch_reblog_data(&state, &status).await?;
    let ctx = build_viewer_context(&state, auth.account_id, id).await?;
    Ok(Json(status_from_db(&status, &account, media, reblog, Some(ctx))))
}

// ── POST /api/v1/statuses/:id/unpin ───────────────────────────────────────

pub async fn unpin_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Status>> {
    let (status, account) = fetch_status_with_account(&state, id).await?;
    sqlx::query!(
        "DELETE FROM status_pins WHERE account_id = $1 AND status_id = $2",
        auth.account_id, id
    )
    .execute(&state.db)
    .await?;
    let media = fetch_status_media(&state, id).await?;
    let reblog = fetch_reblog_data(&state, &status).await?;
    let ctx = build_viewer_context(&state, auth.account_id, id).await?;
    Ok(Json(status_from_db(&status, &account, media, reblog, Some(ctx))))
}

// ── POST /api/v1/statuses/:id/mute ────────────────────────────────────────

pub async fn mute_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Status>> {
    let (status, account) = fetch_status_with_account(&state, id).await?;
    sqlx::query!(
        "INSERT INTO conversation_mutes (account_id, status_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        auth.account_id, id
    )
    .execute(&state.db)
    .await?;
    let media = fetch_status_media(&state, id).await?;
    let reblog = fetch_reblog_data(&state, &status).await?;
    let ctx = build_viewer_context(&state, auth.account_id, id).await?;
    Ok(Json(status_from_db(&status, &account, media, reblog, Some(ctx))))
}

// ── POST /api/v1/statuses/:id/unmute ──────────────────────────────────────

pub async fn unmute_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Status>> {
    let (status, account) = fetch_status_with_account(&state, id).await?;
    sqlx::query!(
        "DELETE FROM conversation_mutes WHERE account_id = $1 AND status_id = $2",
        auth.account_id, id
    )
    .execute(&state.db)
    .await?;
    let media = fetch_status_media(&state, id).await?;
    let reblog = fetch_reblog_data(&state, &status).await?;
    let ctx = build_viewer_context(&state, auth.account_id, id).await?;
    Ok(Json(status_from_db(&status, &account, media, reblog, Some(ctx))))
}

// ── GET /api/v1/statuses/:id/favourited_by ────────────────────────────────

pub async fn favourited_by(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Vec<super::types::Account>>> {
    sqlx::query!("SELECT id FROM statuses WHERE id = $1 AND deleted_at IS NULL", id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let accounts = sqlx::query_as!(
        Account,
        r#"SELECT a.* FROM accounts a
           JOIN favourites f ON f.account_id = a.id
           WHERE f.status_id = $1
           ORDER BY f.id DESC LIMIT 80"#,
        id,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(accounts.iter().map(account_from_db).collect()))
}

// ── GET /api/v1/statuses/:id/reblogged_by ─────────────────────────────────

pub async fn reblogged_by(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Vec<super::types::Account>>> {
    sqlx::query!("SELECT id FROM statuses WHERE id = $1 AND deleted_at IS NULL", id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let accounts = sqlx::query_as!(
        Account,
        r#"SELECT a.* FROM accounts a
           JOIN statuses s ON s.account_id = a.id
           WHERE s.reblog_of_id = $1 AND s.deleted_at IS NULL
           ORDER BY s.id DESC LIMIT 80"#,
        id,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(accounts.iter().map(account_from_db).collect()))
}

// ── PUT /api/v1/statuses/:id ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct EditStatusForm {
    pub status: Option<String>,
    pub spoiler_text: Option<String>,
    pub sensitive: Option<bool>,
    pub language: Option<String>,
}

pub async fn edit_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<EditStatusForm>,
) -> AppResult<Json<Status>> {
    let (status, account) = fetch_status_with_account(&state, id).await?;
    if status.account_id != auth.account_id {
        return Err(AppError::Forbidden);
    }

    // Save current version to edits before updating
    sqlx::query!(
        r#"INSERT INTO status_edits (status_id, account_id, text, content, spoiler_text, sensitive)
           VALUES ($1, $2, $3, $4, $5, $6)"#,
        id, auth.account_id, status.text, status.content, status.spoiler_text, status.sensitive,
    )
    .execute(&state.db)
    .await?;

    let new_text = form.status.unwrap_or_else(|| status.text.clone());
    let new_content = render_markdown(&new_text);
    let new_spoiler = form.spoiler_text.unwrap_or_else(|| status.spoiler_text.clone());
    let new_sensitive = form.sensitive.unwrap_or(status.sensitive);
    let new_language = form.language.or(status.language.clone());

    sqlx::query!(
        "UPDATE statuses SET text = $1, content = $2, spoiler_text = $3, sensitive = $4, language = $5, edited_at = now() WHERE id = $6",
        new_text, new_content, new_spoiler, new_sensitive, new_language, id,
    )
    .execute(&state.db)
    .await?;

    let (updated_status, _) = fetch_status_with_account(&state, id).await?;
    let media = fetch_status_media(&state, id).await?;
    let reblog = fetch_reblog_data(&state, &updated_status).await?;
    let ctx = build_viewer_context(&state, auth.account_id, id).await?;
    Ok(Json(status_from_db(&updated_status, &account, media, reblog, Some(ctx))))
}

// ── GET /api/v1/statuses/:id/history ──────────────────────────────────────

pub async fn get_status_history(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Vec<StatusEdit>>> {
    let status = sqlx::query_as!(
        DbStatus,
        "SELECT * FROM statuses WHERE id = $1 AND deleted_at IS NULL",
        id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let account = sqlx::query_as!(
        Account,
        "SELECT * FROM accounts WHERE id = $1",
        status.account_id,
    )
    .fetch_one(&state.db)
    .await?;

    let edits = sqlx::query_as!(
        crate::db::models::StatusEdit,
        "SELECT * FROM status_edits WHERE status_id = $1 ORDER BY created_at ASC",
        id,
    )
    .fetch_all(&state.db)
    .await?;

    let api_account = account_from_db(&account);
    let mut result: Vec<StatusEdit> = edits.iter().map(|e| StatusEdit {
        content: e.content.clone(),
        spoiler_text: e.spoiler_text.clone(),
        sensitive: e.sensitive,
        created_at: e.created_at.to_rfc3339(),
        account: api_account.clone(),
        media_attachments: vec![],
        emojis: vec![],
        poll: None,
    }).collect();

    // Append current version
    result.push(StatusEdit {
        content: status.content.clone(),
        spoiler_text: status.spoiler_text.clone(),
        sensitive: status.sensitive,
        created_at: status.edited_at.unwrap_or(status.created_at).to_rfc3339(),
        account: api_account,
        media_attachments: vec![],
        emojis: vec![],
        poll: None,
    });

    Ok(Json(result))
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

/// Batch-fetch viewer context for a list of status IDs in 5 queries.
/// Returns a map from status_id → StatusViewerContext.
pub(super) async fn batch_viewer_contexts(
    state: &AppState,
    viewer_id: Uuid,
    status_ids: &[i64],
) -> AppResult<std::collections::HashMap<i64, super::convert::StatusViewerContext>> {
    use std::collections::{HashMap, HashSet};
    use super::convert::StatusViewerContext;

    if status_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let fav_set: HashSet<i64> = sqlx::query_scalar!(
        "SELECT status_id FROM favourites WHERE account_id = $1 AND status_id = ANY($2::bigint[])",
        viewer_id, status_ids,
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .collect();

    let reb_set: HashSet<i64> = sqlx::query_scalar!(
        r#"SELECT reblog_of_id as "reblog_of_id!: i64" FROM statuses
           WHERE account_id = $1 AND reblog_of_id = ANY($2::bigint[]) AND deleted_at IS NULL"#,
        viewer_id, status_ids,
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .collect();

    let book_set: HashSet<i64> = sqlx::query_scalar!(
        "SELECT status_id FROM bookmarks WHERE account_id = $1 AND status_id = ANY($2::bigint[])",
        viewer_id, status_ids,
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .collect();

    let mute_set: HashSet<i64> = sqlx::query_scalar!(
        "SELECT status_id FROM conversation_mutes WHERE account_id = $1 AND status_id = ANY($2::bigint[])",
        viewer_id, status_ids,
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .collect();

    let pin_set: HashSet<i64> = sqlx::query_scalar!(
        "SELECT status_id FROM status_pins WHERE account_id = $1 AND status_id = ANY($2::bigint[])",
        viewer_id, status_ids,
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .collect();

    let mut result = HashMap::with_capacity(status_ids.len());
    for &id in status_ids {
        result.insert(id, StatusViewerContext {
            favourited: fav_set.contains(&id),
            reblogged: reb_set.contains(&id),
            bookmarked: book_set.contains(&id),
            muted: mute_set.contains(&id),
            pinned: pin_set.contains(&id),
        });
    }
    Ok(result)
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

    let muted = sqlx::query!(
        "SELECT 1 as e FROM conversation_mutes WHERE account_id = $1 AND status_id = $2",
        viewer_id, status_id
    )
    .fetch_optional(&state.db)
    .await?
    .is_some();

    let pinned = sqlx::query!(
        "SELECT 1 as e FROM status_pins WHERE account_id = $1 AND status_id = $2",
        viewer_id, status_id
    )
    .fetch_optional(&state.db)
    .await?
    .is_some();

    Ok(super::convert::StatusViewerContext {
        favourited,
        reblogged,
        muted,
        bookmarked,
        pinned,
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
