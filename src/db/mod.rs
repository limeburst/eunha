pub mod models;

use sqlx::PgPool;
use uuid::Uuid;
use crate::error::{AppError, AppResult};
use self::models::Instance;

pub async fn get_instance_by_domain(db: &PgPool, domain: &str) -> AppResult<Instance> {
    sqlx::query_as!(
        Instance,
        "SELECT * FROM instances WHERE domain = $1 OR custom_domain = $1",
        domain
    )
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)
}

pub async fn get_instance_by_id(db: &PgPool, id: Uuid) -> AppResult<Instance> {
    sqlx::query_as!(
        Instance,
        "SELECT * FROM instances WHERE id = $1",
        id
    )
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)
}
