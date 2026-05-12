use axum::{extract::{Extension, State}, Json};

use crate::{
    error::AppResult,
    middleware::AuthenticatedUser,
    state::AppState,
};
use super::types::{Filter, FilterKeyword, FilterV1};

// ── GET /api/v2/filters ───────────────────────────────────────────────────

pub async fn get_filters_v2(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<Filter>>> {
    let filters = sqlx::query!(
        "SELECT id, phrase, context, expires_at, action FROM custom_filters WHERE account_id = $1 ORDER BY id",
        auth.account_id,
    )
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::with_capacity(filters.len());
    for f in &filters {
        let keywords = sqlx::query!(
            "SELECT id, keyword, whole_word FROM custom_filter_keywords WHERE custom_filter_id = $1 ORDER BY id",
            f.id,
        )
        .fetch_all(&state.db)
        .await?;

        result.push(Filter {
            id: f.id.to_string(),
            title: f.phrase.clone(),
            context: f.context.clone(),
            expires_at: f.expires_at.map(|t| t.to_rfc3339()),
            filter_action: f.action.clone(),
            keywords: keywords
                .into_iter()
                .map(|k| FilterKeyword {
                    id: k.id.to_string(),
                    keyword: k.keyword,
                    whole_word: k.whole_word,
                })
                .collect(),
            statuses: vec![],
        });
    }

    Ok(Json(result))
}

// ── GET /api/v1/filters ───────────────────────────────────────────────────

pub async fn get_filters_v1(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<FilterV1>>> {
    let filters = sqlx::query!(
        "SELECT id, phrase, context, expires_at, action FROM custom_filters WHERE account_id = $1 ORDER BY id",
        auth.account_id,
    )
    .fetch_all(&state.db)
    .await?;

    let result = filters
        .into_iter()
        .map(|f| FilterV1 {
            id: f.id.to_string(),
            phrase: f.phrase,
            context: f.context,
            whole_word: true,
            expires_at: f.expires_at.map(|t| t.to_rfc3339()),
            irreversible: f.action == "hide",
        })
        .collect();

    Ok(Json(result))
}
