/// Uploads Mastodon backup media to R2 and patches the eunha database with file URLs.
/// Objects are stored under <instance-uuid>/ in the bucket, derived automatically from
/// the instance domain in the eunha DB (stable across domain renames).
///
/// Prerequisites: eunha-migrate-mastodon must have been run first so that
/// media_attachments.mastodon_file_name and accounts.mastodon_id /
/// mastodon_avatar_file_name / mastodon_header_file_name are populated.
///
/// Usage (via config file):
///   eunha-upload-media \
///     --config /etc/eunha/config.toml \
///     --media-dir ~/seoulearth_dump/media \
///     --domain seoul-earth.eunha.social
///
/// Usage (individual flags):
///   eunha-upload-media \
///     --eunha-db postgres:///eunha \
///     --media-dir ~/seoulearth_dump/media \
///     --bucket eunha-social \
///     --endpoint https://5d508a37b0c6ea183620094959bbc8d1.r2.cloudflarestorage.com \
///     --access-key-id KEY \
///     --secret-access-key SECRET \
///     --base-url https://r2.eunha.social \
///     --domain seoul-earth.eunha.social
use anyhow::{Context, Result};
use aws_sdk_s3::primitives::ByteStream;
use clap::Parser;
use futures::StreamExt;
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Parser, Debug)]
struct Args {
    /// Path to the server config TOML file (database_url and media_storage are used).
    #[arg(long)] config: Option<String>,
    /// eunha database URL (overrides config database_url).
    #[arg(long)] eunha_db: Option<String>,
    #[arg(long)] media_dir: String,
    /// S3 bucket name (overrides config media_storage.bucket).
    #[arg(long)] bucket: Option<String>,
    /// S3 endpoint URL (overrides config media_storage.endpoint).
    #[arg(long)] endpoint: Option<String>,
    /// S3 access key ID (overrides config media_storage.access_key_id).
    #[arg(long)] access_key_id: Option<String>,
    /// S3 secret access key (overrides config media_storage.secret_access_key).
    #[arg(long)] secret_access_key: Option<String>,
    /// Public base URL for uploaded media (overrides config media_storage.base_url).
    #[arg(long)] base_url: Option<String>,
    /// Instance domain in eunha DB — its UUID is used as the R2 key prefix.
    #[arg(long)] domain: String,
    /// Number of concurrent S3 uploads (default: 32).
    #[arg(long, default_value_t = 32)] concurrency: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let cfg = args.config.as_deref().map(eunha::config::Config::from_file).transpose()?;
    let ms = cfg.as_ref().map(|c| &c.media_storage);

    let eunha_db_url = args.eunha_db
        .or_else(|| cfg.as_ref().map(|c| c.database_url.clone()))
        .context("--eunha-db <url> or --config <path> with database_url")?;
    let bucket_val = args.bucket
        .or_else(|| ms.map(|m| m.bucket.clone()))
        .context("--bucket or --config with media_storage.bucket")?;
    let endpoint_val = args.endpoint
        .or_else(|| ms.and_then(|m| m.endpoint.clone()))
        .context("--endpoint or --config with media_storage.endpoint")?;
    let access_key_id_val = args.access_key_id
        .or_else(|| ms.map(|m| m.access_key_id.clone()))
        .context("--access-key-id or --config with media_storage.access_key_id")?;
    let secret_access_key_val = args.secret_access_key
        .or_else(|| ms.map(|m| m.secret_access_key.clone()))
        .context("--secret-access-key or --config with media_storage.secret_access_key")?;
    let base_url_val = args.base_url
        .or_else(|| ms.map(|m| m.base_url.clone()))
        .context("--base-url or --config with media_storage.base_url")?;

    let db = PgPool::connect(&eunha_db_url).await.context("eunha_db")?;

    let creds = aws_sdk_s3::config::Credentials::new(
        &access_key_id_val, &secret_access_key_val, None, None, "static",
    );
    let s3_conf = aws_sdk_s3::config::Builder::new()
        .region(aws_sdk_s3::config::Region::new("auto".to_string()))
        .credentials_provider(creds)
        .endpoint_url(&endpoint_val)
        .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
        .build();
    let client = aws_sdk_s3::Client::from_conf(s3_conf);
    let media_dir = PathBuf::from(&args.media_dir);

    // Derive prefix from the instance UUID — stable across domain renames.
    let instance_uuid: Uuid = sqlx::query_scalar(
        "SELECT id FROM instances WHERE domain = $1",
    )
    .bind(&args.domain)
    .fetch_one(&db)
    .await
    .with_context(|| format!("instance '{}' not found in eunha DB", args.domain))?;
    let prefix = instance_uuid.to_string();
    tracing::info!("using R2 prefix: {}", prefix);

    // ── 1. Upload all files under <prefix>/ ──────────────────────────────────
    tracing::info!("uploading files from {} under prefix '{}' (concurrency={})...", media_dir.display(), prefix, args.concurrency);
    let files = collect_files(&media_dir)?;
    let total = files.len();
    tracing::info!("{} files to upload", total);
    let client = Arc::new(client);
    let bucket = Arc::new(bucket_val);
    let prefix_arc = Arc::new(prefix.clone());
    let media_dir_arc = Arc::new(media_dir.clone());
    let uploaded = upload_parallel(client.clone(), bucket.clone(), prefix_arc.clone(), media_dir_arc, files, args.concurrency).await?;
    tracing::info!("uploaded {} files total", uploaded);

    // ── 2. Patch media_attachments URLs ──────────────────────────────────────
    tracing::info!("patching media_attachment URLs...");
    patch_media_attachments(&db, &base_url_val, &prefix_arc, &media_dir).await?;

