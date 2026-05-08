/// Migrates data from a Mastodon pg_dump (custom format) into eunha's schema.
///
/// Usage:
///   1. Restore the Mastodon dump into a temp database:
///        pg_restore -d mastodon_src pg_dump.custom
///   2. Run this tool:
///        eunha-migrate-mastodon \
///          --mastodon-db postgres://user@localhost/mastodon_src \
///          --eunha-db postgres://eunha:eunha@localhost/eunha \
///          --domain seoul.earth
use anyhow::{Context, Result};
use clap::Parser;
use sqlx::{PgPool, PgConnection, Row};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(about = "Migrate a Mastodon database into eunha")]
struct Args {
    #[arg(long)]
    mastodon_db: String,
    #[arg(long)]
    eunha_db: String,
    #[arg(long)]
    domain: String,
    #[arg(long)]
    limit_accounts: Option<i64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let src = PgPool::connect(&args.mastodon_db).await.context("connecting to Mastodon DB")?;
    let dst = PgPool::connect(&args.eunha_db).await.context("connecting to eunha DB")?;

    // Schema migrations run outside the transaction — they manage their own state.
    sqlx::migrate!("./migrations").run(&dst).await?;

    let mut tx = dst.begin().await.context("beginning transaction")?;

    tracing::info!("migrating instance: {}", args.domain);
    let instance_id = migrate_instance(&src, &mut *tx, &args.domain).await?;
    tracing::info!("instance_id = {}", instance_id);

    tracing::info!("migrating accounts...");
    let account_map = migrate_accounts(&src, &mut *tx, instance_id, args.limit_accounts).await?;
    tracing::info!("migrated {} accounts", account_map.len());

    tracing::info!("migrating users...");
    migrate_users(&src, &mut *tx, instance_id, &account_map).await?;

    tracing::info!("migrating statuses...");
    let status_map = migrate_statuses(&src, &mut *tx, instance_id, &account_map).await?;
    tracing::info!("migrated {} statuses", status_map.len());

    tracing::info!("migrating follows...");
    migrate_follows(&src, &mut *tx, &account_map).await?;

    tracing::info!("migrating favourites...");
    migrate_favourites(&src, &mut *tx, &account_map, &status_map).await?;

    tracing::info!("migrating media attachments...");
    migrate_media(&src, &mut *tx, instance_id, &account_map, &status_map).await?;

    tracing::info!("migrating blocks...");
    migrate_blocks(&src, &mut *tx, &account_map).await?;

    tracing::info!("migrating mutes...");
    migrate_mutes(&src, &mut *tx, &account_map).await?;

    tracing::info!("migrating bookmarks...");
    migrate_bookmarks(&src, &mut *tx, &account_map, &status_map).await?;

    tracing::info!("migrating custom emojis...");
    migrate_custom_emojis(&src, &mut *tx, instance_id).await?;

    tracing::info!("migrating status edits...");
    migrate_status_edits(&src, &mut *tx, &account_map, &status_map).await?;

    tracing::info!("migrating polls...");
    let poll_map = migrate_polls(&src, &mut *tx, &account_map, &status_map).await?;
    tracing::info!("migrated {} polls", poll_map.len());

    tracing::info!("migrating poll votes...");
    migrate_poll_votes(&src, &mut *tx, &account_map, &poll_map).await?;

    tracing::info!("migrating tags...");
    migrate_tags(&src, &mut *tx, &status_map).await?;

    tracing::info!("migrating mentions...");
    migrate_mentions(&src, &mut *tx, &account_map, &status_map).await?;

    tracing::info!("migrating notifications...");
    migrate_notifications(&src, &mut *tx, &account_map, &status_map).await?;

    tx.commit().await.context("committing transaction")?;
    tracing::info!("migration complete");
    Ok(())
}

