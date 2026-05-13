use axum::{
    extract::{Extension, FromRequest, Multipart, Path, State},
    http::header,
    Json,
};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
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
    accounts::{build_status, fetch_reblog_data, fetch_status_media},
    convert::account_from_db,
    types::{Status, StatusContext, StatusEdit, StatusSource},
};
use super::scheduled_statuses::ScheduledStatusResponse;

static HASHTAG_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(^|[\s,.:;!?\(\[\{/])#([a-zA-Z][a-zA-Z0-9_]*)").unwrap()
});

static MENTION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(^|[\s,.:;!?\(\[\{/])@([a-zA-Z0-9_]+)(?:@([a-zA-Z0-9._:\-]+))?").unwrap()
});

static URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new("https?://[^\\s<>&\"]+").unwrap()
});

#[derive(Debug, Deserialize, Default)]
pub struct PollForm {
    pub options: Vec<String>,
    pub expires_in: Option<i64>,
    pub multiple: Option<bool>,
    pub hide_totals: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub struct PostStatusForm {
    pub status: Option<String>,
    pub in_reply_to_id: Option<String>,
    pub spoiler_text: Option<String>,
    pub sensitive: Option<bool>,
    pub language: Option<String>,
    pub visibility: Option<String>,
    pub media_ids: Option<Vec<String>>,
    pub poll: Option<PollForm>,
    pub scheduled_at: Option<String>,
}

// ── POST /api/v1/statuses ──────────────────────────────────────────────────

pub async fn post_status(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
    Extension(auth): Extension<AuthenticatedUser>,
    request: axum::extract::Request,
) -> AppResult<axum::response::Response> {
    use axum::response::IntoResponse;
    let form = extract_post_status_form(request).await?;
    let account = fetch_account(&state, auth.account_id).await?;
    let text = form.status.clone().unwrap_or_default();
    if text.is_empty() && form.media_ids.as_ref().map_or(true, |m| m.is_empty()) && form.poll.is_none() {
        return Err(AppError::Unprocessable("Status must have text or media".into()));
    }

    // Handle scheduled statuses
    if let Some(ref scheduled_at_str) = form.scheduled_at {
        let scheduled_at = chrono::DateTime::parse_from_rfc3339(scheduled_at_str)
            .map(|t| t.with_timezone(&chrono::Utc))
            .map_err(|_| AppError::Unprocessable("Invalid scheduled_at format".into()))?;
        let params = serde_json::json!({
            "text": text,
            "visibility": form.visibility,
            "spoiler_text": form.spoiler_text,
            "sensitive": form.sensitive,
            "language": form.language,
            "in_reply_to_id": form.in_reply_to_id,
            "media_ids": form.media_ids,
            "poll": form.poll.as_ref().map(|p| serde_json::json!({
                "options": p.options,
                "expires_in": p.expires_in,
                "multiple": p.multiple,
                "hide_totals": p.hide_totals,
            })),
        });
        let row = sqlx::query!(
            r#"INSERT INTO scheduled_statuses (account_id, scheduled_at, params)
               VALUES ($1, $2, $3)
               RETURNING id, scheduled_at"#,
            account.id, scheduled_at, params,
        )
        .fetch_one(&state.db)
        .await?;
        let resp = ScheduledStatusResponse {
            id: row.id.to_string(),
            scheduled_at: row.scheduled_at.map(|t| t.to_rfc3339()),
            params,
            media_attachments: vec![],
        };
        return Ok((axum::http::StatusCode::CREATED, Json(resp)).into_response());
    }

    let visibility = form.visibility.as_deref().unwrap_or("public");
    let in_reply_to_id = form.in_reply_to_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let hashtags = extract_hashtags(&text);
    let mention_handles = extract_mention_handles(&text);
    let resolved = resolve_mention_accounts(&state, instance.id, &mention_handles).await;
    let mention_map = build_mention_map(&resolved);
    let content = render_content(&text, &instance.domain, &mention_map);

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

    // Store tags and mentions
    store_status_tags(&state, status.id, &hashtags).await?;
    store_status_mentions(&state, status.id, &resolved).await?;

    // Manage conversation for direct messages
    if visibility == "direct" {
        let conv_id = if let Some(parent_id) = in_reply_to_id {
            sqlx::query_scalar!(
                "SELECT conversation_id FROM statuses WHERE id = $1",
                parent_id
            )
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten()
            .flatten()
        } else {
            None
        };

        let conv_id = if let Some(cid) = conv_id {
            cid
        } else {
            sqlx::query_scalar!(
                "INSERT INTO conversations (instance_id) VALUES ($1) RETURNING id",
                instance.id
            )
            .fetch_one(&state.db)
            .await?
        };

        sqlx::query!(
            "UPDATE statuses SET conversation_id = $1 WHERE id = $2",
            conv_id, status.id
        )
        .execute(&state.db)
        .await?;

        sqlx::query!(
            "UPDATE conversations SET updated_at = now() WHERE id = $1",
            conv_id
        )
        .execute(&state.db)
        .await?;

        sqlx::query!(
            "INSERT INTO conversation_participants (conversation_id, account_id, unread)
             VALUES ($1, $2, false)
             ON CONFLICT (conversation_id, account_id) DO UPDATE SET unread = false",
            conv_id, account.id
        )
        .execute(&state.db)
        .await?;

        for (_, mentioned) in &resolved {
            sqlx::query!(
                "INSERT INTO conversation_participants (conversation_id, account_id, unread)
                 VALUES ($1, $2, true)
                 ON CONFLICT (conversation_id, account_id) DO UPDATE SET unread = true",
                conv_id, mentioned.id
            )
            .execute(&state.db)
            .await?;
        }
    }

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

    // Create poll if requested
    if let Some(ref poll_form) = form.poll {
        if poll_form.options.len() >= 2 {
            let expires_at = poll_form.expires_in.map(|secs| chrono::Utc::now() + chrono::Duration::seconds(secs));
            let options_json: serde_json::Value = serde_json::Value::Array(
                poll_form.options.iter().map(|o| serde_json::json!({ "title": o, "votes_count": 0 })).collect()
            );
            sqlx::query!(
                r#"INSERT INTO polls (status_id, account_id, options, multiple, expires_at)
                   VALUES ($1, $2, $3, $4, $5)"#,
                status.id, account.id, options_json,
                poll_form.multiple.unwrap_or(false),
                expires_at,
            )
            .execute(&state.db)
            .await?;
        }
    }

    let mut status = status;
    status.uri = Some(uri);

    let media = fetch_status_media(&state, status.id).await?;
    let api_status = build_status(&state, &status, &account, media, None, None).await?;

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
    let mut notified = std::collections::HashSet::new();
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
            notified.insert(parent.account_id);
        }
    }

    // Notify each mentioned account not already notified above
    for (_, mentioned) in &resolved {
        if mentioned.id == account.id || notified.contains(&mentioned.id) {
            continue;
        }
        push::create_and_push(
            &state,
            mentioned.id,
            account.id,
            "mention",
            Some(status.id),
            format!("{} mentioned you", account.display_name),
            account.acct().clone(),
            account.avatar.clone().unwrap_or_default(),
        ).await;
        notified.insert(mentioned.id);
    }

    Ok((axum::http::StatusCode::OK, Json(api_status)).into_response())
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
                "scheduled_at" => form.scheduled_at = if text.is_empty() { None } else { Some(text) },
                "media_ids[]" | "media_ids" => {
                    if !text.is_empty() {
                        media_ids.push(text);
                    }
                }
                name if name.starts_with("poll[options]") || name == "poll[options][]" => {
                    if !text.is_empty() {
                        let p = form.poll.get_or_insert_with(PollForm::default);
                        p.options.push(text);
                    }
                }
                "poll[expires_in]" => {
                    if let Ok(n) = text.parse::<i64>() {
                        form.poll.get_or_insert_with(PollForm::default).expires_in = Some(n);
                    }
                }
                "poll[multiple]" => {
                    form.poll.get_or_insert_with(PollForm::default).multiple = Some(text == "true" || text == "1");
                }
                "poll[hide_totals]" => {
                    form.poll.get_or_insert_with(PollForm::default).hide_totals = Some(text == "true" || text == "1");
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

    Ok(Json(build_status(&state, &status, &account, media, reblog, viewer_ctx).await?))
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
    let mut s = build_status(&state, &status, &account, media, reblog, None).await?;
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

    Ok(Json(build_status(&state, &status, &account, media, reblog, Some(ctx)).await?))
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
    Ok(Json(build_status(&state, &status, &account, media, reblog, Some(ctx)).await?))
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

    // Build viewer context against the ORIGINAL so the nested reblog object
    // carries correct favourited/bookmarked/reblogged flags for the iOS client.
    let ctx = build_viewer_context(&state, auth.account_id, id).await?;
    let media = fetch_status_media(&state, boost.id).await?;
    let reblog = fetch_reblog_data(&state, &boost).await?;
    let api_boost = build_status(&state, &boost, &boost_account, media, reblog, Some(ctx)).await?;

    if let Ok(payload) = serde_json::to_string(&api_boost) {
        state.streaming.publish(Event::NewStatus {
            instance_id: instance.id,
            author_id: boost_account.id,
            is_public: original.visibility == "public",
            status_id: boost.id,
            payload: std::sync::Arc::new(payload),
        });
    }

    Ok(Json(api_boost))
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
        ancestors.push(build_status(&state, s, &acct, media, reblog, ctx).await?);
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
        descendants.push(build_status(&state, s, &acct, media, reblog, ctx).await?);
    }

    Ok(Json(StatusContext { ancestors, descendants }))
}

