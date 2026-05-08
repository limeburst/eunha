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
    let user_count: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM accounts WHERE instance_id = $1 AND domain IS NULL",
        instance.id
    )
    .fetch_one(&state.db)
    .await?
    .unwrap_or(0);

    let status_count: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM statuses WHERE instance_id = $1 AND deleted_at IS NULL",
        instance.id
    )
    .fetch_one(&state.db)
    .await?
    .unwrap_or(0);

    Ok(Json(json!({
        "version": "2.0",
        "software": {
            "name": "eunha",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "protocols": ["activitypub"],
        "usage": {
            "users": {
                "total": user_count,
                "activeHalfyear": 0,
                "activeMonth": 0,
            },
            "localPosts": status_count,
        },
        "openRegistrations": instance.registrations_open,
        "metadata": {
            "nodeName": instance.title,
            "nodeDescription": instance.description,
        }
    })))
}