async fn migrate_instance(src: &PgPool, dst: &mut PgConnection, domain: &str) -> Result<Uuid> {
    let settings_rows = sqlx::query(
        "SELECT var, value FROM settings WHERE thing_type IS NULL LIMIT 100"
    )
    .fetch_all(src)
    .await
    .unwrap_or_default();

    let mut title = domain.to_string();
    let mut description = String::new();
    let mut short_description = String::new();
    let mut contact_email: Option<String> = None;

    for row in &settings_rows {
        let key: Option<String> = row.try_get("var").ok();
        let val: Option<String> = row.try_get("value").ok();
        match (key.as_deref(), val) {
            (Some("site_title"), Some(v)) => title = strip_yaml(&v),
            (Some("site_short_description"), Some(v)) => short_description = strip_yaml(&v),
            (Some("site_description"), Some(v)) => description = strip_yaml(&v),
            (Some("site_contact_email"), Some(v)) => contact_email = Some(strip_yaml(&v)),
            _ => {}
        }
    }

    let keypair = sqlx::query("SELECT private_key, public_key FROM server_keypairs LIMIT 1")
        .fetch_optional(src)
        .await
        .ok()
        .flatten();

    let private_key: String = keypair.as_ref().and_then(|r| r.try_get("private_key").ok()).unwrap_or_default();
    let public_key: String = keypair.as_ref().and_then(|r| r.try_get("public_key").ok()).unwrap_or_default();

    let id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO instances
             (domain, title, description, short_description, contact_email,
              registrations_open, private_key, public_key)
           VALUES ($1,$2,$3,$4,$5,true,$6,$7)
           ON CONFLICT (domain) DO UPDATE
             SET title = EXCLUDED.title,
                 description = EXCLUDED.description,
                 short_description = EXCLUDED.short_description,
                 contact_email = EXCLUDED.contact_email,
                 private_key = EXCLUDED.private_key,
                 public_key = EXCLUDED.public_key,
                 updated_at = now()
           RETURNING id"#,
    )
    .bind(domain)
    .bind(&title)
    .bind(&description)
    .bind(&short_description)
    .bind(contact_email)
    .bind(&private_key)
    .bind(&public_key)
    .fetch_one(&mut *dst)
    .await?;

    Ok(id)
}

async fn migrate_accounts(
    src: &PgPool,
    dst: &mut PgConnection,
    instance_id: Uuid,
    limit: Option<i64>,
) -> Result<HashMap<i64, Uuid>> {
    let rows = sqlx::query(
        r#"SELECT a.*,
               COALESCE(s.followers_count, 0) AS followers_count,
               COALESCE(s.following_count, 0) AS following_count,
               COALESCE(s.statuses_count,  0) AS statuses_count
           FROM accounts a
           LEFT JOIN account_stats s ON s.account_id = a.id
           ORDER BY a.id LIMIT $1"#,
    )
    .bind(limit.unwrap_or(i64::MAX))
    .fetch_all(src)
    .await?;

    let mut map = HashMap::new();

    for row in &rows {
        let src_id: i64 = row.get("id");
        let domain: Option<String> = row.try_get("domain").ok().flatten();
        let is_local = domain.is_none();

        let eunha_instance_id = if is_local {
            instance_id
        } else {
            let remote_domain = domain.as_deref().unwrap_or("unknown");
            sqlx::query_scalar(
                r#"INSERT INTO instances (domain, title, registrations_open, private_key, public_key)
                   VALUES ($1,$1,false,'','')
                   ON CONFLICT (domain) DO UPDATE SET updated_at = now()
                   RETURNING id"#,
            )
            .bind(remote_domain)
            .fetch_one(&mut *dst)
            .await?
        };

        let username: String = row.try_get("username").unwrap_or_default();
        let display_name: Option<String> = row.try_get("display_name").ok().flatten();
        let note: Option<String> = row.try_get("note").ok().flatten();
        let url: Option<String> = row.try_get("url").ok().flatten();
        let uri: Option<String> = row.try_get("uri").ok().flatten();
        let locked: Option<bool> = row.try_get("locked").ok().flatten();
        // Mastodon ≥3.x uses actor_type enum; older versions have a direct `bot` boolean
        let bot: bool = row.try_get::<bool, _>("bot").ok().unwrap_or_else(|| {
            row.try_get::<String, _>("actor_type")
                .ok()
                .map(|t| t == "Service" || t == "Application")
                .unwrap_or(false)
        });
        let discoverable: Option<bool> = row.try_get("discoverable").ok().flatten();
        let private_key: Option<String> = row.try_get("private_key").ok().flatten();
        let public_key: Option<String> = row.try_get("public_key").ok().flatten();
        let followers_count: Option<i64> = row.try_get("followers_count").ok().flatten();
        let following_count: Option<i64> = row.try_get("following_count").ok().flatten();
        let statuses_count: Option<i64> = row.try_get("statuses_count").ok().flatten();
        let inbox_url: Option<String> = row.try_get("inbox_url").ok().flatten();
        let outbox_url: Option<String> = row.try_get("outbox_url").ok().flatten();
        let shared_inbox_url: Option<String> = row.try_get("shared_inbox_url").ok().flatten();
        let suspended_at = get_ts_opt(&row, "suspended_at");
        let silenced_at = get_ts_opt(&row, "silenced_at");
        let created_at = get_ts(&row, "created_at")?;
        let updated_at = get_ts(&row, "updated_at")?;

        let new_id: Option<Uuid> = sqlx::query_scalar(
            r#"INSERT INTO accounts
                 (instance_id, username, domain, display_name, note,
                  url, uri, locked, bot, discoverable,
                  private_key, public_key,
                  followers_count, following_count, statuses_count,
                  inbox_url, outbox_url, shared_inbox_url,
                  suspended_at, silenced_at,
                  created_at, updated_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22)
               ON CONFLICT DO NOTHING
               RETURNING id"#,
        )
        .bind(eunha_instance_id)
        .bind(&username)
        .bind(&domain)
        .bind(display_name.as_deref().unwrap_or(""))
        .bind(note.as_deref().unwrap_or(""))
        .bind(url.as_deref().unwrap_or(""))
        .bind(uri.as_deref().unwrap_or(""))
        .bind(locked.unwrap_or(false))
        .bind(bot)
        .bind(discoverable.unwrap_or(true))
        .bind(&private_key)
        .bind(public_key.as_deref().unwrap_or(""))
        .bind(followers_count.unwrap_or(0))
        .bind(following_count.unwrap_or(0))
        .bind(statuses_count.unwrap_or(0))
        .bind(inbox_url.as_deref().unwrap_or(""))
        .bind(outbox_url.as_deref().unwrap_or(""))
        .bind(&shared_inbox_url)
        .bind(suspended_at)
        .bind(silenced_at)
        .bind(created_at)
        .bind(updated_at)
        .fetch_optional(&mut *dst)
        .await?;

        if let Some(new_id) = new_id {
            map.insert(src_id, new_id);
        }
    }

    Ok(map)
}

