/// Uploads Mastodon backup media to R2 and patches the eunha database with file URLs.
///
/// Usage:
///   eunha-upload-media \
///     --mastodon-db postgres:///mastodon_src \
///     --eunha-db postgres:///eunha \
///     --media-dir ~/seoulearth_dump/media \
///     --bucket eunha-social \
///     --endpoint https://5d508a37b0c6ea183620094959bbc8d1.r2.cloudflarestorage.com \
///     --access-key-id d2f345c5441ed9c58fcef0173833afad \
///     --secret-access-key 02cbaabf4a806a6d43eafdc0c16192bf5ee29860f48ef4ca1683c91a9bbaa89f \
///     --base-url https://r2.eunha.social
use anyhow::{Context, Result};
use aws_sdk_s3::primitives::ByteStream;
use clap::Parser;
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long)] mastodon_db: String,
    #[arg(long)] eunha_db: String,
    #[arg(long)] media_dir: String,
    #[arg(long)] bucket: String,
    #[arg(long)] endpoint: String,
    #[arg(long)] access_key_id: String,
    #[arg(long)] secret_access_key: String,
    #[arg(long)] base_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let src = PgPool::connect(&args.mastodon_db).await.context("mastodon_db")?;
    let dst = PgPool::connect(&args.eunha_db).await.context("eunha_db")?;

    let creds = aws_sdk_s3::config::Credentials::new(
        &args.access_key_id, &args.secret_access_key, None, None, "static",
    );
    let s3_conf = aws_sdk_s3::config::Builder::new()
        .region(aws_sdk_s3::config::Region::new("auto".to_string()))
        .credentials_provider(creds)
        .endpoint_url(&args.endpoint)
        .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
        .build();
    let client = aws_sdk_s3::Client::from_conf(s3_conf);
    let media_dir = PathBuf::from(&args.media_dir);

    // ── 1. Account mapping: mastodon i64 id → eunha UUID ─────────────────────
    tracing::info!("building account map...");
    let masto_accounts = sqlx::query("SELECT id, username, domain FROM accounts")
        .fetch_all(&src)
        .await?;

    let eunha_accounts = sqlx::query("SELECT id, username, domain FROM accounts")
        .fetch_all(&dst)
        .await?;

    // (username, domain) → eunha UUID
    let eunha_account_lookup: HashMap<(String, Option<String>), Uuid> = eunha_accounts
        .iter()
        .map(|r| {
            let username: String = r.get("username");
            let domain: Option<String> = r.try_get("domain").ok().flatten();
            let id: Uuid = r.get("id");
            ((username, domain), id)
        })
        .collect();

    // mastodon account id → eunha UUID
    let account_map: HashMap<i64, Uuid> = masto_accounts
        .iter()
        .filter_map(|r| {
            let masto_id: i64 = r.get("id");
            let username: String = r.get("username");
            let domain: Option<String> = r.try_get("domain").ok().flatten();
            eunha_account_lookup.get(&(username, domain)).map(|&uid| (masto_id, uid))
        })
        .collect();
    tracing::info!("mapped {} accounts", account_map.len());

    // ── 2. Status mapping: mastodon i64 id → eunha i64 id ────────────────────
    tracing::info!("building status map...");
    let masto_statuses = sqlx::query("SELECT id, uri FROM statuses WHERE uri IS NOT NULL")
        .fetch_all(&src)
        .await?;

    let eunha_statuses = sqlx::query("SELECT id, uri FROM statuses WHERE uri IS NOT NULL")
        .fetch_all(&dst)
        .await?;

    let eunha_status_lookup: HashMap<String, i64> = eunha_statuses
        .iter()
        .filter_map(|r| {
            let uri: Option<String> = r.try_get("uri").ok().flatten();
            let id: i64 = r.get("id");
            uri.map(|u| (u, id))
        })
        .collect();

    let status_map: HashMap<i64, i64> = masto_statuses
        .iter()
        .filter_map(|r| {
            let masto_id: i64 = r.get("id");
            let uri: String = r.try_get("uri").ok().flatten()?;
            eunha_status_lookup.get(&uri).map(|&eid| (masto_id, eid))
        })
        .collect();
    tracing::info!("mapped {} statuses", status_map.len());

    // ── 3. Upload all files ───────────────────────────────────────────────────
    tracing::info!("uploading files from {}...", media_dir.display());
    let mut uploaded = 0usize;
    upload_dir(&client, &args.bucket, &media_dir, &media_dir, &mut uploaded).await?;
    tracing::info!("uploaded {} files total", uploaded);

    // ── 4. Patch media_attachments URLs ──────────────────────────────────────
    tracing::info!("patching media_attachment URLs...");
    patch_media_attachments(&src, &dst, &account_map, &status_map, &args.base_url).await?;

    // ── 5. Patch account avatar/header URLs ──────────────────────────────────
    tracing::info!("patching account avatar/header URLs...");
    patch_account_media(&src, &dst, &account_map, &args.base_url).await?;

    tracing::info!("done");
    Ok(())
}

async fn upload_dir(
    client: &aws_sdk_s3::Client,
    bucket: &str,
    root: &Path,
    dir: &Path,
    count: &mut usize,
) -> Result<()> {
    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.is_dir() {
            Box::pin(upload_dir(client, bucket, root, &path, count)).await?;
        } else {
            let rel = path.strip_prefix(root).unwrap();
            let key = rel.to_string_lossy().replace('\\', "/");
            let data = tokio::fs::read(&path).await?;
            let ct = mime_guess::from_path(&path).first_or_octet_stream().to_string();
            client.put_object()
                .bucket(bucket)
                .key(&key)
                .body(ByteStream::from(data))
                .content_type(ct)
                .send()
                .await
                .with_context(|| format!("uploading {key}"))?;
            *count += 1;
            if *count % 100 == 0 {
                tracing::info!("  {} files uploaded...", count);
            }
        }
    }
    Ok(())
}

