/// Send a test push notification to all active web push subscriptions for a
/// given user account.
///
/// Usage (via config file):
///   eunha-send-test-push --config /etc/eunha/config.toml --acct alice@seoul.earth
///
/// Usage (individual flags):
///   eunha-send-test-push \
///     --db postgres://localhost/eunha \
///     --acct alice@seoul.earth
use anyhow::{bail, Context, Result};
use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use web_push::{
    ContentEncoding, SubscriptionInfo, SubscriptionKeys,
    VapidSignatureBuilder, WebPushMessageBuilder,
};

#[derive(Parser, Debug)]
struct Args {
    /// Path to the server config TOML file (database_url is used).
    #[arg(long)]
    config: Option<String>,

    /// PostgreSQL connection string for the eunha database (overrides config).
    #[arg(long)]
    db: Option<String>,

    /// Account to notify, as `username` (local) or `username@domain`.
    #[arg(long)]
    acct: String,

    /// Notification title shown in the OS notification.
    #[arg(long, default_value = "Test notification")]
    title: String,

    /// Notification body text.
    #[arg(long, default_value = "This is a test push notification from eunha.")]
    body: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let cfg = args.config.as_deref().map(eunha::config::Config::from_file).transpose()?;
    let db_url = args.db
        .or_else(|| cfg.as_ref().map(|c| c.database_url.clone()))
        .context("--db <url> or --config <path> with database_url")?;

    let db = PgPoolOptions::new()
        .max_connections(3)
        .connect(&db_url)
        .await
        .context("connect to database")?;

    // Resolve acct → account UUID
    let (username, domain_part): (&str, Option<&str>) = if let Some(at) = args.acct.find('@') {
        let (u, d) = args.acct.split_at(at);
        (u, Some(&d[1..]))
    } else {
        (args.acct.as_str(), None)
    };

    let account_id: i64 = if let Some(domain) = domain_part {
        sqlx::query_scalar!(
            r#"SELECT a.id FROM accounts a
               JOIN instances i ON i.id = a.instance_id
               WHERE a.username = $1
                 AND (i.domain = $2 OR i.custom_domain = $2)
                 AND a.domain IS NULL"#,
            username,
            domain,
        )
        .fetch_optional(&db)
        .await?
        .context(format!("account {username}@{domain} not found"))?
    } else {
        sqlx::query_scalar!(
            "SELECT id FROM accounts WHERE username = $1 AND domain IS NULL LIMIT 1",
            username,
        )
        .fetch_optional(&db)
        .await?
        .context(format!("account {username} not found (multiple instances? pass username@domain)"))?
    };

    println!("account_id: {account_id}");

    // Load subscriptions + VAPID key for this account
    let subs = sqlx::query!(
        r#"SELECT wps.id, wps.endpoint, wps.p256dh, wps.auth,
                  i.vapid_private_key, i.vapid_public_key
           FROM web_push_subscriptions wps
           JOIN accounts a ON a.id = wps.account_id
           JOIN instances i ON i.id = a.instance_id
           WHERE wps.account_id = $1"#,
        account_id,
    )
    .fetch_all(&db)
    .await?;

    if subs.is_empty() {
        bail!("no push subscriptions found for this account");
    }

    println!("found {} subscription(s)", subs.len());

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let payload = serde_json::json!({
        "notification_id": 0,
        "notification_type": "mention",
        "title": args.title,
        "body": args.body,
        "icon": "",
        "preferred_locale": "en",
    })
    .to_string();

    let mut ok = 0usize;
    let mut fail = 0usize;

    for sub in &subs {
        if sub.vapid_private_key.is_empty() {
            eprintln!("  sub {} — skipped (no VAPID key on instance)", sub.id);
            fail += 1;
            continue;
        }

        match send_one(&http, sub.endpoint.as_str(), sub.p256dh.as_str(), sub.auth.as_str(), sub.vapid_private_key.as_str(), &payload).await {
            Ok(()) => {
                println!("  sub {} → OK  ({})", sub.id, truncate(&sub.endpoint, 60));
                ok += 1;
            }
            Err(e) => {
                eprintln!("  sub {} → FAIL: {e}  ({})", sub.id, truncate(&sub.endpoint, 60));
                fail += 1;
            }
        }
    }

    println!("\n{ok} delivered, {fail} failed");
    if fail > 0 && ok == 0 {
        bail!("all deliveries failed");
    }
    Ok(())
}

async fn send_one(
    http: &reqwest::Client,
    endpoint: &str,
    p256dh: &str,
    auth: &str,
    vapid_private_pem: &str,
    payload: &str,
) -> Result<()> {
    let sub_info = SubscriptionInfo {
        endpoint: endpoint.to_string(),
        keys: SubscriptionKeys {
            auth: auth.to_string(),
            p256dh: p256dh.to_string(),
        },
    };

    let mut builder = WebPushMessageBuilder::new(&sub_info);
    builder.set_payload(ContentEncoding::AesGcm, payload.as_bytes());
    builder.set_ttl(300);

    let sig_builder = VapidSignatureBuilder::from_pem(vapid_private_pem.as_bytes(), &sub_info)
        .context("build VAPID signature")?;
    builder.set_vapid_signature(sig_builder.build()?);

    let message = builder.build()?;
    let endpoint_url = message.endpoint.to_string();
    let ttl = message.ttl;

    let mut req = http.post(endpoint_url.as_str()).header("TTL", ttl.to_string());

    if let Some(p) = message.payload {
        req = req
            .header("Content-Encoding", p.content_encoding.to_str())
            .header("Content-Type", "application/octet-stream");
        for (k, v) in &p.crypto_headers {
            req = req.header(*k, v.as_str());
        }
        req = req.body(p.content);
    }

    let resp = req.send().await.context("HTTP send")?;
    let status = resp.status();
    if !status.is_success() && status.as_u16() != 201 {
        let body = resp.text().await.unwrap_or_default();
        bail!("push relay returned {status}: {body}");
    }
    Ok(())
}

fn truncate(s: &str, n: usize) -> &str {
    if s.len() <= n {
        s
    } else {
        &s[..n]
    }
}
