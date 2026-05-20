use axum::{
    extract::{Extension, State},
    Json,
};
use serde::Deserialize;

use crate::{
    error::{AppError, AppResult},
    middleware::AuthenticatedUser,
    state::AppState,
    push::notify_admins,
};
use super::{
    convert::account_from_db,
    types::Report,
};
use crate::middleware::ResolvedInstance;

fn de_i64_from_str_or_num<'de, D: serde::Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StrOrNum { S(String), N(i64) }
    match StrOrNum::deserialize(d)? {
        StrOrNum::S(s) => s.parse().map_err(serde::de::Error::custom),
        StrOrNum::N(n) => Ok(n),
    }
}

// ── POST /api/v1/reports ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ReportForm {
    #[serde(deserialize_with = "de_i64_from_str_or_num")]
    pub account_id: i64,
    pub status_ids: Option<Vec<String>>,
    pub comment: Option<String>,
    pub forward: Option<bool>,
    pub category: Option<String>,
}

pub async fn file_report(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<crate::middleware::ResolvedInstance>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(form): Json<ReportForm>,
) -> AppResult<Json<Report>> {
    auth.require_scope("write:reports")?;
    let target_account = sqlx::query_as!(
        crate::db::models::Account,
        "SELECT * FROM accounts WHERE id = $1",
        form.account_id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let status_ids: Vec<i64> = form.status_ids
        .unwrap_or_default()
        .iter()
        .filter_map(|s| s.parse::<i64>().ok())
        .collect();

    // Verify all provided statuses belong to the target account.
    for &sid in &status_ids {
        let belongs = sqlx::query_scalar!(
            "SELECT 1 as e FROM statuses WHERE id = $1 AND account_id = $2 AND deleted_at IS NULL",
            sid, form.account_id,
        )
        .fetch_optional(&state.db)
        .await?;
        if belongs.is_none() {
            return Err(AppError::NotFound);
        }
    }

    let comment = form.comment.unwrap_or_default();
    let forwarded = form.forward.unwrap_or(false);
    let category = form.category.unwrap_or_else(|| "other".into());
    let category_int = crate::db::models::report_category::from_str(&category);

    let report = sqlx::query!(
        r#"INSERT INTO reports (account_id, target_account_id, status_ids, comment, forwarded, category)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id, created_at"#,
        auth.account_id,
        form.account_id,
        &status_ids,
        comment,
        forwarded,
        category_int,
    )
    .fetch_one(&state.db)
    .await?;

    let status_id_strings: Vec<String> = status_ids.iter().map(|id| id.to_string()).collect();

    // Notify admins/moderators about the new report.
    {
        let state2 = state.clone();
        let reporter_id = auth.account_id;
        let rid = report.id;
        let iid = instance.id;
        tokio::spawn(async move {
            notify_admins(&state2, iid, reporter_id, "admin.report", Some(rid)).await;
        });
    }

    Ok(Json(Report {
        id: report.id.to_string(),
        action_taken: false,
        action_taken_at: None,
        category,
        comment,
        forwarded,
        created_at: super::convert::mastodon_date(report.created_at),
        status_ids: status_id_strings,
        rule_ids: vec![],
        collection_ids: vec![],
        target_account: account_from_db(&target_account),
    }))
}