    // ── 3. Patch account avatar/header URLs ──────────────────────────────────
    tracing::info!("patching account avatar/header URLs...");
    patch_account_media(&db, &base_url_val, &prefix_arc, &media_dir).await?;

    tracing::info!("done");
    Ok(())
}

fn collect_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_files_inner(dir, &mut files)?;
    Ok(files)
}

fn collect_files_inner(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files_inner(&path, out)?;
        } else if path.file_name().and_then(|n| n.to_str()).map(|n| !n.starts_with('.')).unwrap_or(false) {
            out.push(path);
        }
    }
    Ok(())
}

async fn upload_parallel(
    client: Arc<aws_sdk_s3::Client>,
    bucket: Arc<String>,
    prefix: Arc<String>,
    root: Arc<PathBuf>,
    files: Vec<PathBuf>,
    concurrency: usize,
) -> Result<usize> {
    let total = files.len();
    let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    futures::stream::iter(files)
        .map(|path| {
            let client = client.clone();
            let bucket = bucket.clone();
            let prefix = prefix.clone();
            let root = root.clone();
            let counter = counter.clone();
            async move {
                let rel = path.strip_prefix(root.as_ref()).unwrap();
                let rel_key = rel.to_string_lossy().replace('\\', "/");
                let key = format!("{}/{}", prefix, rel_key);
                let data = tokio::fs::read(&path).await
                    .with_context(|| format!("reading {}", path.display()))?;
                let ct = mime_guess::from_path(&path).first_or_octet_stream().to_string();
                client.put_object()
                    .bucket(bucket.as_ref())
                    .key(&key)
                    .body(ByteStream::from(data))
                    .content_type(ct)
                    .send()
                    .await
                    .with_context(|| format!("uploading {key}"))?;
                let n = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                if n % 100 == 0 {
                    tracing::info!("  {}/{} files uploaded...", n, total);
                }
                Ok::<(), anyhow::Error>(())
            }
        })
        .buffer_unordered(concurrency)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<()>>()?;

    Ok(total)
}

async fn patch_media_attachments(db: &PgPool, base_url: &str, prefix: &str, media_dir: &Path) -> Result<()> {
    let rows: Vec<(i64,)> = sqlx::query_as(
        "SELECT id FROM media_attachments WHERE file_url IS NULL",
    )
    .fetch_all(db)
    .await?;

    let mut updated = 0usize;
    for (id,) in &rows {
        let id_path = split_id(*id);
        let orig_dir = media_dir.join("media_attachments/files").join(&id_path).join("original");
        let Some(filename) = first_file_in(&orig_dir) else { continue };
        let key = format!("{}/media_attachments/files/{}/original/{}", prefix, id_path, filename);
        let file_url = format!("{}/{}", base_url.trim_end_matches('/'), key);
        let preview_key = format!("{}/media_attachments/files/{}/small/{}", prefix, id_path, filename);
        let preview_url = format!("{}/{}", base_url.trim_end_matches('/'), preview_key);
        sqlx::query(
            "UPDATE media_attachments SET file_url = $1, file_key = $2, preview_url = $3 WHERE id = $4",
        )
        .bind(&file_url).bind(&key).bind(&preview_url).bind(id)
        .execute(db)
        .await?;
        updated += 1;
    }
    tracing::info!("updated {} media_attachments", updated);
    Ok(())
}

async fn patch_account_media(db: &PgPool, base_url: &str, prefix: &str, media_dir: &Path) -> Result<()> {
    let rows: Vec<(i64,)> = sqlx::query_as(
        "SELECT id FROM accounts WHERE domain IS NULL AND (avatar IS NULL OR header IS NULL)",
    )
    .fetch_all(db)
    .await?;

    let mut updated = 0usize;
    for (id,) in &rows {
        let id_path = split_id(*id);

        let avatar_orig_dir = media_dir.join("accounts/avatars").join(&id_path).join("original");
        if let Some(fname) = first_file_in(&avatar_orig_dir) {
            let key = format!("{}/accounts/avatars/{}/original/{}", prefix, id_path, fname);
            let url = format!("{}/{}", base_url.trim_end_matches('/'), key);
            let static_key = format!("{}/accounts/avatars/{}/static/{}", prefix, id_path, fname);
            let static_url = format!("{}/{}", base_url.trim_end_matches('/'), static_key);
            sqlx::query(
                "UPDATE accounts SET avatar = $1, avatar_static = $2 WHERE id = $3 AND avatar IS NULL",
            )
            .bind(&url).bind(&static_url).bind(id)
            .execute(db).await?;
            updated += 1;
        }

        let header_orig_dir = media_dir.join("accounts/headers").join(&id_path).join("original");
        if let Some(fname) = first_file_in(&header_orig_dir) {
            let key = format!("{}/accounts/headers/{}/original/{}", prefix, id_path, fname);
            let url = format!("{}/{}", base_url.trim_end_matches('/'), key);
            let static_key = format!("{}/accounts/headers/{}/static/{}", prefix, id_path, fname);
            let static_url = format!("{}/{}", base_url.trim_end_matches('/'), static_key);
            sqlx::query(
                "UPDATE accounts SET header = $1, header_static = $2 WHERE id = $3 AND header IS NULL",
            )
            .bind(&url).bind(&static_url).bind(id)
            .execute(db).await?;
            updated += 1;
        }
    }
    tracing::info!("updated {} account avatar/headers", updated);
    Ok(())
}

fn first_file_in(dir: &Path) -> Option<String> {
    std::fs::read_dir(dir).ok()?.filter_map(|e| {
        let p = e.ok()?.path();
        if p.is_file() {
            p.file_name()?.to_str().map(str::to_owned)
        } else {
            None
        }
    }).find(|n| !n.starts_with('.'))
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