async fn patch_media_attachments(
    src: &PgPool,
    dst: &PgPool,
    account_map: &HashMap<i64, Uuid>,
    status_map: &HashMap<i64, i64>,
    base_url: &str,
) -> Result<()> {
    let masto_rows = sqlx::query(
        r#"SELECT id, account_id, status_id, file_file_name
           FROM media_attachments
           WHERE file_file_name IS NOT NULL AND file_file_name != ''
           ORDER BY COALESCE(status_id, 0), id"#,
    )
    .fetch_all(src)
    .await?;

    // Group by (eunha_account_id, eunha_status_id) → Vec<(masto_id, filename)>
    let mut masto_groups: HashMap<(Uuid, Option<i64>), Vec<(i64, String)>> = HashMap::new();
    for row in &masto_rows {
        let masto_account: i64 = row.get("account_id");
        let masto_status: Option<i64> = row.try_get("status_id").ok().flatten();
        let Some(&eunha_account) = account_map.get(&masto_account) else { continue };
        let eunha_status = masto_status.and_then(|sid| status_map.get(&sid)).copied();
        let masto_id: i64 = row.get("id");
        let filename: String = row.get("file_file_name");
        masto_groups.entry((eunha_account, eunha_status)).or_default().push((masto_id, filename));
    }
    for v in masto_groups.values_mut() {
        v.sort_by_key(|&(id, _)| id);
    }

    let eunha_rows = sqlx::query(
        "SELECT id, account_id, status_id FROM media_attachments WHERE file_url IS NULL ORDER BY id",
    )
    .fetch_all(dst)
    .await?;

    let mut eunha_groups: HashMap<(Uuid, Option<i64>), Vec<i64>> = HashMap::new();
    for row in &eunha_rows {
        let account_id: Uuid = row.get("account_id");
        let status_id: Option<i64> = row.try_get("status_id").ok().flatten();
        let id: i64 = row.get("id");
        eunha_groups.entry((account_id, status_id)).or_default().push(id);
    }

    let mut updated = 0usize;
    for ((eunha_account, eunha_status), masto_attachments) in &masto_groups {
        let Some(eunha_ids) = eunha_groups.get(&(*eunha_account, *eunha_status)) else { continue };
        for (i, (masto_id, filename)) in masto_attachments.iter().enumerate() {
            let Some(&eunha_id) = eunha_ids.get(i) else { break };
            let id_path = split_id(*masto_id);
            let file_url = format!("{}/media_attachments/files/{}/original/{}", base_url, id_path, filename);
            let preview_url = format!("{}/media_attachments/files/{}/small/{}", base_url, id_path, filename);
            let key = format!("media_attachments/files/{}/original/{}", id_path, filename);
            sqlx::query(
                "UPDATE media_attachments SET file_url = $1, file_key = $2, preview_url = $3 WHERE id = $4",
            )
            .bind(&file_url)
            .bind(&key)
            .bind(&preview_url)
            .bind(eunha_id)
            .execute(dst)
            .await?;
            updated += 1;
        }
    }
    tracing::info!("updated {} media_attachments", updated);
    Ok(())
}

async fn patch_account_media(
    src: &PgPool,
    dst: &PgPool,
    account_map: &HashMap<i64, Uuid>,
    base_url: &str,
) -> Result<()> {
    let rows = sqlx::query(
        "SELECT id, avatar_file_name, header_file_name FROM accounts WHERE avatar_file_name IS NOT NULL OR header_file_name IS NOT NULL",
    )
    .fetch_all(src)
    .await?;

    let mut updated = 0usize;
    for row in &rows {
        let masto_id: i64 = row.get("id");
        let Some(&eunha_id) = account_map.get(&masto_id) else { continue };
        let id_path = split_id(masto_id);

        let avatar: Option<String> = row.try_get("avatar_file_name").ok().flatten();
        if let Some(ref fname) = avatar {
            if !fname.is_empty() {
                let url = format!("{}/accounts/avatars/{}/original/{}", base_url, id_path, fname);
                let static_url = format!("{}/accounts/avatars/{}/static/{}", base_url, id_path, fname);
                sqlx::query("UPDATE accounts SET avatar = $1, avatar_static = $2 WHERE id = $3")
                    .bind(&url).bind(&static_url).bind(eunha_id)
                    .execute(dst).await?;
                updated += 1;
            }
        }

        let header: Option<String> = row.try_get("header_file_name").ok().flatten();
        if let Some(ref fname) = header {
            if !fname.is_empty() {
                let url = format!("{}/accounts/headers/{}/original/{}", base_url, id_path, fname);
                let static_url = format!("{}/accounts/headers/{}/static/{}", base_url, id_path, fname);
                sqlx::query("UPDATE accounts SET header = $1, header_static = $2 WHERE id = $3")
                    .bind(&url).bind(&static_url).bind(eunha_id)
                    .execute(dst).await?;
                updated += 1;
            }
        }
    }
    tracing::info!("updated {} account avatar/headers", updated);
    Ok(())
}

/// Converts a Mastodon numeric ID into Paperclip's directory path:
/// 109328195934886822 → "109/328/195/934/886/822"
fn split_id(id: i64) -> String {
    let s = format!("{:018}", id);
    s.as_bytes()
        .chunks(3)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect::<Vec<_>>()
        .join("/")
}