// ── POST /api/v1/statuses/:id/unreblog ────────────────────────────────────

pub async fn unreblog_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Status>> {
    let (status_raw, _) = fetch_status_with_account(&state, id).await?;

    // Accept both the original status ID and the reblog's own ID.
    // When iOS sends the reblog wrapper's ID, resolve it to the original.
    let original_id = status_raw.reblog_of_id.unwrap_or(id);

    let deleted = sqlx::query!(
        "DELETE FROM statuses WHERE account_id = $1 AND reblog_of_id = $2 AND deleted_at IS NULL RETURNING id",
        auth.account_id, original_id
    )
    .fetch_optional(&state.db)
    .await?;

    if deleted.is_some() {
        sqlx::query!(
            "UPDATE statuses SET reblogs_count = GREATEST(reblogs_count - 1, 0) WHERE id = $1",
            original_id
        )
        .execute(&state.db)
        .await?;
    }

    let (original, account) = fetch_status_with_account(&state, original_id).await?;
    let media = fetch_status_media(&state, original_id).await?;
    let reblog = fetch_reblog_data(&state, &original).await?;
    let ctx = build_viewer_context(&state, auth.account_id, original_id).await?;
    Ok(Json(build_status(&state, &original, &account, media, reblog, Some(ctx)).await?))
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
    Ok(Json(build_status(&state, &status, &account, media, reblog, Some(ctx)).await?))
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
    Ok(Json(build_status(&state, &status, &account, media, reblog, Some(ctx)).await?))
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
    Ok(Json(build_status(&state, &status, &account, media, reblog, Some(ctx)).await?))
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
    Ok(Json(build_status(&state, &status, &account, media, reblog, Some(ctx)).await?))
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
    Ok(Json(build_status(&state, &status, &account, media, reblog, Some(ctx)).await?))
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
    Ok(Json(build_status(&state, &status, &account, media, reblog, Some(ctx)).await?))
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

    let instance_domain = sqlx::query_scalar!(
        "SELECT domain FROM instances WHERE id = $1",
        status.instance_id,
    )
    .fetch_one(&state.db)
    .await?;

    let new_text = form.status.unwrap_or_else(|| status.text.clone());
    let hashtags = extract_hashtags(&new_text);
    let mention_handles = extract_mention_handles(&new_text);
    let resolved = resolve_mention_accounts(&state, status.instance_id, &mention_handles).await;
    let mention_map = build_mention_map(&resolved);
    let new_content = render_content(&new_text, &instance_domain, &mention_map);
    let new_spoiler = form.spoiler_text.unwrap_or_else(|| status.spoiler_text.clone());
    let new_sensitive = form.sensitive.unwrap_or(status.sensitive);
    let new_language = form.language.or(status.language.clone());

    sqlx::query!(
        "UPDATE statuses SET text = $1, content = $2, spoiler_text = $3, sensitive = $4, language = $5, edited_at = now() WHERE id = $6",
        new_text, new_content, new_spoiler, new_sensitive, new_language, id,
    )
    .execute(&state.db)
    .await?;

    store_status_tags(&state, id, &hashtags).await?;
    store_status_mentions(&state, id, &resolved).await?;

    let (updated_status, _) = fetch_status_with_account(&state, id).await?;
    let media = fetch_status_media(&state, id).await?;
    let reblog = fetch_reblog_data(&state, &updated_status).await?;
    let ctx = build_viewer_context(&state, auth.account_id, id).await?;
    Ok(Json(build_status(&state, &updated_status, &account, media, reblog, Some(ctx)).await?))
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

pub async fn build_viewer_context(
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

pub fn render_content(
    text: &str,
    domain: &str,
    mention_map: &HashMap<String, (String, String)>,
) -> String {
    if text.is_empty() {
        return String::new();
    }
    text.split("\n\n")
        .map(|para| {
            let linked = linkify_entities(para, domain, mention_map);
            format!("<p>{}</p>", linked.replace('\n', "<br />"))
        })
        .collect::<Vec<_>>()
        .join("")
}

fn linkify_entities(
    text: &str,
    domain: &str,
    mention_map: &HashMap<String, (String, String)>,
) -> String {
    struct Entity {
        start: usize,
        end: usize,
        html: String,
    }

    let mut entities: Vec<Entity> = Vec::new();

    for cap in HASHTAG_RE.captures_iter(text) {
        let full = cap.get(0).unwrap();
        let prefix_len = cap.get(1).unwrap().as_str().len();
        let tag_text = &cap[2];
        let tag_lower = tag_text.to_lowercase();
        let url = format!("https://{}/tags/{}", domain, urlencoding::encode(&tag_lower));
        entities.push(Entity {
            start: full.start() + prefix_len,
            end: full.end(),
            html: format!(
                r#"<a href="{}" class="mention hashtag" rel="tag">#<span>{}</span></a>"#,
                ammonia::clean_text(&url),
                ammonia::clean_text(tag_text),
            ),
        });
    }

    for cap in MENTION_RE.captures_iter(text) {
        let full = cap.get(0).unwrap();
        let prefix_len = cap.get(1).unwrap().as_str().len();
        let username = cap[2].to_lowercase();
        let mention_domain = cap.get(3).map(|m| m.as_str().to_lowercase());
        let key = match &mention_domain {
            Some(d) => format!("{}@{}", username, d),
            None => username.clone(),
        };
        if let Some((url, display)) = mention_map.get(&key) {
            entities.push(Entity {
                start: full.start() + prefix_len,
                end: full.end(),
                html: format!(
                    r#"<span class="h-card" translate="no"><a href="{}" class="u-url mention">@<span>{}</span></a></span>"#,
                    ammonia::clean_text(url),
                    ammonia::clean_text(display),
                ),
            });
        }
    }

    for m in URL_RE.find_iter(text) {
        let url = m.as_str();
        entities.push(Entity {
            start: m.start(),
            end: m.end(),
            html: format!(
                r#"<a href="{}" target="_blank" rel="nofollow noopener noreferrer">{}</a>"#,
                ammonia::clean_text(url),
                ammonia::clean_text(url),
            ),
        });
    }

    entities.sort_by_key(|e| e.start);

    let mut result = String::with_capacity(text.len() * 2);
    let mut last_end = 0usize;
    for entity in &entities {
        if entity.start < last_end {
            continue;
        }
        result.push_str(&ammonia::clean_text(&text[last_end..entity.start]));
        result.push_str(&entity.html);
        last_end = entity.end;
    }
    result.push_str(&ammonia::clean_text(&text[last_end..]));
    result
}

pub fn extract_hashtags(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    HASHTAG_RE.captures_iter(text)
        .filter_map(|c| {
            let tag = c[2].to_lowercase();
            if seen.insert(tag.clone()) { Some(tag) } else { None }
        })
        .collect()
}

pub fn extract_mention_handles(text: &str) -> Vec<(String, Option<String>)> {
    let mut seen = std::collections::HashSet::new();
    MENTION_RE.captures_iter(text)
        .filter_map(|c| {
            let username = c[2].to_lowercase();
            let domain = c.get(3).map(|m| m.as_str().to_lowercase());
            let key = match &domain {
                Some(d) => format!("{}@{}", username, d),
                None => username.clone(),
            };
            if seen.insert(key) { Some((username, domain)) } else { None }
        })
        .collect()
}

pub async fn resolve_mention_accounts(
    state: &AppState,
    instance_id: Uuid,
    handles: &[(String, Option<String>)],
) -> Vec<(String, Account)> {
    let mut result = Vec::new();
    for (username, domain) in handles {
        let account = if let Some(d) = domain {
            sqlx::query_as!(
                Account,
                "SELECT * FROM accounts WHERE LOWER(username) = $1 AND domain = $2 LIMIT 1",
                username, d,
            )
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten()
        } else {
            sqlx::query_as!(
                Account,
                "SELECT * FROM accounts WHERE instance_id = $1 AND LOWER(username) = $2 AND domain IS NULL LIMIT 1",
                instance_id, username,
            )
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten()
        };
        if let Some(acct) = account {
            result.push((username.clone(), acct));
        }
    }
    result
}

pub fn build_mention_map(resolved: &[(String, Account)]) -> HashMap<String, (String, String)> {
    let mut map = HashMap::new();
    for (username_lower, account) in resolved {
        let url = account.url.clone();
        let display = account.acct();
        map.insert(username_lower.clone(), (url.clone(), display.clone()));
        if let Some(ref d) = account.domain {
            map.insert(format!("{}@{}", username_lower, d.to_lowercase()), (url, display));
        }
    }
    map
}

pub async fn store_status_tags(state: &AppState, status_id: i64, hashtags: &[String]) -> AppResult<()> {
    sqlx::query!("DELETE FROM status_tags WHERE status_id = $1", status_id)
        .execute(&state.db)
        .await?;
    for tag_name in hashtags {
        let tag_id = sqlx::query_scalar!(
            "INSERT INTO tags (name) VALUES ($1)
             ON CONFLICT (name) DO UPDATE SET updated_at = now()
             RETURNING id",
            tag_name,
        )
        .fetch_one(&state.db)
        .await?;
        sqlx::query!(
            "INSERT INTO status_tags (status_id, tag_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            status_id, tag_id,
        )
        .execute(&state.db)
        .await?;
    }
    Ok(())
}

pub async fn store_status_mentions(
    state: &AppState,
    status_id: i64,
    resolved: &[(String, Account)],
) -> AppResult<()> {
    sqlx::query!("DELETE FROM mentions WHERE status_id = $1", status_id)
        .execute(&state.db)
        .await?;
    for (_, account) in resolved {
        sqlx::query!(
            "INSERT INTO mentions (status_id, account_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            status_id, account.id,
        )
        .execute(&state.db)
        .await?;
    }
    Ok(())
}
