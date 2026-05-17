use axum::{
    extract::{Extension, Path, Query, State},
    http::{header, HeaderMap, Uri},
    response::IntoResponse,
    Json,
};

use crate::{
    db::models::Account,
    error::{AppError, AppResult},
    middleware::AuthenticatedUser,
    state::AppState,
};
use super::{
    accounts::{
        batch_reblog_data, batch_status_cards, batch_status_emojis, batch_status_media,
        batch_status_mentions, batch_status_polls, batch_status_tags,
        build_status, fetch_account, fetch_reblog_data, fetch_status_media,
    },
    convert::{account_from_db, status_from_db},
    statuses::{batch_viewer_contexts, build_viewer_context},
    types::{Conversation, PaginationParams},
};

struct ConvRow {
    id: i64,
    unread: bool,
}

// ── GET /api/v1/conversations ─────────────────────────────────────────────

pub async fn get_conversations(
    State(state): State<AppState>,
    Query(pagination): Query<PaginationParams>,
    uri: Uri,
    req_headers: HeaderMap,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<impl IntoResponse> {
    let limit = pagination.limit_clamped(20, 40);
    let max_id = pagination.max_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let since_id = pagination.since_id.as_deref().and_then(|s| s.parse::<i64>().ok());
    let min_id = pagination.min_id.as_deref().and_then(|s| s.parse::<i64>().ok());

    let rows: Vec<ConvRow> = if min_id.is_some() {
        sqlx::query_as!(
            ConvRow,
            r#"SELECT c.id, cp.unread
               FROM conversations c
               JOIN conversation_participants cp ON cp.conversation_id = c.id
               WHERE cp.account_id = $1
                 AND ($2::bigint IS NULL OR c.id > $2)
               ORDER BY c.id ASC
               LIMIT $3"#,
            auth.account_id,
            min_id,
            limit,
        )
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as!(
            ConvRow,
            r#"SELECT c.id, cp.unread
               FROM conversations c
               JOIN conversation_participants cp ON cp.conversation_id = c.id
               WHERE cp.account_id = $1
                 AND ($2::bigint IS NULL OR c.id < $2)
                 AND ($4::bigint IS NULL OR c.id > $4)
               ORDER BY c.updated_at DESC
               LIMIT $3"#,
            auth.account_id,
            max_id,
            limit,
            since_id,
        )
        .fetch_all(&state.db)
        .await?
    };

    if rows.is_empty() {
        return Ok((HeaderMap::new(), Json(vec![])));
    }

    let conv_ids: Vec<i64> = rows.iter().map(|r| r.id).collect();

    // Batch fetch participant links then accounts
    struct PartLink { conversation_id: i64, account_id: i64 }
    let part_links = sqlx::query_as!(
        PartLink,
        r#"SELECT conversation_id, account_id FROM conversation_participants
           WHERE conversation_id = ANY($1::bigint[]) AND account_id != $2"#,
        &conv_ids,
        auth.account_id,
    )
    .fetch_all(&state.db)
    .await?;

    let all_participant_ids: Vec<i64> = part_links.iter()
        .map(|r| r.account_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let participant_accounts_vec: Vec<Account> = if !all_participant_ids.is_empty() {
        sqlx::query_as!(
            Account,
            "SELECT * FROM accounts WHERE id = ANY($1::bigint[])",
            &all_participant_ids,
        )
        .fetch_all(&state.db)
        .await?
    } else {
        vec![]
    };
    let participant_acct_map: std::collections::HashMap<i64, Account> =
        participant_accounts_vec.into_iter().map(|a| (a.id, a)).collect();
    let mut participants_by_conv: std::collections::HashMap<i64, Vec<&Account>> =
        std::collections::HashMap::new();
    for link in &part_links {
        if let Some(acct) = participant_acct_map.get(&link.account_id) {
            participants_by_conv.entry(link.conversation_id).or_default().push(acct);
        }
    }

    // Batch fetch last status per conversation via DISTINCT ON
    let last_statuses: Vec<crate::db::models::Status> = sqlx::query_as!(
        crate::db::models::Status,
        r#"SELECT DISTINCT ON (conversation_id) *
           FROM statuses
           WHERE conversation_id = ANY($1::bigint[]) AND deleted_at IS NULL
           ORDER BY conversation_id, id DESC"#,
        &conv_ids,
    )
    .fetch_all(&state.db)
    .await?;

    // Batch enrich all last statuses
    let status_ids: Vec<i64> = last_statuses.iter().map(|s| s.id).collect();
    let mut enriched_map: std::collections::HashMap<i64, super::types::Status> =
        std::collections::HashMap::new();

    if !status_ids.is_empty() {
        let media_map = batch_status_media(&state, &status_ids).await?;
        let reblog_map = batch_reblog_data(&state, &last_statuses).await?;
        let reblog_ids: Vec<i64> = reblog_map.values().map(|(rs, _, _)| rs.id).collect();
        let mut enrich_ids = status_ids.clone();
        enrich_ids.extend_from_slice(&reblog_ids);
        let tags_map = batch_status_tags(&state, &enrich_ids).await?;
        let mentions_map = batch_status_mentions(&state, &enrich_ids).await?;
        let all_for_emoji: Vec<crate::db::models::Status> = last_statuses.iter().cloned()
            .chain(reblog_map.values().map(|(rs, _, _)| rs.clone()))
            .collect();
        let emojis_map = batch_status_emojis(&state, &all_for_emoji).await?;
        let polls_map = batch_status_polls(&state, &enrich_ids, Some(auth.account_id)).await?;
        let cards_map = batch_status_cards(&state, &enrich_ids).await?;
        let ctxs = batch_viewer_contexts(&state, auth.account_id, &status_ids).await?;

        let status_account_ids: Vec<i64> = last_statuses.iter().map(|s| s.account_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        let status_accounts: Vec<Account> = sqlx::query_as!(
            Account,
            "SELECT * FROM accounts WHERE id = ANY($1::bigint[])",
            &status_account_ids,
        )
        .fetch_all(&state.db)
        .await?;
        let status_account_map: std::collections::HashMap<i64, Account> =
            status_accounts.into_iter().map(|a| (a.id, a)).collect();

        for s in &last_statuses {
            let Some(conv_id) = s.conversation_id else { continue };
            let Some(account) = status_account_map.get(&s.account_id) else { continue };
            let media = media_map.get(&s.id).cloned().unwrap_or_default();
            let reblog = reblog_map.get(&s.id).cloned();
            let ctx = ctxs.get(&s.id).cloned();
            let mentions = mentions_map.get(&s.id).cloned().unwrap_or_default();
            let rb_mentions = reblog.as_ref()
                .and_then(|(rs, _, _)| mentions_map.get(&rs.id))
                .cloned()
                .unwrap_or_default();
            let mut api = status_from_db(s, account, media, reblog, ctx, &mentions, &rb_mentions);
            api.tags = tags_map.get(&s.id).cloned().unwrap_or_default();
            api.mentions = mentions;
            api.emojis = emojis_map.get(&s.id).cloned().unwrap_or_default();
            api.poll = polls_map.get(&s.id).cloned();
            api.card = cards_map.get(&s.id).cloned();
            if let Some(ref mut rb) = api.reblog {
                let rid: i64 = rb.id.parse().unwrap_or(0);
                rb.tags = tags_map.get(&rid).cloned().unwrap_or_default();
                rb.mentions = rb_mentions;
                rb.emojis = emojis_map.get(&rid).cloned().unwrap_or_default();
                rb.poll = polls_map.get(&rid).cloned();
                rb.card = cards_map.get(&rid).cloned();
            }
            enriched_map.insert(conv_id, api);
        }
    }

    let mut result = Vec::with_capacity(rows.len());
    for row in &rows {
        result.push(Conversation {
            id: row.id.to_string(),
            unread: row.unread,
            accounts: participants_by_conv.get(&row.id)
                .map(|v| v.iter().map(|a| account_from_db(a)).collect())
                .unwrap_or_default(),
            last_status: enriched_map.remove(&row.id),
        });
    }

    let link = result.first().zip(result.last()).map(|(newest, oldest)| {
        let extra = super::non_pagination_query(uri.query());
        super::link_header(&req_headers, uri.path(), &extra, &newest.id, &oldest.id)
    });
    let mut resp_headers = HeaderMap::new();
    if let Some(v) = link {
        if let Ok(val) = v.parse() {
            resp_headers.insert(header::LINK, val);
        }
    }
    Ok((resp_headers, Json(result)))
}

// ── DELETE /api/v1/conversations/:id ─────────────────────────────────────

pub async fn delete_conversation(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<serde_json::Value>> {
    let deleted = sqlx::query!(
        "DELETE FROM conversation_participants WHERE conversation_id = $1 AND account_id = $2 RETURNING conversation_id",
        id,
        auth.account_id,
    )
    .fetch_optional(&state.db)
    .await?;

    if deleted.is_none() {
        return Err(AppError::NotFound);
    }

    Ok(Json(serde_json::json!({})))
}

// ── POST /api/v1/conversations/:id/unread ────────────────────────────────

pub async fn mark_conversation_unread(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Conversation>> {
    let updated = sqlx::query!(
        "UPDATE conversation_participants SET unread = true WHERE conversation_id = $1 AND account_id = $2 RETURNING conversation_id",
        id,
        auth.account_id,
    )
    .fetch_optional(&state.db)
    .await?;

    if updated.is_none() {
        return Err(AppError::NotFound);
    }

    let participants = sqlx::query_as!(
        Account,
        r#"SELECT a.* FROM accounts a
           JOIN conversation_participants cp ON cp.account_id = a.id
           WHERE cp.conversation_id = $1 AND a.id != $2"#,
        id,
        auth.account_id,
    )
    .fetch_all(&state.db)
    .await?;

    let last = sqlx::query_as!(
        crate::db::models::Status,
        "SELECT * FROM statuses WHERE conversation_id = $1 AND deleted_at IS NULL ORDER BY id DESC LIMIT 1",
        id,
    )
    .fetch_optional(&state.db)
    .await?;

    let last_status = if let Some(s) = last {
        let saccount = fetch_account(&state, s.account_id).await?;
        let media = fetch_status_media(&state, s.id).await?;
        let reblog = fetch_reblog_data(&state, &s).await?;
        let ctx = build_viewer_context(&state, auth.account_id, s.id).await?;
        Some(build_status(&state, &s, &saccount, media, reblog, Some(ctx)).await?)
    } else {
        None
    };

    Ok(Json(Conversation {
        id: id.to_string(),
        unread: true,
        accounts: participants.iter().map(account_from_db).collect(),
        last_status,
    }))
}

// ── POST /api/v1/conversations/:id/read ──────────────────────────────────

pub async fn mark_conversation_read(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Conversation>> {
    let updated = sqlx::query!(
        "UPDATE conversation_participants SET unread = false WHERE conversation_id = $1 AND account_id = $2 RETURNING conversation_id",
        id,
        auth.account_id,
    )
    .fetch_optional(&state.db)
    .await?;

    if updated.is_none() {
        return Err(AppError::NotFound);
    }

    let participants = sqlx::query_as!(
        Account,
        r#"SELECT a.* FROM accounts a
           JOIN conversation_participants cp ON cp.account_id = a.id
           WHERE cp.conversation_id = $1 AND a.id != $2"#,
        id,
        auth.account_id,
    )
    .fetch_all(&state.db)
    .await?;

    let last = sqlx::query_as!(
        crate::db::models::Status,
        "SELECT * FROM statuses WHERE conversation_id = $1 AND deleted_at IS NULL ORDER BY id DESC LIMIT 1",
        id,
    )
    .fetch_optional(&state.db)
    .await?;

    let last_status = if let Some(s) = last {
        let saccount = fetch_account(&state, s.account_id).await?;
        let media = fetch_status_media(&state, s.id).await?;
        let reblog = fetch_reblog_data(&state, &s).await?;
        let ctx = build_viewer_context(&state, auth.account_id, s.id).await?;
        Some(build_status(&state, &s, &saccount, media, reblog, Some(ctx)).await?)
    } else {
        None
    };

    Ok(Json(Conversation {
        id: id.to_string(),
        unread: false,
        accounts: participants.iter().map(account_from_db).collect(),
        last_status,
    }))
}
