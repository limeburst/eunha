use redis::{aio::ConnectionManager, AsyncCommands};
use sqlx::PgPool;
use uuid::Uuid;

const FEED_MAX_ITEMS: isize = 800;
const FEED_TTL_SECS: u64 = 7 * 24 * 3600; // 1 week

fn feed_key(instance_id: Uuid, account_id: i64) -> String {
    format!("{}:feed:home:{}", instance_id, account_id)
}

// Marks that a feed has been fully populated from DB.
// Distinguishes "populated but empty" from "never populated".
fn populated_key(instance_id: Uuid, account_id: i64) -> String {
    format!("{}:feed:home:{}:populated", instance_id, account_id)
}

pub async fn is_feed_populated(
    redis: &mut ConnectionManager,
    instance_id: Uuid,
    account_id: i64,
) -> bool {
    redis
        .exists::<_, bool>(populated_key(instance_id, account_id))
        .await
        .unwrap_or(false)
}

pub async fn feed_push(
    redis: &mut ConnectionManager,
    instance_id: Uuid,
    account_id: i64,
    status_id: i64,
) {
    let key = feed_key(instance_id, account_id);
    let result: redis::RedisResult<()> = redis::pipe()
        .zadd(&key, status_id as f64, status_id)
        .zremrangebyrank(&key, 0, -(FEED_MAX_ITEMS + 1))
        .ignore()
        .query_async(redis)
        .await;
    if let Err(e) = result {
        tracing::warn!("feed_push error for account {}: {}", account_id, e);
    }
}

pub async fn feed_remove(
    redis: &mut ConnectionManager,
    instance_id: Uuid,
    account_id: i64,
    status_id: i64,
) {
    let result: redis::RedisResult<()> = redis
        .zrem(feed_key(instance_id, account_id), status_id)
        .await;
    if let Err(e) = result {
        tracing::warn!("feed_remove error for account {}: {}", account_id, e);
    }
}

/// Fetch status IDs from the Redis feed, honouring Mastodon-style pagination.
/// Returns None if the feed has never been populated (cold start signal).
pub async fn feed_get(
    redis: &mut ConnectionManager,
    instance_id: Uuid,
    account_id: i64,
    max_id: Option<i64>,
    since_id: Option<i64>,
    min_id: Option<i64>,
    limit: isize,
) -> Option<Vec<i64>> {
    if !is_feed_populated(redis, instance_id, account_id).await {
        return None;
    }

    let key = feed_key(instance_id, account_id);

    let ids: Vec<i64> = if min_id.is_some() {
        // ASC: oldest items strictly after min_id
        let min_score = format!("({}", min_id.unwrap());
        redis::cmd("ZRANGEBYSCORE")
            .arg(&key)
            .arg(&min_score)
            .arg("+inf")
            .arg("LIMIT")
            .arg(0i64)
            .arg(limit)
            .query_async(redis)
            .await
            .unwrap_or_default()
    } else {
        // DESC: newest items strictly before max_id, floored at since_id
        let max_score = max_id
            .map(|id| format!("({}", id))
            .unwrap_or_else(|| "+inf".to_string());
        let min_score = since_id
            .map(|id| format!("({}", id))
            .unwrap_or_else(|| "-inf".to_string());
        redis::cmd("ZREVRANGEBYSCORE")
            .arg(&key)
            .arg(&max_score)
            .arg(&min_score)
            .arg("LIMIT")
            .arg(0i64)
            .arg(limit)
            .query_async(redis)
            .await
            .unwrap_or_default()
    };

    Some(ids)
}

/// Populate the Redis feed from DB (called on first timeline load).
pub async fn feed_populate(
    redis: &mut ConnectionManager,
    instance_id: Uuid,
    account_id: i64,
    db: &PgPool,
) {
    let status_ids: Vec<i64> = sqlx::query_scalar!(
        r#"WITH candidate_ids AS (
               SELECT s.id FROM statuses s
               WHERE s.account_id IN (
                   SELECT target_account_id FROM follows
                   WHERE account_id = $1 AND state = 'accepted'
                   UNION ALL SELECT $1
               )
               AND s.deleted_at IS NULL
               UNION
               SELECT st.status_id FROM status_tags st
               JOIN tag_follows tf ON tf.tag_id = st.tag_id
               JOIN statuses s ON s.id = st.status_id
               WHERE tf.account_id = $1
               AND s.visibility = 'public'
               AND s.deleted_at IS NULL
           )
           SELECT id FROM candidate_ids ORDER BY id DESC LIMIT $2"#,
        account_id,
        FEED_MAX_ITEMS as i64,
    )
    .fetch_all(db)
    .await
    .unwrap_or_default()
    .into_iter()
    .flatten()
    .collect();

    if !status_ids.is_empty() {
        let key = feed_key(instance_id, account_id);
        let mut pipe = redis::pipe();
        for &id in &status_ids {
            pipe.zadd(&key, id as f64, id);
        }
        pipe.expire(&key, FEED_TTL_SECS as i64);
        let _: redis::RedisResult<()> = pipe.query_async(redis).await;
    }

    let _: redis::RedisResult<()> = redis
        .set_ex(populated_key(instance_id, account_id), 1i64, FEED_TTL_SECS)
        .await;
}

