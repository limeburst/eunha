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
use serde_json;

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
    let account_map = migrate_accounts(&src, &mut *tx, instance_id, args.limit_accounts, &args.domain).await?;
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

    tracing::info!("migrating follow requests...");
    migrate_follow_requests(&src, &mut *tx, &account_map).await?;

    tracing::info!("migrating status pins...");
    migrate_status_pins(&src, &mut *tx, &account_map, &status_map).await?;

    tracing::info!("migrating account notes...");
    migrate_account_notes(&src, &mut *tx, &account_map).await?;

    tracing::info!("migrating lists...");
    let list_map = migrate_lists(&src, &mut *tx, &account_map).await?;
    tracing::info!("migrated {} lists", list_map.len());

    tracing::info!("migrating list accounts...");
    migrate_list_accounts(&src, &mut *tx, &account_map, &list_map).await?;

    tracing::info!("migrating custom filters...");
    migrate_custom_filters(&src, &mut *tx, &account_map, &status_map).await?;

    tracing::info!("migrating featured tags...");
    migrate_featured_tags(&src, &mut *tx, &account_map).await?;

    tracing::info!("migrating domain blocks...");
    migrate_domain_blocks(&src, &mut *tx).await?;

    tracing::info!("migrating domain allows...");
    migrate_domain_allows(&src, &mut *tx).await?;

    tracing::info!("migrating reports...");
    let report_map = migrate_reports(&src, &mut *tx, &account_map, &status_map).await?;
    tracing::info!("migrated {} reports", report_map.len());

    tracing::info!("migrating report notes...");
    migrate_report_notes(&src, &mut *tx, &account_map, &report_map).await?;

    tracing::info!("migrating account warnings...");
    migrate_account_warnings(&src, &mut *tx, &account_map, &status_map, &report_map).await?;

    tracing::info!("migrating account moderation notes...");
    migrate_account_moderation_notes(&src, &mut *tx, &account_map).await?;

    tracing::info!("migrating admin action logs...");
    migrate_admin_action_logs(&src, &mut *tx, &account_map).await?;

    tracing::info!("migrating scheduled statuses...");
    migrate_scheduled_statuses(&src, &mut *tx, &account_map).await?;

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
    _args_domain: &str,
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

        let avatar_remote_url: Option<String> = row.try_get("avatar_remote_url").ok().flatten();
        let header_remote_url: Option<String> = row.try_get("header_remote_url").ok().flatten();
        let _avatar_file_name: Option<String> = row.try_get("avatar_file_name").ok().flatten();
        let _header_file_name: Option<String> = row.try_get("header_file_name").ok().flatten();

        // Remote accounts: use cached remote URL directly.
        // Local accounts: avatar/header files live in R2; the URL is set later
        // by eunha-upload-media. Leave NULL here so upload-media's patch step wins.
        let avatar = avatar_remote_url.filter(|s| !s.is_empty());
        let header = header_remote_url.filter(|s| !s.is_empty());

        let new_id: Option<Uuid> = sqlx::query_scalar(
            r#"INSERT INTO accounts
                 (instance_id, username, domain, display_name, note,
                  url, uri, locked, bot, discoverable,
                  private_key, public_key,
                  followers_count, following_count, statuses_count,
                  inbox_url, outbox_url, shared_inbox_url,
                  suspended_at, silenced_at,
                  avatar, header,
                  created_at, updated_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24)
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
        .bind(&avatar)
        .bind(&header)
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
        let reblog_of_id_src: Option<i64> = row.try_get("reblog_of_id").ok().flatten();
        let replies_count: Option<i64> = row.try_get("replies_count").ok().flatten();
        let reblogs_count: Option<i64> = row.try_get("reblogs_count").ok().flatten();
        let favourites_count: Option<i64> = row.try_get("favourites_count").ok().flatten();
        let deleted_at = get_ts_opt(&row, "deleted_at");
        let edited_at = get_ts_opt(&row, "edited_at");
        let created_at = get_ts(&row, "created_at")?;

        // Best-effort remapping using already-processed statuses (ORDER BY id ensures
        // originals come before their boosts/replies in the vast majority of cases).
        let in_reply_to_id: Option<i64> = in_reply_to_id_src.and_then(|id| map.get(&id)).copied();
        let reblog_of_id: Option<i64> = reblog_of_id_src.and_then(|id| map.get(&id)).copied();

        let new_id: Option<i64> = sqlx::query_scalar(
            r#"INSERT INTO statuses
                 (instance_id, account_id, text, content, spoiler_text,
                  visibility, language, sensitive, url, uri,
                  in_reply_to_id, reblog_of_id,
                  replies_count, reblogs_count, favourites_count,
                  deleted_at, edited_at, created_at)
               VALUES ($1,$2,$3,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)
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
        .bind(reblog_of_id)
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

async fn migrate_follow_requests(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
) -> Result<()> {
    let rows = sqlx::query(
        "SELECT account_id, target_account_id, uri, created_at FROM follow_requests",
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
               VALUES ($1,$2,'pending',$3,$4)
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

async fn migrate_status_pins(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
    status_map: &HashMap<i64, i64>,
) -> Result<()> {
    let rows = sqlx::query("SELECT account_id, status_id, created_at FROM status_pins")
        .fetch_all(src)
        .await?;

    for row in &rows {
        let src_account: i64 = row.get("account_id");
        let src_status: i64 = row.get("status_id");
        let (Some(&account_id), Some(&status_id)) = (account_map.get(&src_account), status_map.get(&src_status))
        else { continue };

        let created_at = get_ts(&row, "created_at")?;

        sqlx::query(
            r#"INSERT INTO status_pins (account_id, status_id, created_at)
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

async fn migrate_account_notes(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
) -> Result<()> {
    let rows = sqlx::query(
        "SELECT account_id, target_account_id, comment, created_at, updated_at FROM account_notes",
    )
    .fetch_all(src)
    .await?;

    for row in &rows {
        let src_account: i64 = row.get("account_id");
        let src_target: i64 = row.get("target_account_id");
        let (Some(&account_id), Some(&target_id)) = (account_map.get(&src_account), account_map.get(&src_target))
        else { continue };

        let comment: String = row.try_get("comment").unwrap_or_default();
        let created_at = get_ts(&row, "created_at")?;
        let updated_at = get_ts(&row, "updated_at")?;

        sqlx::query(
            r#"INSERT INTO account_notes (account_id, target_account_id, comment, created_at, updated_at)
               VALUES ($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING"#,
        )
        .bind(account_id)
        .bind(target_id)
        .bind(&comment)
        .bind(created_at)
        .bind(updated_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_lists(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
) -> Result<HashMap<i64, i64>> {
    let rows = sqlx::query(
        "SELECT id, account_id, title, replies_policy, exclusive, created_at, updated_at FROM lists",
    )
    .fetch_all(src)
    .await?;

    let mut map = HashMap::new();

    for row in &rows {
        let src_id: i64 = row.get("id");
        let src_account: i64 = row.get("account_id");
        let Some(&account_id) = account_map.get(&src_account) else { continue };

        let title: String = row.try_get("title").unwrap_or_default();
        let replies_policy = match row.try_get::<i32, _>("replies_policy").ok().unwrap_or(1) {
            0 => "followed", 1 => "list", 2 => "none", _ => "list",
        };
        let exclusive: bool = row.try_get("exclusive").unwrap_or(false);
        let created_at = get_ts(&row, "created_at")?;
        let updated_at = get_ts(&row, "updated_at")?;

        let new_id: Option<i64> = sqlx::query_scalar(
            r#"INSERT INTO lists (account_id, title, replies_policy, exclusive, created_at, updated_at)
               VALUES ($1,$2,$3,$4,$5,$6)
               RETURNING id"#,
        )
        .bind(account_id)
        .bind(&title)
        .bind(replies_policy)
        .bind(exclusive)
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

async fn migrate_list_accounts(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
    list_map: &HashMap<i64, i64>,
) -> Result<()> {
    let rows = sqlx::query("SELECT list_id, account_id FROM list_accounts")
        .fetch_all(src)
        .await?;

    for row in &rows {
        let src_list: i64 = row.get("list_id");
        let src_account: i64 = row.get("account_id");
        let (Some(&list_id), Some(&account_id)) = (list_map.get(&src_list), account_map.get(&src_account))
        else { continue };

        sqlx::query(
            "INSERT INTO list_accounts (list_id, account_id) VALUES ($1,$2) ON CONFLICT DO NOTHING",
        )
        .bind(list_id)
        .bind(account_id)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_custom_filters(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
    status_map: &HashMap<i64, i64>,
) -> Result<()> {
    let filter_rows = sqlx::query(
        "SELECT id, account_id, expires_at, phrase, context, action, created_at, updated_at FROM custom_filters",
    )
    .fetch_all(src)
    .await?;

    let mut filter_id_map: HashMap<i64, i64> = HashMap::new();

    for row in &filter_rows {
        let src_id: i64 = row.get("id");
        let src_account: i64 = row.get("account_id");
        let Some(&account_id) = account_map.get(&src_account) else { continue };

        let expires_at = get_ts_opt(&row, "expires_at");
        let phrase: String = row.try_get("phrase").unwrap_or_default();
        let context: Vec<String> = row.try_get("context").unwrap_or_default();
        let action = match row.try_get::<i32, _>("action").ok().unwrap_or(0) {
            0 => "warn", 1 => "hide", _ => "warn",
        };
        let created_at = get_ts(&row, "created_at")?;
        let updated_at = get_ts(&row, "updated_at")?;

        let new_id: Option<i64> = sqlx::query_scalar(
            r#"INSERT INTO custom_filters (account_id, expires_at, phrase, context, action, created_at, updated_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7)
               RETURNING id"#,
        )
        .bind(account_id)
        .bind(expires_at)
        .bind(&phrase)
        .bind(&context)
        .bind(action)
        .bind(created_at)
        .bind(updated_at)
        .fetch_optional(&mut *dst)
        .await?;

        if let Some(new_id) = new_id {
            filter_id_map.insert(src_id, new_id);
        }
    }

    let kw_rows = sqlx::query(
        "SELECT custom_filter_id, keyword, whole_word, created_at, updated_at FROM custom_filter_keywords",
    )
    .fetch_all(src)
    .await?;

    for row in &kw_rows {
        let src_filter: i64 = row.get("custom_filter_id");
        let Some(&filter_id) = filter_id_map.get(&src_filter) else { continue };

        let keyword: String = row.try_get("keyword").unwrap_or_default();
        let whole_word: bool = row.try_get("whole_word").unwrap_or(true);
        let created_at = get_ts(&row, "created_at")?;
        let updated_at = get_ts(&row, "updated_at")?;

        sqlx::query(
            r#"INSERT INTO custom_filter_keywords (custom_filter_id, keyword, whole_word, created_at, updated_at)
               VALUES ($1,$2,$3,$4,$5)"#,
        )
        .bind(filter_id)
        .bind(&keyword)
        .bind(whole_word)
        .bind(created_at)
        .bind(updated_at)
        .execute(&mut *dst)
        .await?;
    }

    let st_rows = sqlx::query(
        "SELECT custom_filter_id, status_id, created_at, updated_at FROM custom_filter_statuses",
    )
    .fetch_all(src)
    .await?;

    for row in &st_rows {
        let src_filter: i64 = row.get("custom_filter_id");
        let src_status: i64 = row.get("status_id");
        let (Some(&filter_id), Some(&status_id)) = (filter_id_map.get(&src_filter), status_map.get(&src_status))
        else { continue };

        let created_at = get_ts(&row, "created_at")?;
        let updated_at = get_ts(&row, "updated_at")?;

        sqlx::query(
            r#"INSERT INTO custom_filter_statuses (custom_filter_id, status_id, created_at, updated_at)
               VALUES ($1,$2,$3,$4)"#,
        )
        .bind(filter_id)
        .bind(status_id)
        .bind(created_at)
        .bind(updated_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_featured_tags(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
) -> Result<()> {
    // Build mastodon tag_id → name map from source
    let masto_tags = sqlx::query("SELECT id, name FROM tags")
        .fetch_all(src)
        .await?;
    let masto_tag_names: HashMap<i64, String> = masto_tags
        .iter()
        .map(|r| (r.get::<i64, _>("id"), r.get::<String, _>("name")))
        .collect();

    // Build eunha tag name → uuid map from destination
    let eunha_tags = sqlx::query("SELECT name, id FROM tags")
        .fetch_all(&mut *dst)
        .await?;
    let eunha_tag_by_name: HashMap<String, Uuid> = eunha_tags
        .iter()
        .map(|r| (r.get::<String, _>("name").to_lowercase(), r.get::<Uuid, _>("id")))
        .collect();

    let rows = sqlx::query(
        "SELECT account_id, tag_id, statuses_count, last_status_at, created_at, updated_at FROM featured_tags",
    )
    .fetch_all(src)
    .await?;

    for row in &rows {
        let src_account: i64 = row.get("account_id");
        let src_tag: i64 = row.get("tag_id");
        let Some(&account_id) = account_map.get(&src_account) else { continue };
        let Some(tag_name) = masto_tag_names.get(&src_tag) else { continue };
        let Some(&tag_id) = eunha_tag_by_name.get(&tag_name.to_lowercase()) else { continue };

        let statuses_count: i64 = row.try_get("statuses_count").unwrap_or(0);
        // last_status_at is a DATE column in Mastodon
        let last_status_at: Option<chrono::DateTime<chrono::Utc>> = row
            .try_get::<Option<chrono::NaiveDate>, _>("last_status_at")
            .ok()
            .flatten()
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .map(|dt| dt.and_utc());
        let created_at = get_ts(&row, "created_at")?;
        let updated_at = get_ts(&row, "updated_at")?;

        sqlx::query(
            r#"INSERT INTO featured_tags (account_id, tag_id, name, statuses_count, last_status_at, created_at, updated_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7) ON CONFLICT DO NOTHING"#,
        )
        .bind(account_id)
        .bind(tag_id)
        .bind(tag_name)
        .bind(statuses_count)
        .bind(last_status_at)
        .bind(created_at)
        .bind(updated_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_domain_blocks(
    src: &PgPool,
    dst: &mut PgConnection,
) -> Result<()> {
    let rows = sqlx::query(
        "SELECT domain, severity, reject_media, reject_reports, private_comment, public_comment, obfuscate, created_at, updated_at FROM domain_blocks",
    )
    .fetch_all(src)
    .await?;

    for row in &rows {
        let domain: String = row.try_get("domain").unwrap_or_default();
        let severity = match row.try_get::<i32, _>("severity").ok().unwrap_or(1) {
            0 => "noop", 1 => "silence", 2 => "suspend", _ => "silence",
        };
        let reject_media: bool = row.try_get("reject_media").unwrap_or(false);
        let reject_reports: bool = row.try_get("reject_reports").unwrap_or(false);
        let private_comment: Option<String> = row.try_get("private_comment").ok().flatten();
        let public_comment: Option<String> = row.try_get("public_comment").ok().flatten();
        let obfuscate: bool = row.try_get("obfuscate").unwrap_or(false);
        let created_at = get_ts(&row, "created_at")?;
        let updated_at = get_ts(&row, "updated_at")?;

        sqlx::query(
            r#"INSERT INTO domain_blocks (domain, severity, reject_media, reject_reports, private_comment, public_comment, obfuscate, created_at, updated_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (domain) DO NOTHING"#,
        )
        .bind(&domain)
        .bind(severity)
        .bind(reject_media)
        .bind(reject_reports)
        .bind(&private_comment)
        .bind(&public_comment)
        .bind(obfuscate)
        .bind(created_at)
        .bind(updated_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_domain_allows(
    src: &PgPool,
    dst: &mut PgConnection,
) -> Result<()> {
    let rows = sqlx::query("SELECT domain, created_at, updated_at FROM domain_allows")
        .fetch_all(src)
        .await?;

    for row in &rows {
        let domain: String = row.try_get("domain").unwrap_or_default();
        let created_at = get_ts(&row, "created_at")?;
        let updated_at = get_ts(&row, "updated_at")?;

        sqlx::query(
            "INSERT INTO domain_allows (domain, created_at, updated_at) VALUES ($1,$2,$3) ON CONFLICT (domain) DO NOTHING",
        )
        .bind(&domain)
        .bind(created_at)
        .bind(updated_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_reports(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
    status_map: &HashMap<i64, i64>,
) -> Result<HashMap<i64, i64>> {
    let rows = sqlx::query(
        "SELECT id, account_id, target_account_id, assigned_account_id, action_taken_by_account_id, status_ids, comment, forwarded, category, action_taken_at, uri, created_at, updated_at FROM reports ORDER BY id",
    )
    .fetch_all(src)
    .await?;

    let mut map = HashMap::new();

    for row in &rows {
        let src_id: i64 = row.get("id");
        let src_account: i64 = row.get("account_id");
        let src_target: i64 = row.get("target_account_id");
        let (Some(&account_id), Some(&target_id)) = (account_map.get(&src_account), account_map.get(&src_target))
        else { continue };

        let assigned_id: Option<i64> = row.try_get("assigned_account_id").ok().flatten();
        let action_taken_by_id: Option<i64> = row.try_get("action_taken_by_account_id").ok().flatten();
        let assigned_account_id: Option<Uuid> = assigned_id.and_then(|id| account_map.get(&id)).copied();
        let action_taken_by_account_id: Option<Uuid> = action_taken_by_id.and_then(|id| account_map.get(&id)).copied();

        let src_status_ids: Vec<i64> = row.try_get("status_ids").unwrap_or_default();
        let status_ids: Vec<i64> = src_status_ids.iter()
            .filter_map(|sid| status_map.get(sid))
            .copied()
            .collect();

        let comment: String = row.try_get("comment").unwrap_or_default();
        let forwarded: Option<bool> = row.try_get("forwarded").ok().flatten();
        let category: String = row.try_get("category").unwrap_or_else(|_| "other".to_string());
        let action_taken_at = get_ts_opt(&row, "action_taken_at");
        let uri: Option<String> = row.try_get("uri").ok().flatten();
        let created_at = get_ts(&row, "created_at")?;
        let updated_at = get_ts(&row, "updated_at")?;

        let new_id: Option<i64> = sqlx::query_scalar(
            r#"INSERT INTO reports
                 (account_id, target_account_id, assigned_account_id, action_taken_by_account_id,
                  status_ids, comment, forwarded, category, action_taken_at, uri, created_at, updated_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
               RETURNING id"#,
        )
        .bind(account_id)
        .bind(target_id)
        .bind(assigned_account_id)
        .bind(action_taken_by_account_id)
        .bind(&status_ids)
        .bind(&comment)
        .bind(forwarded)
        .bind(&category)
        .bind(action_taken_at)
        .bind(&uri)
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

async fn migrate_report_notes(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
    report_map: &HashMap<i64, i64>,
) -> Result<()> {
    let rows = sqlx::query(
        "SELECT content, report_id, account_id, created_at, updated_at FROM report_notes",
    )
    .fetch_all(src)
    .await?;

    for row in &rows {
        let src_report: i64 = row.get("report_id");
        let src_account: i64 = row.get("account_id");
        let (Some(&report_id), Some(&account_id)) = (report_map.get(&src_report), account_map.get(&src_account))
        else { continue };

        let content: String = row.try_get("content").unwrap_or_default();
        let created_at = get_ts(&row, "created_at")?;
        let updated_at = get_ts(&row, "updated_at")?;

        sqlx::query(
            r#"INSERT INTO report_notes (content, report_id, account_id, created_at, updated_at)
               VALUES ($1,$2,$3,$4,$5)"#,
        )
        .bind(&content)
        .bind(report_id)
        .bind(account_id)
        .bind(created_at)
        .bind(updated_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_account_warnings(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
    status_map: &HashMap<i64, i64>,
    report_map: &HashMap<i64, i64>,
) -> Result<()> {
    let rows = sqlx::query(
        "SELECT account_id, target_account_id, action, text, status_ids, report_id, overruled_at, created_at, updated_at FROM account_warnings",
    )
    .fetch_all(src)
    .await?;

    for row in &rows {
        let src_account: Option<i64> = row.try_get("account_id").ok().flatten();
        let src_target: Option<i64> = row.try_get("target_account_id").ok().flatten();
        let account_id: Option<Uuid> = src_account.and_then(|id| account_map.get(&id)).copied();
        let target_id: Option<Uuid> = src_target.and_then(|id| account_map.get(&id)).copied();

        if target_id.is_none() { continue }

        let action = match row.try_get::<i32, _>("action").ok().unwrap_or(0) {
            0 => "none",
            1 => "disable",
            2 => "mark_statuses_as_sensitive",
            3 => "silence",
            4 => "suspend",
            5 => "delete_statuses",
            6 => "none_and_reject_appeal",
            _ => "none",
        };

        let text: String = row.try_get("text").unwrap_or_default();
        let src_status_ids: Vec<i64> = row.try_get("status_ids").unwrap_or_default();
        let status_ids: Vec<i64> = src_status_ids.iter()
            .filter_map(|sid| status_map.get(sid))
            .copied()
            .collect();

        let src_report_id: Option<i64> = row.try_get("report_id").ok().flatten();
        let report_id: Option<i64> = src_report_id.and_then(|id| report_map.get(&id)).copied();

        let overruled_at = get_ts_opt(&row, "overruled_at");
        let created_at = get_ts(&row, "created_at")?;
        let updated_at = get_ts(&row, "updated_at")?;

        sqlx::query(
            r#"INSERT INTO account_warnings
                 (account_id, target_account_id, action, text, status_ids, report_id, overruled_at, created_at, updated_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)"#,
        )
        .bind(account_id)
        .bind(target_id)
        .bind(action)
        .bind(&text)
        .bind(&status_ids)
        .bind(report_id)
        .bind(overruled_at)
        .bind(created_at)
        .bind(updated_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_account_moderation_notes(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
) -> Result<()> {
    let rows = sqlx::query(
        "SELECT content, account_id, target_account_id, created_at, updated_at FROM account_moderation_notes",
    )
    .fetch_all(src)
    .await?;

    for row in &rows {
        let src_account: i64 = row.get("account_id");
        let src_target: i64 = row.get("target_account_id");
        let (Some(&account_id), Some(&target_id)) = (account_map.get(&src_account), account_map.get(&src_target))
        else { continue };

        let content: String = row.try_get("content").unwrap_or_default();
        let created_at = get_ts(&row, "created_at")?;
        let updated_at = get_ts(&row, "updated_at")?;

        sqlx::query(
            r#"INSERT INTO account_moderation_notes (content, account_id, target_account_id, created_at, updated_at)
               VALUES ($1,$2,$3,$4,$5)"#,
        )
        .bind(&content)
        .bind(account_id)
        .bind(target_id)
        .bind(created_at)
        .bind(updated_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_admin_action_logs(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
) -> Result<()> {
    let rows = sqlx::query(
        "SELECT account_id, action, target_type, target_id, human_identifier, route_param, permalink, created_at, updated_at FROM admin_action_logs",
    )
    .fetch_all(src)
    .await?;

    for row in &rows {
        let src_account: i64 = row.get("account_id");
        let Some(&account_id) = account_map.get(&src_account) else { continue };

        let action: String = row.try_get("action").unwrap_or_default();
        let target_type: Option<String> = row.try_get("target_type").ok().flatten();
        let target_id: Option<i64> = row.try_get("target_id").ok().flatten();
        let human_identifier: Option<String> = row.try_get("human_identifier").ok().flatten();
        let route_param: Option<String> = row.try_get("route_param").ok().flatten();
        let permalink: Option<String> = row.try_get("permalink").ok().flatten();
        let created_at = get_ts(&row, "created_at")?;
        let updated_at = get_ts(&row, "updated_at")?;

        sqlx::query(
            r#"INSERT INTO admin_action_logs
                 (account_id, action, target_type, target_id, human_identifier, route_param, permalink, created_at, updated_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)"#,
        )
        .bind(account_id)
        .bind(&action)
        .bind(&target_type)
        .bind(target_id)
        .bind(&human_identifier)
        .bind(&route_param)
        .bind(&permalink)
        .bind(created_at)
        .bind(updated_at)
        .execute(&mut *dst)
        .await?;
    }

    Ok(())
}

async fn migrate_scheduled_statuses(
    src: &PgPool,
    dst: &mut PgConnection,
    account_map: &HashMap<i64, Uuid>,
) -> Result<()> {
    let rows = sqlx::query(
        "SELECT account_id, scheduled_at, params, created_at FROM scheduled_statuses",
    )
    .fetch_all(src)
    .await?;

    for row in &rows {
        let src_account: i64 = row.get("account_id");
        let Some(&account_id) = account_map.get(&src_account) else { continue };

        let scheduled_at = get_ts_opt(&row, "scheduled_at");
        let params: Option<serde_json::Value> = row.try_get("params").ok().flatten();
        let created_at = get_ts(&row, "created_at")?;

        sqlx::query(
            "INSERT INTO scheduled_statuses (account_id, scheduled_at, params, created_at) VALUES ($1,$2,$3,$4)",
        )
        .bind(account_id)
        .bind(scheduled_at)
        .bind(&params)
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
