use axum::{extract::State, response::Json, Extension};
use serde::Serialize;
use crate::{
    error::AppResult,
    middleware::AuthenticatedUser,
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct ScheduledStatus {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_at: Option<String>,
    pub params: serde_json::Value,
    pub media_attachments: Vec<serde_json::Value>,
}

pub async fn list_scheduled_statuses(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<Vec<ScheduledStatus>>> {
    let rows = sqlx::query!(
        r#"SELECT id, scheduled_at, params
           FROM scheduled_statuses
           WHERE account_id = $1
           ORDER BY scheduled_at ASC NULLS LAST, created_at ASC"#,
        auth.account_id,
    )
    .fetch_all(&state.db)
    .await?;

    let statuses = rows
        .into_iter()
        .map(|r| ScheduledStatus {
            id: r.id.to_string(),
            scheduled_at: r.scheduled_at.map(|t| t.to_rfc3339()),
            params: r.params.unwrap_or(serde_json::Value::Null),
            media_attachments: vec![],
        })
        .collect();

    Ok(Json(statuses))
}