/// Fan-out a newly posted status to all followers' initialized feeds,
/// plus accounts following any of the status's hashtags.
pub async fn fanout_new_status(
    redis: &mut ConnectionManager,
    db: &PgPool,
    instance_id: Uuid,
    author_id: i64,
    status_id: i64,
    tag_ids: &[Uuid],
) {
    let follower_ids: Vec<i64> = sqlx::query_scalar!(
        "SELECT account_id FROM follows WHERE target_account_id = $1 AND state = 'accepted'",
        author_id,
    )
    .fetch_all(db)
    .await
    .unwrap_or_default();

    let hashtag_recipients: Vec<i64> = if !tag_ids.is_empty() {
        sqlx::query_scalar!(
            r#"SELECT DISTINCT tf.account_id FROM tag_follows tf
               WHERE tf.tag_id = ANY($1::uuid[])
               AND tf.account_id != $2
               AND NOT EXISTS (
                   SELECT 1 FROM follows
                   WHERE account_id = tf.account_id
                   AND target_account_id = $2
                   AND state = 'accepted'
               )"#,
            tag_ids as &[Uuid],
            author_id,
        )
        .fetch_all(db)
        .await
        .unwrap_or_default()
    } else {
        vec![]
    };

    let recipients: Vec<i64> = std::iter::once(author_id)
        .chain(follower_ids)
        .chain(hashtag_recipients)
        .collect();

    if recipients.is_empty() {
        return;
    }

    // Batch-check which recipients have an initialized feed
    let pop_keys: Vec<String> = recipients
        .iter()
        .map(|&id| populated_key(instance_id, id))
        .collect();
    let initialized: Vec<Option<i64>> = match redis.mget(&pop_keys).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("fanout mget error: {}", e);
            return;
        }
    };

    let score = status_id as f64;
    let mut pipe = redis::pipe();
    let mut any = false;
    for (&id, init) in recipients.iter().zip(initialized.iter()) {
        if init.is_some() {
            let key = feed_key(instance_id, id);
            pipe.zadd(&key, score, status_id);
            pipe.zremrangebyrank(&key, 0, -(FEED_MAX_ITEMS + 1));
            any = true;
        }
    }
    if any {
        let _: redis::RedisResult<()> = pipe.query_async(redis).await;
    }
}

/// Remove a deleted status from all followers' initialized feeds.
pub async fn fanout_remove_status(
    redis: &mut ConnectionManager,
    db: &PgPool,
    instance_id: Uuid,
    author_id: i64,
    status_id: i64,
) {
    let follower_ids: Vec<i64> = sqlx::query_scalar!(
        "SELECT account_id FROM follows WHERE target_account_id = $1 AND state = 'accepted'",
        author_id,
    )
    .fetch_all(db)
    .await
    .unwrap_or_default();

    let recipients: Vec<i64> = std::iter::once(author_id).chain(follower_ids).collect();
    let pop_keys: Vec<String> = recipients
        .iter()
        .map(|&id| populated_key(instance_id, id))
        .collect();
    let initialized: Vec<Option<i64>> = match redis.mget(&pop_keys).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("fanout_remove mget error: {}", e);
            return;
        }
    };

    let mut pipe = redis::pipe();
    let mut any = false;
    for (&id, init) in recipients.iter().zip(initialized.iter()) {
        if init.is_some() {
            pipe.zrem(feed_key(instance_id, id), status_id);
            any = true;
        }
    }
    if any {
        let _: redis::RedisResult<()> = pipe.query_async(redis).await;
    }
}

/// Backfill the follower's feed with recent statuses from the newly-followed account.
pub async fn backfill_follow(
    redis: &mut ConnectionManager,
    db: &PgPool,
    instance_id: Uuid,
    follower_id: i64,
    followed_id: i64,
) {
    if !is_feed_populated(redis, instance_id, follower_id).await {
        return;
    }

    let recent: Vec<i64> = sqlx::query_scalar!(
        "SELECT id FROM statuses WHERE account_id = $1 AND deleted_at IS NULL ORDER BY id DESC LIMIT 20",
        followed_id,
    )
    .fetch_all(db)
    .await
    .unwrap_or_default();

    if recent.is_empty() {
        return;
    }

    let key = feed_key(instance_id, follower_id);
    let mut pipe = redis::pipe();
    for &id in &recent {
        pipe.zadd(&key, id as f64, id);
    }
    pipe.zremrangebyrank(&key, 0, -(FEED_MAX_ITEMS + 1));
    let _: redis::RedisResult<()> = pipe.query_async(redis).await;
}
