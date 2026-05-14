use axum::{
    body::Bytes,
    extract::{Extension, State},
    http::Uri,
    Json,
};
use std::collections::HashMap;

use crate::{
    error::AppResult,
    middleware::AuthenticatedUser,
    state::AppState,
};
use super::types::MarkerInfo;

// ── GET /api/v1/markers ───────────────────────────────────────────────────

pub async fn get_markers(
    State(state): State<AppState>,
    uri: Uri,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<HashMap<String, MarkerInfo>>> {
    auth.require_scope("read:statuses")?;
    let query = uri.query().unwrap_or("");
    let timelines: Vec<String> = query.split('&')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            if k == "timeline%5B%5D" || k == "timeline[]" {
                urlencoding::decode(v).ok().map(|s| s.into_owned())
            } else {
                None
            }
        })
        .collect();

    let mut result = HashMap::new();

    for timeline in &timelines {
        let row = sqlx::query!(
            "SELECT last_read_id, version, updated_at FROM markers WHERE account_id = $1 AND timeline = $2",
            auth.account_id, timeline.as_str()
        )
        .fetch_optional(&state.db)
        .await?;

        if let Some(r) = row {
            result.insert(timeline.clone(), MarkerInfo {
                last_read_id: r.last_read_id,
                version: r.version,
                updated_at: r.updated_at.to_rfc3339(),
            });
        }
    }

    Ok(Json(result))
}

// ── POST /api/v1/markers ──────────────────────────────────────────────────

pub async fn set_markers(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    body: Bytes,
) -> AppResult<Json<HashMap<String, MarkerInfo>>> {
    auth.require_scope("write:statuses")?;
    let body_str = std::str::from_utf8(&body).unwrap_or("");

    // Parse bracket-notation form: home[last_read_id]=..., notifications[last_read_id]=...
    let mut home_id: Option<String> = None;
    let mut notif_id: Option<String> = None;

    for pair in body_str.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            let key = urlencoding::decode(k).unwrap_or_default();
            let val = urlencoding::decode(v).unwrap_or_default();
            match key.as_ref() {
                "home[last_read_id]" => home_id = Some(val.into_owned()),
                "notifications[last_read_id]" => notif_id = Some(val.into_owned()),
                _ => {}
            }
        }
    }

    let mut result = HashMap::new();

    for (timeline, last_read_id) in [("home", home_id), ("notifications", notif_id)] {
        let Some(id) = last_read_id else { continue };

        sqlx::query!(
            r#"INSERT INTO markers (account_id, timeline, last_read_id, version, updated_at)
               VALUES ($1, $2, $3, 1, now())
               ON CONFLICT (account_id, timeline) DO UPDATE
                 SET last_read_id = EXCLUDED.last_read_id,
                     version = markers.version + 1,
                     updated_at = now()"#,
            auth.account_id, timeline, id
        )
        .execute(&state.db)
        .await?;

        let row = sqlx::query!(
            "SELECT last_read_id, version, updated_at FROM markers WHERE account_id = $1 AND timeline = $2",
            auth.account_id, timeline
        )
        .fetch_one(&state.db)
        .await?;

        result.insert(timeline.to_string(), MarkerInfo {
            last_read_id: row.last_read_id,
            version: row.version,
            updated_at: row.updated_at.to_rfc3339(),
        });
    }

    Ok(Json(result))
}
