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
use super::{
    convert::account_from_db,
    types::{Account, List},
};

// ── GET /api/v1/lists ──────────────────────────────────────────────────────

pub async fn get_lists(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<List>>> {
    auth.require_scope("read:lists")?;
    let lists = sqlx::query_as!(
        models::List,
        "SELECT * FROM lists WHERE account_id = $1 ORDER BY id ASC",
        auth.account_id,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(lists.iter().map(list_from_db).collect()))
}

// ── GET /api/v1/lists/:id ─────────────────────────────────────────────────

pub async fn get_list(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<List>> {
    auth.require_scope("read:lists")?;
    let list = fetch_list(&state, id, auth.account_id).await?;
    Ok(Json(list_from_db(&list)))
}

// ── POST /api/v1/lists ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListForm {
    pub title: String,
    pub replies_policy: Option<String>,
    pub exclusive: Option<bool>,
}

const VALID_REPLIES_POLICIES: &[&str] = &["followed", "list", "none"];

pub async fn create_list(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<ListForm>,
) -> AppResult<Json<List>> {
    auth.require_scope("write:lists")?;
    if form.title.trim().is_empty() {
        return Err(AppError::Unprocessable("Title can't be blank".into()));
    }
    let replies_policy = form.replies_policy.as_deref().unwrap_or("list");
    if !VALID_REPLIES_POLICIES.contains(&replies_policy) {
        return Err(AppError::Unprocessable(
            format!("Replies policy is not included in the list: {replies_policy}"),
        ));
    }
    let list = sqlx::query_as!(
        models::List,
        r#"INSERT INTO lists (account_id, title, replies_policy, exclusive)
           VALUES ($1, $2, $3, $4)
           RETURNING *"#,
        auth.account_id,
        form.title,
        replies_policy,
        form.exclusive.unwrap_or(false),
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(list_from_db(&list)))
}

// ── PUT /api/v1/lists/:id ─────────────────────────────────────────────────

pub async fn update_list(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<ListForm>,
) -> AppResult<Json<List>> {
    auth.require_scope("write:lists")?;
    fetch_list(&state, id, auth.account_id).await?;

    let list = sqlx::query_as!(
        models::List,
        r#"UPDATE lists SET title = $1, replies_policy = $2, exclusive = $3, updated_at = now()
           WHERE id = $4 AND account_id = $5
           RETURNING *"#,
        form.title,
        form.replies_policy.as_deref().unwrap_or("list"),
        form.exclusive.unwrap_or(false),
        id,
        auth.account_id,
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(list_from_db(&list)))
}

// ── DELETE /api/v1/lists/:id ──────────────────────────────────────────────

pub async fn delete_list(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<serde_json::Value>> {
    auth.require_scope("write:lists")?;
    fetch_list(&state, id, auth.account_id).await?;
    sqlx::query!(
        "DELETE FROM lists WHERE id = $1 AND account_id = $2",
        id, auth.account_id
    )
    .execute(&state.db)
    .await?;
    Ok(Json(serde_json::json!({})))
}

// ── GET /api/v1/lists/:id/accounts ───────────────────────────────────────

pub async fn get_list_accounts(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<Account>>> {
    auth.require_scope("read:lists")?;
    fetch_list(&state, id, auth.account_id).await?;

    let accounts = sqlx::query_as!(
        models::Account,
        r#"SELECT a.* FROM accounts a
           JOIN list_accounts la ON la.account_id = a.id
           WHERE la.list_id = $1
           ORDER BY a.username ASC"#,
        id,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(accounts.iter().map(account_from_db).collect()))
}

// ── POST /api/v1/lists/:id/accounts ──────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListAccountsForm {
    pub account_ids: Vec<String>,
}

pub async fn add_list_accounts(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<ListAccountsForm>,
) -> AppResult<Json<serde_json::Value>> {
    auth.require_scope("write:lists")?;
    fetch_list(&state, id, auth.account_id).await?;

    for id_str in &form.account_ids {
        if let Ok(account_id) = id_str.parse::<Uuid>() {
            sqlx::query!(
                "INSERT INTO list_accounts (list_id, account_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                id, account_id,
            )
            .execute(&state.db)
            .await?;
        }
    }

    Ok(Json(serde_json::json!({})))
}

// ── DELETE /api/v1/lists/:id/accounts ────────────────────────────────────

pub async fn remove_list_accounts(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<ListAccountsForm>,
) -> AppResult<Json<serde_json::Value>> {
    auth.require_scope("write:lists")?;
    fetch_list(&state, id, auth.account_id).await?;

    for id_str in &form.account_ids {
        if let Ok(account_id) = id_str.parse::<Uuid>() {
            sqlx::query!(
                "DELETE FROM list_accounts WHERE list_id = $1 AND account_id = $2",
                id, account_id,
            )
            .execute(&state.db)
            .await?;
        }
    }

    Ok(Json(serde_json::json!({})))
}

// ── Helpers ────────────────────────────────────────────────────────────────

async fn fetch_list(state: &AppState, id: i64, account_id: Uuid) -> AppResult<models::List> {
    sqlx::query_as!(
        models::List,
        "SELECT * FROM lists WHERE id = $1 AND account_id = $2",
        id, account_id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

fn list_from_db(l: &models::List) -> List {
    List {
        id: l.id.to_string(),
        title: l.title.clone(),
        replies_policy: l.replies_policy.clone(),
        exclusive: l.exclusive,
    }
}
