/// Initializes an eunha R2 bucket with the static assets the server requires.
///
/// Currently uploads:
///   avatars/original/missing.png  — placeholder shown when no avatar is set
///   headers/original/missing.png  — placeholder shown when no header is set
///
/// Usage:
///   eunha-init-r2 \
///     --bucket eunha-social \
///     --endpoint https://ACCOUNT_ID.r2.cloudflarestorage.com \
///     --access-key-id KEY \
///     --secret-access-key SECRET
use anyhow::{Context, Result};
use aws_sdk_s3::primitives::ByteStream;
use clap::Parser;

static AVATAR_MISSING: &[u8] = include_bytes!("../../assets/avatar_missing.png");
static HEADER_MISSING: &[u8] = include_bytes!("../../assets/header_missing.png");

#[derive(Parser, Debug)]
#[command(about = "Initialize the eunha R2 bucket with required static assets")]
struct Args {
    #[arg(long)]
    bucket: String,
    #[arg(long)]
    endpoint: String,
    #[arg(long)]
    access_key_id: String,
    #[arg(long)]
    secret_access_key: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let creds = aws_sdk_s3::config::Credentials::new(
        &args.access_key_id,
        &args.secret_access_key,
        None,
        None,
        "static",
    );
    let s3_conf = aws_sdk_s3::config::Builder::new()
        .region(aws_sdk_s3::config::Region::new("auto"))
        .credentials_provider(creds)
        .endpoint_url(&args.endpoint)
        .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
        .build();
    let s3 = aws_sdk_s3::Client::from_conf(s3_conf);

    let assets: &[(&str, &[u8])] = &[
        ("avatars/original/missing.png", AVATAR_MISSING),
        ("headers/original/missing.png", HEADER_MISSING),
    ];

    for (key, data) in assets {
        tracing::info!("uploading {} ({} bytes) ...", key, data.len());
        s3.put_object()
            .bucket(&args.bucket)
            .key(*key)
            .body(ByteStream::from(data.to_vec()))
            .content_type("image/png")
            .cache_control("public, max-age=2419200, must-revalidate")
            .send()
            .await
            .with_context(|| format!("uploading {key}"))?;
        tracing::info!("uploaded {}", key);
    }

    tracing::info!("R2 bucket initialized");
    Ok(())
}