async fn migrate_users(
    src: &PgPool,
    dst: &mut PgConnection,
    instance_id: Uuid,
    account_map: &HashMap<i64, Uuid>,
) -> Result<()> {
    let rows = sqlx::query("SELECT * FROM users")
        .fetch_all(src)
        .await?;

    for row in &rows {
        let src_account_id: i64 = row.get("account_id");
        let Some(&account_id) = account_map.get(&src_account_id) else { continue };

        let email: String = row.try_get("email").unwrap_or_default();
        // Mastodon stores bcrypt hashes in encrypted_password
        let password_hash: String = row.try_get("encrypted_password").unwrap_or_default();
        let confirmed_at = get_ts_opt(&row, "confirmed_at");
        let created_at = get_ts(&row, "created_at")?;
        let updated_at = get_ts(&row, "updated_at")?;

        sqlx::query(
            r#"INSERT INTO users
                 (account_id, instance_id, email, email_normalized, password_hash,
                  confirmed_at, created_at, updated_at)
               VALUES ($1,$2,$3,lower($3),$4,$5,$6,$7)
               ON CONFLICT (instance_id, email_normalized) DO NOTHING"#,
        )
        .bind(account_id)
        .bind(instance_id)
        .bind(&email)
        .bind(&password_hash)
        .bind(confirmed_at)
        .bind(created_at)
        .bind(updated_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_statuses(
    src: &PgPool,
    dst: &mut PgConnection,
    instance_id: Uuid,
    account_map: &HashMap<i64, Uuid>,
) -> Result<HashMap<i64, i64>> {
    // Mastodon `visibility` is an integer enum: 0=public 1=unlisted 2=private 3=direct
    let rows = sqlx::query("SELECT * FROM statuses ORDER BY id")
        .fetch_all(src)
        .await?;

    let mut map = HashMap::new();

    for row in &rows {
        let src_id: i64 = row.get("id");
        let src_account_id: i64 = row.get("account_id");
        let Some(&account_id) = account_map.get(&src_account_id) else { continue };

        let text: Option<String> = row.try_get("text").ok().flatten();
        let spoiler_text: Option<String> = row.try_get("spoiler_text").ok().flatten();
        let visibility_int: Option<i32> = row.try_get("visibility").ok().flatten();
        let visibility = match visibility_int.unwrap_or(0) {
            0 => "public", 1 => "unlisted", 2 => "private", 3 => "direct", _ => "public",
        };
        let language: Option<String> = row.try_get("language").ok().flatten();
        let sensitive: Option<bool> = row.try_get("sensitive").ok().flatten();
        let url: Option<String> = row.try_get("url").ok().flatten();
        let uri: Option<String> = row.try_get("uri").ok().flatten();
        let in_reply_to_id_src: Option<i64> = row.try_get("in_reply_to_id").ok().flatten();
        let replies_count: Option<i64> = row.try_get("replies_count").ok().flatten();
        let reblogs_count: Option<i64> = row.try_get("reblogs_count").ok().flatten();
        let favourites_count: Option<i64> = row.try_get("favourites_count").ok().flatten();
        let deleted_at = get_ts_opt(&row, "deleted_at");
        let edited_at = get_ts_opt(&row, "edited_at");
        let created_at = get_ts(&row, "created_at")?;

        // Best-effort in_reply_to remapping using already-processed statuses
        let in_reply_to_id: Option<i64> = in_reply_to_id_src.and_then(|id| map.get(&id)).copied();

        let new_id: Option<i64> = sqlx::query_scalar(
            r#"INSERT INTO statuses
                 (instance_id, account_id, text, content, spoiler_text,
                  visibility, language, sensitive, url, uri,
                  in_reply_to_id,
                  replies_count, reblogs_count, favourites_count,
                  deleted_at, edited_at, created_at)
               VALUES ($1,$2,$3,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)
               ON CONFLICT (uri) DO NOTHING
               RETURNING id"#,
        )
        .bind(instance_id)
        .bind(account_id)
        .bind(text.as_deref().unwrap_or(""))
        .bind(spoiler_text.as_deref().unwrap_or(""))
        .bind(visibility)
        .bind(&language)
        .bind(sensitive.unwrap_or(false))
        .bind(&url)
        .bind(&uri)
        .bind(in_reply_to_id)
        .bind(replies_count.unwrap_or(0))
        .bind(reblogs_count.unwrap_or(0))
        .bind(favourites_count.unwrap_or(0))
        .bind(deleted_at)
        .bind(edited_at)
        .bind(created_at)
        .fetch_optional(&mut *dst)
        .await?;

        if let Some(new_id) = new_id {
            map.insert(src_id, new_id);
        } else {
            // Already exists — look up the existing eunha ID by uri so
            // downstream maps (favourites, media) still resolve correctly.
            if let Some(uri_str) = &uri {
                if let Ok(existing_id) = sqlx::query_scalar::<_, i64>(
                    "SELECT id FROM statuses WHERE uri = $1",
                )
                .bind(uri_str)
                .fetch_one(&mut *dst)
                .await {
                    map.insert(src_id, existing_id);
                }
            }
        }
    }

    Ok(map)
}

async fn migrate_follows(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
) -> Result<()> {
    let rows = sqlx::query(
        "SELECT account_id, target_account_id, uri, created_at FROM follows",
    )
    .fetch_all(src)
    .await?;

    for row in &rows {
        let src_account: i64 = row.get("account_id");
        let src_target: i64 = row.get("target_account_id");
        let (Some(&account_id), Some(&target_id)) = (account_map.get(&src_account), account_map.get(&src_target))
        else { continue };

        let uri: Option<String> = row.try_get("uri").ok().flatten();
        let created_at = get_ts(&row, "created_at")?;

        sqlx::query(
            r#"INSERT INTO follows (account_id, target_account_id, state, uri, created_at)
               VALUES ($1,$2,'accepted',$3,$4)
               ON CONFLICT DO NOTHING"#,
        )
        .bind(account_id)
        .bind(target_id)
        .bind(&uri)
        .bind(created_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_favourites(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
    status_map: &HashMap<i64, i64>,
) -> Result<()> {
    let rows = sqlx::query("SELECT account_id, status_id, created_at FROM favourites")
        .fetch_all(src)
        .await?;

    for row in &rows {
        let src_account: i64 = row.get("account_id");
        let src_status: i64 = row.get("status_id");
        let (Some(&account_id), Some(&status_id)) = (account_map.get(&src_account), status_map.get(&src_status))
        else { continue };

        let created_at = get_ts(&row, "created_at")?;

        sqlx::query(
            r#"INSERT INTO favourites (account_id, status_id, created_at)
               VALUES ($1,$2,$3) ON CONFLICT DO NOTHING"#,
        )
        .bind(account_id)
        .bind(status_id)
        .bind(created_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_media(
    src: &PgPool,
    dst: &mut PgConnection,
    instance_id: Uuid,
    account_map: &HashMap<i64, Uuid>,
    status_map: &HashMap<i64, i64>,
) -> Result<()> {
    // Skip entirely if this instance already has media — avoids silent duplicates
    // since media_attachments has no natural unique key from the source.
    let existing: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM media_attachments WHERE account_id = ANY(SELECT id FROM accounts WHERE instance_id = $1)",
    )
    .bind(instance_id)
    .fetch_one(&mut *dst)
    .await?;

    if existing > 0 {
        tracing::info!("skipping media migration — {} attachments already present", existing);
        return Ok(());
    }

    let rows = sqlx::query("SELECT * FROM media_attachments")
        .fetch_all(src)
        .await?;

    for row in &rows {
        let src_account: i64 = row.try_get("account_id").ok().flatten().unwrap_or(0);
        let Some(&account_id) = account_map.get(&src_account) else { continue };

        let src_status: Option<i64> = row.try_get("status_id").ok().flatten();
        let status_id: Option<i64> = src_status.and_then(|sid| status_map.get(&sid)).copied();

        let media_type_int: Option<i32> = row.try_get("type").ok().flatten();
        let media_type = match media_type_int.unwrap_or(0) {
            0 => "image", 1 => "gifv", 2 => "video", 3 => "audio", _ => "unknown",
        };

        let description: Option<String> = row.try_get("description").ok().flatten();
        let blurhash: Option<String> = row.try_get("blurhash").ok().flatten();
        let remote_url: Option<String> = row.try_get("remote_url").ok().flatten();
        let created_at = get_ts(&row, "created_at")?;

        sqlx::query(
            r#"INSERT INTO media_attachments (account_id, status_id, media_type, remote_url, description, blurhash, created_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7)"#,
        )
        .bind(account_id)
        .bind(status_id)
        .bind(media_type)
        .bind(&remote_url)
        .bind(&description)
        .bind(&blurhash)
        .bind(created_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_blocks(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
) -> Result<()> {
    let rows = sqlx::query("SELECT account_id, target_account_id, created_at FROM blocks")
        .fetch_all(src)
        .await?;

    for row in &rows {
        let src_account: i64 = row.get("account_id");
        let src_target: i64 = row.get("target_account_id");
        let (Some(&account_id), Some(&target_id)) = (account_map.get(&src_account), account_map.get(&src_target))
        else { continue };

        let created_at = get_ts(&row, "created_at")?;

        sqlx::query(
            r#"INSERT INTO blocks (account_id, target_account_id, created_at)
               VALUES ($1,$2,$3) ON CONFLICT DO NOTHING"#,
        )
        .bind(account_id)
        .bind(target_id)
        .bind(created_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_mutes(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
) -> Result<()> {
    let rows = sqlx::query(
        "SELECT account_id, target_account_id, hide_notifications, expires_at, created_at FROM mutes",
    )
    .fetch_all(src)
    .await?;

    for row in &rows {
        let src_account: i64 = row.get("account_id");
        let src_target: i64 = row.get("target_account_id");
        let (Some(&account_id), Some(&target_id)) = (account_map.get(&src_account), account_map.get(&src_target))
        else { continue };

        let hide_notifications: bool = row.try_get("hide_notifications").unwrap_or(true);
        let expires_at = get_ts_opt(&row, "expires_at");
        let created_at = get_ts(&row, "created_at")?;

        sqlx::query(
            r#"INSERT INTO mutes (account_id, target_account_id, hide_notifications, expires_at, created_at)
               VALUES ($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING"#,
        )
        .bind(account_id)
        .bind(target_id)
        .bind(hide_notifications)
        .bind(expires_at)
        .bind(created_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_bookmarks(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
    status_map: &HashMap<i64, i64>,
) -> Result<()> {
    let rows = sqlx::query("SELECT account_id, status_id, created_at FROM bookmarks")
        .fetch_all(src)
        .await?;

    for row in &rows {
        let src_account: i64 = row.get("account_id");
        let src_status: i64 = row.get("status_id");
        let (Some(&account_id), Some(&status_id)) = (account_map.get(&src_account), status_map.get(&src_status))
        else { continue };

        let created_at = get_ts(&row, "created_at")?;

        sqlx::query(
            r#"INSERT INTO bookmarks (account_id, status_id, created_at)
               VALUES ($1,$2,$3) ON CONFLICT DO NOTHING"#,
        )
        .bind(account_id)
        .bind(status_id)
        .bind(created_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_custom_emojis(
    src: &PgPool,
    dst: &mut PgConnection,
    instance_id: Uuid,
) -> Result<()> {
    let rows = sqlx::query(
        "SELECT shortcode, domain, image_remote_url, visible_in_picker, disabled, created_at FROM custom_emojis",
    )
    .fetch_all(src)
    .await?;

    for row in &rows {
        let shortcode: String = row.try_get("shortcode").unwrap_or_default();
        let domain: Option<String> = row.try_get("domain").ok().flatten();
        let image_url: Option<String> = row.try_get("image_remote_url").ok().flatten();
        let visible_in_picker: bool = row.try_get("visible_in_picker").unwrap_or(true);
        let disabled: bool = row.try_get("disabled").unwrap_or(false);
        let created_at = get_ts(&row, "created_at")?;

        // Local emojis stored in ActiveStorage have no image_remote_url — skip them.
        // They would need to be re-uploaded through the admin UI.
        let Some(image_url) = image_url else { continue };

        let emoji_instance_id = if let Some(ref d) = domain {
            sqlx::query_scalar(
                r#"INSERT INTO instances (domain, title, registrations_open, private_key, public_key)
                   VALUES ($1,$1,false,'','')
                   ON CONFLICT (domain) DO UPDATE SET updated_at = now()
                   RETURNING id"#,
            )
            .bind(d)
            .fetch_one(&mut *dst)
            .await?
        } else {
            instance_id
        };

        sqlx::query(
            r#"INSERT INTO custom_emojis
                 (instance_id, shortcode, domain, image_url, visible_in_picker, disabled, created_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7)
               ON CONFLICT (instance_id, shortcode) DO NOTHING"#,
        )
        .bind(emoji_instance_id)
        .bind(&shortcode)
        .bind(&domain)
        .bind(&image_url)
        .bind(visible_in_picker)
        .bind(disabled)
        .bind(created_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_status_edits(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
    status_map: &HashMap<i64, i64>,
) -> Result<()> {
    let rows = sqlx::query(
        "SELECT status_id, account_id, text, spoiler_text, sensitive, created_at FROM status_edits ORDER BY id",
    )
    .fetch_all(src)
    .await?;

    for row in &rows {
        let src_status: i64 = row.get("status_id");
        let Some(&status_id) = status_map.get(&src_status) else { continue };

        let src_account: Option<i64> = row.try_get("account_id").ok().flatten();
        let account_id: Option<Uuid> = src_account.and_then(|id| account_map.get(&id)).copied();

        let text: String = row.try_get("text").unwrap_or_default();
        let spoiler_text: String = row.try_get("spoiler_text").unwrap_or_default();
        let sensitive: Option<bool> = row.try_get("sensitive").ok().flatten();
        let created_at = get_ts(&row, "created_at")?;

        sqlx::query(
            r#"INSERT INTO status_edits (status_id, account_id, text, content, spoiler_text, sensitive, created_at)
               VALUES ($1,$2,$3,$3,$4,$5,$6)"#,
        )
        .bind(status_id)
        .bind(account_id)
        .bind(&text)
        .bind(&spoiler_text)
        .bind(sensitive.unwrap_or(false))
        .bind(created_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_polls(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
    status_map: &HashMap<i64, i64>,
) -> Result<HashMap<i64, Uuid>> {
    let rows = sqlx::query(
        "SELECT id, account_id, status_id, options, cached_tallies, multiple, votes_count, voters_count, expires_at, created_at FROM polls",
    )
    .fetch_all(src)
    .await?;

    let mut map = HashMap::new();

    for row in &rows {
        let src_id: i64 = row.get("id");
        let src_account: i64 = row.get("account_id");
        let src_status: i64 = row.get("status_id");

        let (Some(&account_id), Some(&status_id)) = (account_map.get(&src_account), status_map.get(&src_status))
        else { continue };

        let options: Vec<String> = row.try_get::<Vec<String>, _>("options").unwrap_or_default();
        let tallies: Vec<i64> = row.try_get::<Vec<i64>, _>("cached_tallies").unwrap_or_default();
        let options_json: serde_json::Value = options.iter().enumerate()
            .map(|(i, title)| serde_json::json!({
                "title": title,
                "votes_count": tallies.get(i).copied().unwrap_or(0),
            }))
            .collect::<Vec<_>>()
            .into();

        let multiple: bool = row.try_get("multiple").unwrap_or(false);
        let votes_count: i64 = row.try_get("votes_count").unwrap_or(0);
        let voters_count: Option<i64> = row.try_get("voters_count").ok().flatten();
        let expires_at = get_ts_opt(&row, "expires_at");
        let created_at = get_ts(&row, "created_at")?;

        let new_id: Option<Uuid> = sqlx::query_scalar(
            r#"INSERT INTO polls
                 (account_id, status_id, options, votes_count, voters_count, multiple, expires_at, created_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
               ON CONFLICT (status_id) DO NOTHING
               RETURNING id"#,
        )
        .bind(account_id)
        .bind(status_id)
        .bind(&options_json)
        .bind(votes_count)
        .bind(voters_count)
        .bind(multiple)
        .bind(expires_at)
        .bind(created_at)
        .fetch_optional(&mut *dst)
        .await?;

        if let Some(new_id) = new_id {
            map.insert(src_id, new_id);
        } else {
            // Already exists — recover ID for poll_votes mapping
            if let Ok(existing_id) = sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM polls WHERE status_id = $1",
            )
            .bind(status_id)
            .fetch_one(&mut *dst)
            .await {
                map.insert(src_id, existing_id);
            }
        }
    }

    Ok(map)
}

async fn migrate_poll_votes(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
    poll_map: &HashMap<i64, Uuid>,
) -> Result<()> {
    let rows = sqlx::query("SELECT account_id, poll_id, choice, created_at FROM poll_votes")
        .fetch_all(src)
        .await?;

    for row in &rows {
        let src_account: i64 = row.get("account_id");
        let src_poll: i64 = row.get("poll_id");
        let (Some(&account_id), Some(&poll_id)) = (account_map.get(&src_account), poll_map.get(&src_poll))
        else { continue };

        let choice: i32 = row.try_get("choice").unwrap_or(0);
        let created_at = get_ts(&row, "created_at")?;

        sqlx::query(
            r#"INSERT INTO poll_votes (poll_id, account_id, choice, created_at)
               VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING"#,
        )
        .bind(poll_id)
        .bind(account_id)
        .bind(choice)
        .bind(created_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_tags(
    src: &PgPool,
    dst: &mut PgConnection,
    status_map: &HashMap<i64, i64>,
) -> Result<()> {
    let tag_rows = sqlx::query("SELECT id, name, created_at FROM tags")
        .fetch_all(src)
        .await?;

    let mut tag_id_map: HashMap<i64, Uuid> = HashMap::new();

    for row in &tag_rows {
        let src_id: i64 = row.get("id");
        let name: String = row.try_get("name").unwrap_or_default();
        let created_at = get_ts(&row, "created_at")?;

        let new_id: Uuid = sqlx::query_scalar(
            r#"INSERT INTO tags (name, created_at)
               VALUES (lower($1), $2)
               ON CONFLICT (name) DO UPDATE SET updated_at = now()
               RETURNING id"#,
        )
        .bind(&name)
        .bind(created_at)
        .fetch_one(&mut *dst)
        .await?;

        tag_id_map.insert(src_id, new_id);
    }

    let st_rows = sqlx::query("SELECT status_id, tag_id FROM statuses_tags")
        .fetch_all(src)
        .await?;

    for row in &st_rows {
        let src_status: i64 = row.get("status_id");
        let src_tag: i64 = row.get("tag_id");
        let (Some(&status_id), Some(&tag_id)) = (status_map.get(&src_status), tag_id_map.get(&src_tag))
        else { continue };

        sqlx::query(
            r#"INSERT INTO status_tags (status_id, tag_id) VALUES ($1,$2) ON CONFLICT DO NOTHING"#,
        )
        .bind(status_id)
        .bind(tag_id)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_mentions(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
    status_map: &HashMap<i64, i64>,
) -> Result<()> {
    let rows = sqlx::query("SELECT status_id, account_id, created_at FROM mentions")
        .fetch_all(src)
        .await?;

    for row in &rows {
        let src_status: i64 = row.get("status_id");
        let src_account: i64 = row.get("account_id");
        let (Some(&status_id), Some(&account_id)) = (status_map.get(&src_status), account_map.get(&src_account))
        else { continue };

        sqlx::query(
            r#"INSERT INTO mentions (status_id, account_id) VALUES ($1,$2) ON CONFLICT DO NOTHING"#,
        )
        .bind(status_id)
        .bind(account_id)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_notifications(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
    status_map: &HashMap<i64, i64>,
) -> Result<()> {
    // Resolve the associated status_id per notification type using JOINs on the source DB.
    let rows = sqlx::query(
        r#"SELECT
             n.account_id,
             n.from_account_id,
             n.type,
             n.created_at,
             CASE
               WHEN n.type IN ('reblog', 'update', 'status') THEN n.activity_id
               WHEN n.type = 'mention'   THEN m.status_id
               WHEN n.type = 'favourite' THEN f.status_id
               WHEN n.type = 'poll'      THEN p.status_id
               ELSE NULL
             END AS resolved_status_id
           FROM notifications n
           LEFT JOIN mentions   m ON n.activity_type = 'Mention'   AND m.id = n.activity_id
           LEFT JOIN favourites f ON n.activity_type = 'Favourite' AND f.id = n.activity_id
           LEFT JOIN polls      p ON n.activity_type = 'Poll'      AND p.id = n.activity_id
           WHERE n.type IS NOT NULL"#,
    )
    .fetch_all(src)
    .await?;

    for row in &rows {
        let src_account: i64 = row.get("account_id");
        let src_from: i64 = row.get("from_account_id");
        let (Some(&account_id), Some(&from_account_id)) = (account_map.get(&src_account), account_map.get(&src_from))
        else { continue };

        let notification_type: String = row.try_get("type").unwrap_or_default();
        let src_status: Option<i64> = row.try_get("resolved_status_id").ok().flatten();
        let status_id: Option<i64> = src_status.and_then(|id| status_map.get(&id)).copied();
        let created_at = get_ts(&row, "created_at")?;

        sqlx::query(
            r#"INSERT INTO notifications
                 (account_id, from_account_id, notification_type, status_id, created_at)
               VALUES ($1,$2,$3,$4,$5)"#,
        )
        .bind(account_id)
        .bind(from_account_id)
        .bind(&notification_type)
        .bind(status_id)
        .bind(created_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

/// Reads a `timestamp without time zone` column as UTC.
fn get_ts(row: &sqlx::postgres::PgRow, col: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    row.try_get::<chrono::NaiveDateTime, _>(col)
        .map(|dt| dt.and_utc())
        .with_context(|| format!("reading timestamp column '{col}'"))
}

/// Reads a nullable `timestamp without time zone` column as UTC.
fn get_ts_opt(row: &sqlx::postgres::PgRow, col: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    row.try_get::<Option<chrono::NaiveDateTime>, _>(col)
        .ok()
        .flatten()
        .map(|dt| dt.and_utc())
}

/// Mastodon stores site settings as YAML scalars (e.g. `--- 서울지구\n`).
/// Strip the YAML document marker and trim whitespace.
fn strip_yaml(s: &str) -> String {
    s.strip_prefix("--- ").or_else(|| s.strip_prefix("---"))
        .unwrap_or(s)
        .trim()
        .to_string()
}
