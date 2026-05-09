use axum::{
    extract::{Extension, Json, Path, State},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    db::models,
    error::{AppError, AppResult},
    middleware::AuthenticatedUser,
    state::AppState,
};
use super::types::{Poll, PollOption};

// ── GET /api/v1/polls/:id ─────────────────────────────────────────────────

pub async fn get_poll(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    auth: Option<Extension<AuthenticatedUser>>,
) -> AppResult<Json<Poll>> {
    let poll = fetch_poll(&state, id).await?;
    let viewer_id = auth.map(|Extension(a)| a.account_id);
    poll_from_db(&state, &poll, viewer_id).await.map(Json)
}

// ── POST /api/v1/polls/:id/votes ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PollVoteForm {
    pub choices: Vec<i32>,
}

pub async fn vote_poll(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<PollVoteForm>,
) -> AppResult<Json<Poll>> {
    let poll = fetch_poll(&state, id).await?;

    let expired = poll.expires_at.map(|e| e < chrono::Utc::now()).unwrap_or(false);
    if expired {
        return Err(AppError::Unprocessable("Poll has expired".into()));
    }

    let already_voted = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM poll_votes WHERE poll_id = $1 AND account_id = $2)",
        id, auth.account_id,
    )
    .fetch_one(&state.db)
    .await?
    .unwrap_or(false);

    if already_voted {
        return Err(AppError::Unprocessable("Already voted".into()));
    }

    let option_count = poll.options.as_array().map(|a| a.len()).unwrap_or(0) as i32;
    if !poll.multiple && form.choices.len() > 1 {
        return Err(AppError::Unprocessable("Multiple choices not allowed".into()));
    }

    for choice in &form.choices {
        if *choice < 0 || *choice >= option_count {
            return Err(AppError::Unprocessable("Invalid choice index".into()));
        }
        sqlx::query!(
            "INSERT INTO poll_votes (poll_id, account_id, choice) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            id, auth.account_id, choice,
        )
        .execute(&state.db)
        .await?;
    }

    sqlx::query!(
        "UPDATE polls SET votes_count = (SELECT COUNT(*) FROM poll_votes WHERE poll_id = $1) WHERE id = $1",
        id,
    )
    .execute(&state.db)
    .await?;

    let poll = fetch_poll(&state, id).await?;
    poll_from_db(&state, &poll, Some(auth.account_id)).await.map(Json)
}

// ── Helpers ────────────────────────────────────────────────────────────────

async fn fetch_poll(state: &AppState, id: Uuid) -> AppResult<models::Poll> {
    sqlx::query_as!(
        models::Poll,
        "SELECT * FROM polls WHERE id = $1",
        id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

async fn poll_from_db(state: &AppState, poll: &models::Poll, viewer_id: Option<Uuid>) -> AppResult<Poll> {
    let options: Vec<PollOption> = poll.options
        .as_array()
        .map(|arr| arr.iter().map(|o| PollOption {
            title: o["title"].as_str().unwrap_or("").to_string(),
            votes_count: o["votes_count"].as_i64(),
        }).collect())
        .unwrap_or_default();

    let (voted, own_votes) = if let Some(vid) = viewer_id {
        let votes = sqlx::query!(
            "SELECT choice FROM poll_votes WHERE poll_id = $1 AND account_id = $2 ORDER BY choice",
            poll.id, vid,
        )
        .fetch_all(&state.db)
        .await?;
        if votes.is_empty() {
            (Some(false), None)
        } else {
            let choices: Vec<i32> = votes.iter().map(|v| v.choice).collect();
            (Some(true), Some(choices))
        }
    } else {
        (None, None)
    };

    let expired = poll.expires_at.map(|e| e < chrono::Utc::now()).unwrap_or(false);

    Ok(Poll {
        id: poll.id.to_string(),
        expires_at: poll.expires_at.map(|e| e.to_rfc3339()),
        expired,
        multiple: poll.multiple,
        votes_count: poll.votes_count,
        voters_count: poll.voters_count,
        options,
        emojis: vec![],
        voted,
        own_votes,
    })
}
