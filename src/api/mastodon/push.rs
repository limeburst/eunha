use axum::{
    extract::{Extension, State},
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
    pub standard: bool,
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

fn alerts_from_data(data: &serde_json::Value) -> PushAlerts {
    let a = &data["alerts"];
    PushAlerts {
        follow:    a["follow"]   .as_bool().unwrap_or(true),
        favourite: a["favourite"].as_bool().unwrap_or(true),
        reblog:    a["reblog"]   .as_bool().unwrap_or(true),
        mention:   a["mention"]  .as_bool().unwrap_or(true),
        poll:      a["poll"]     .as_bool().unwrap_or(false),
        status:    a["status"]   .as_bool().unwrap_or(false),
    }
}

fn policy_from_data(data: &serde_json::Value) -> String {
    data["policy"].as_str().unwrap_or("all").to_string()
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
    auth.require_scope("push")?;
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
    let data = serde_json::json!({
        "alerts": {
            "follow":    alerts.follow.unwrap_or(true),
            "favourite": alerts.favourite.unwrap_or(true),
            "reblog":    alerts.reblog.unwrap_or(true),
            "mention":   alerts.mention.unwrap_or(true),
            "poll":      alerts.poll.unwrap_or(false),
            "status":    alerts.status.unwrap_or(false),
        },
        "policy": policy,
    });

    let row = sqlx::query!(
        r#"INSERT INTO web_push_subscriptions
             (account_id, access_token_id, endpoint, key_p256dh, key_auth, data)
           VALUES ($1, $2, $3, $4, $5, $6)
           ON CONFLICT (access_token_id) DO UPDATE SET
             endpoint   = EXCLUDED.endpoint,
             key_p256dh = EXCLUDED.key_p256dh,
             key_auth   = EXCLUDED.key_auth,
             data       = EXCLUDED.data,
             updated_at = now()
           RETURNING id, data as "data: serde_json::Value""#,
        auth.account_id,
        auth.token_id,
        body.subscription.endpoint,
        body.subscription.keys.p256dh,
        body.subscription.keys.auth,
        data,
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(PushSubscription {
        id: row.id.to_string(),
        endpoint: body.subscription.endpoint,
        standard: false,
        alerts: alerts_from_data(&row.data),
        policy: policy_from_data(&row.data),
        server_key: instance.vapid_public_key,
    }))
}

// ── GET /api/v1/push/subscription ─────────────────────────────────────────

pub async fn get_subscription(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Extension(crate::middleware::ResolvedInstance(instance)): Extension<crate::middleware::ResolvedInstance>,
) -> AppResult<Json<PushSubscription>> {
    auth.require_scope("push")?;
    let instance = sqlx::query_as!(
        crate::db::models::Instance,
        "SELECT * FROM instances WHERE id = $1",
        instance.id,
    )
    .fetch_one(&state.db)
    .await?;

    let row = sqlx::query!(
        r#"SELECT id, endpoint, data as "data: serde_json::Value"
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
        standard: false,
        alerts: alerts_from_data(&row.data),
        policy: policy_from_data(&row.data),
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
    auth.require_scope("push")?;
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
             data = jsonb_build_object(
               'alerts', jsonb_build_object(
                 'follow',    COALESCE($1::boolean, (data->'alerts'->>'follow')::boolean, true),
                 'favourite', COALESCE($2::boolean, (data->'alerts'->>'favourite')::boolean, true),
                 'reblog',    COALESCE($3::boolean, (data->'alerts'->>'reblog')::boolean, true),
                 'mention',   COALESCE($4::boolean, (data->'alerts'->>'mention')::boolean, true),
                 'poll',      COALESCE($5::boolean, (data->'alerts'->>'poll')::boolean, false),
                 'status',    COALESCE($6::boolean, (data->'alerts'->>'status')::boolean, false)
               ),
               'policy', $7::text
             ),
             updated_at = now()
           WHERE access_token_id = $8
           RETURNING id, endpoint, data as "data: serde_json::Value""#,
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
        standard: false,
        alerts: alerts_from_data(&row.data),
        policy: policy_from_data(&row.data),
        server_key: instance.vapid_public_key,
    }))
}

// ── DELETE /api/v1/push/subscription ──────────────────────────────────────

pub async fn delete_subscription(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> AppResult<Json<serde_json::Value>> {
    auth.require_scope("push")?;
    sqlx::query!(
        "DELETE FROM web_push_subscriptions WHERE access_token_id = $1",
        auth.token_id,
    )
    .execute(&state.db)
    .await?;

    Ok(Json(serde_json::json!({})))
}
