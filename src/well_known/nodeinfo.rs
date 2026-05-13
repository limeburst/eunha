use axum::{
    extract::{Extension, State},
    Json,
};
use serde_json::{json, Value};

use crate::{
    error::AppResult,
    middleware::ResolvedInstance,
    state::AppState,
};

pub async fn nodeinfo_links(
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
) -> AppResult<Json<Value>> {
    Ok(Json(json!({
        "links": [{
            "rel": "http://nodeinfo.diaspora.software/ns/schema/2.0",
            "href": format!("https://{}/nodeinfo/2.0", instance.domain),
        }]
    })))
}

pub async fn nodeinfo(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
) -> AppResult<Json<Value>> {
    let (user_count, active_month, active_halfyear, status_count) = tokio::try_join!(
        sqlx::query_scalar!(
            "SELECT COUNT(*) FROM accounts WHERE instance_id = $1 AND domain IS NULL AND suspended_at IS NULL",
            instance.id
        ).fetch_one(&state.db),
        sqlx::query_scalar!(
            r#"SELECT COUNT(DISTINCT s.account_id) FROM statuses s
               JOIN accounts a ON a.id = s.account_id
               WHERE a.instance_id = $1 AND a.domain IS NULL AND s.deleted_at IS NULL
                 AND s.created_at > now() - interval '30 days'"#,
            instance.id
        ).fetch_one(&state.db),
        sqlx::query_scalar!(
            r#"SELECT COUNT(DISTINCT s.account_id) FROM statuses s
               JOIN accounts a ON a.id = s.account_id
               WHERE a.instance_id = $1 AND a.domain IS NULL AND s.deleted_at IS NULL
                 AND s.created_at > now() - interval '180 days'"#,
            instance.id
        ).fetch_one(&state.db),
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) FROM statuses s JOIN accounts a ON a.id = s.account_id
               WHERE a.instance_id = $1 AND a.domain IS NULL AND s.deleted_at IS NULL"#,
            instance.id
        ).fetch_one(&state.db),
    )?;

    Ok(Json(json!({
        "version": "2.0",
        "software": {
            "name": "eunha",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "protocols": ["activitypub"],
        "usage": {
            "users": {
                "total": user_count.unwrap_or(0),
                "activeMonth": active_month.unwrap_or(0),
                "activeHalfyear": active_halfyear.unwrap_or(0),
            },
            "localPosts": status_count.unwrap_or(0),
        },
        "openRegistrations": instance.registrations_open,
        "metadata": {
            "nodeName": instance.title,
            "nodeDescription": instance.description,
        }
    })))
}
