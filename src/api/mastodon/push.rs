use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::{AppError, AppResult},
    middleware::AuthenticatedUser,
    push::ensure_vapid_keys,
    state::AppState,
};

// ── Subscription response type ─────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PushSubscription {
    pub id: String,
    pub endpoint: String,
    pub alerts: PushAlerts,
    pub policy: String,
    pub server_key: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct PushAlerts {
    pub follow: bool,
    pub favourite: bool,
    pub reblog: bool,
    pub mention: bool,
    pub poll: bool,
    pub status: bool,
}

// ── POST /api/v1/push/subscription ────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateSubscriptionBody {
    pub subscription: SubscriptionInput,
    #[serde(default)]
    pub data: SubscriptionData,
}

#[derive(Debug, Deserialize)]
pub struct SubscriptionInput {
    pub endpoint: String,
    pub keys: SubscriptionKeys,
}

#[derive(Debug, Deserialize)]
pub struct SubscriptionKeys {
    pub p256dh: String,
    pub auth: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct SubscriptionData {
    #[serde(default)]
    pub alerts: AlertsInput,
    #[serde(default)]
    pub policy: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct AlertsInput {
    #[serde(default)]
    pub follow: Option<bool>,
    #[serde(default)]
    pub favourite: Option<bool>,
    #[serde(default)]
    pub reblog: Option<bool>,
    #[serde(default)]
    pub mention: Option<bool>,
    #[serde(default)]
    pub poll: Option<bool>,
    #[serde(default)]
    pub status: Option<bool>,
}

pub async fn create_subscription(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Extension(crate::middleware::ResolvedInstance(instance)): Extension<crate::middleware::ResolvedInstance>,
    Json(body): Json<CreateSubscriptionBody>,
) -> AppResult<Json<PushSubscription>> {
    ensure_vapid_keys(&state, instance.id)
        .await
        .map_err(|e| AppError::Internal(e))?;

    let instance = sqlx::query_as!(
        crate::db::models::Instance,
        "SELECT * FROM instances WHERE id = $1",
        instance.id,
    )
    .fetch_one(&state.db)
    .await?;

    let alerts = &body.data.alerts;
    let policy = body.data.policy.as_deref().unwrap_or("all");

    let row = sqlx::query!(
        r#"INSERT INTO web_push_subscriptions
             (account_id, access_token_id, endpoint, p256dh, auth,
              alert_follow, alert_favourite, alert_reblog, alert_mention, alert_poll, alert_status, policy)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
           ON CONFLICT (access_token_id) DO UPDATE SET
             endpoint        = EXCLUDED.endpoint,
             p256dh          = EXCLUDED.p256dh,
             auth            = EXCLUDED.auth,
             alert_follow    = EXCLUDED.alert_follow,
             alert_favourite = EXCLUDED.alert_favourite,
             alert_reblog    = EXCLUDED.alert_reblog,
             alert_mention   = EXCLUDED.alert_mention,
             alert_poll      = EXCLUDED.alert_poll,
             alert_status    = EXCLUDED.alert_status,
             policy          = EXCLUDED.policy,
             updated_at      = now()
           RETURNING id, alert_follow, alert_favourite, alert_reblog, alert_mention, alert_poll, alert_status, policy"#,
        auth.account_id,
        auth.token_id,
        body.subscription.endpoint,
        body.subscription.keys.p256dh,
        body.subscription.keys.auth,
        alerts.follow.unwrap_or(true),
        alerts.favourite.unwrap_or(true),
        alerts.reblog.unwrap_or(true),
        alerts.mention.unwrap_or(true),
        alerts.poll.unwrap_or(false),
        alerts.status.unwrap_or(false),
        policy,
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(PushSubscription {
        id: row.id.to_string(),
        endpoint: body.subscription.endpoint,
        alerts: PushAlerts {
            follow: row.alert_follow,
            favourite: row.alert_favourite,
            reblog: row.alert_reblog,
            mention: row.alert_mention,
            poll: row.alert_poll,
            status: row.alert_status,
        },
        policy: row.policy,
        server_key: instance.vapid_public_key,
    }))
}

// ── GET /api/v1/push/subscription ─────────────────────────────────────────

pub async fn get_subscription(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Extension(crate::middleware::ResolvedInstance(instance)): Extension<crate::middleware::ResolvedInstance>,
) -> AppResult<Json<PushSubscription>> {
    let instance = sqlx::query_as!(
        crate::db::models::Instance,
        "SELECT * FROM instances WHERE id = $1",
        instance.id,
    )
    .fetch_one(&state.db)
    .await?;

    let row = sqlx::query!(
        r#"SELECT id, endpoint, alert_follow, alert_favourite, alert_reblog,
                  alert_mention, alert_poll, alert_status, policy
           FROM web_push_subscriptions
           WHERE access_token_id = $1"#,
        auth.token_id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(Json(PushSubscription {
        id: row.id.to_string(),
        endpoint: row.endpoint,
        alerts: PushAlerts {
            follow: row.alert_follow,
            favourite: row.alert_favourite,
            reblog: row.alert_reblog,
            mention: row.alert_mention,
            poll: row.alert_poll,
            status: row.alert_status,
        },
        policy: row.policy,
        server_key: instance.vapid_public_key,
    }))
}

// ── PUT /api/v1/push/subscription ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UpdateSubscriptionBody {
    #[serde(default)]
    pub data: SubscriptionData,
}

pub async fn update_subscription(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Extension(crate::middleware::ResolvedInstance(instance)): Extension<crate::middleware::ResolvedInstance>,
    Json(body): Json<UpdateSubscriptionBody>,
) -> AppResult<Json<PushSubscription>> {
    let instance = sqlx::query_as!(
        crate::db::models::Instance,
        "SELECT * FROM instances WHERE id = $1",
        instance.id,
    )
    .fetch_one(&state.db)
    .await?;

    let alerts = &body.data.alerts;
    let policy = body.data.policy.as_deref().unwrap_or("all");

    let row = sqlx::query!(
        r#"UPDATE web_push_subscriptions SET
             alert_follow    = COALESCE($1, alert_follow),
             alert_favourite = COALESCE($2, alert_favourite),
             alert_reblog    = COALESCE($3, alert_reblog),
             alert_mention   = COALESCE($4, alert_mention),
             alert_poll      = COALESCE($5, alert_poll),
             alert_status    = COALESCE($6, alert_status),
             policy          = $7,
             updated_at      = now()
           WHERE access_token_id = $8
           RETURNING id, endpoint, alert_follow, alert_favourite, alert_reblog,
                     alert_mention, alert_poll, alert_status, policy"#,
        alerts.follow,
        alerts.favourite,
        alerts.reblog,
        alerts.mention,
        alerts.poll,
        alerts.status,
        policy,
        auth.token_id,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(Json(PushSubscription {
        id: row.id.to_string(),
        endpoint: row.endpoint,
        alerts: PushAlerts {
            follow: row.alert_follow,
            favourite: row.alert_favourite,
            reblog: row.alert_reblog,
            mention: row.alert_mention,
            poll: row.alert_poll,
            status: row.alert_status,
        },
        policy: row.policy,
        server_key: instance.vapid_public_key,
    }))
}

// ── DELETE /api/v1/push/subscription ──────────────────────────────────────

pub async fn delete_subscription(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let _ = sqlx::query!(
        "DELETE FROM web_push_subscriptions WHERE access_token_id = $1",
        auth.token_id,
    )
    .execute(&state.db)
    .await;

    StatusCode::OK
}
