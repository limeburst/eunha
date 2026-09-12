use clap::{Parser, Subcommand};
use eunha::{config, migrate, tenants};
use std::{path::PathBuf, sync::Arc};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(name = "eunha", about = "A Mastodon-compatible ActivityPub server")]
struct Args {
    /// Serve every instance configured in this directory — one `*.toml` per
    /// instance, each answering to its `instance.domain` — from this one
    /// process. Without it, eunha serves the single instance in `config.toml`
    /// and the environment.
    #[arg(long, value_name = "DIR", global = true)]
    tenants: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Apply pending database migrations and exit.
    ///
    /// Separate from serving on purpose: a migration takes as long as it takes,
    /// and some of them are destructive. Running them from a deploy script,
    /// before the new binary starts, means a failure is found with the old
    /// version still serving rather than with nothing serving at all.
    Migrate {
        /// Report what is pending without applying anything. Exits non-zero if
        /// a database is behind this binary.
        #[arg(long)]
        check: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "eunha=debug,tower_http=info,sqlx=warn".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    if let Some(Command::Migrate { check }) = args.command {
        return migrate_databases(args.tenants.as_deref(), check).await;
    }

    let configs = match &args.tenants {
        Some(dir) => tenants::load_dir(dir)?,
        None => vec![tenants::TenantConfig {
            source: "config.toml".to_string(),
            config: config::Config::from_env()?,
        }],
    };
    let tenants = Arc::new(tenants::start(configs).await?);
    let bind_address = tenants.bind_address().to_string();
    let serving = tenants.states().await.len();
    if let Some(dir) = args.tenants {
        reload_on_hangup(tenants.clone(), dir)?;
    }
    let app = tenants.router();

    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    tracing::info!(tenants = serving, "listening on {bind_address}");
    axum::serve(listener, app).await?;

    Ok(())
}

/// Reread the tenants directory each time the process is sent SIGHUP, and serve
/// what it says now: start the tenants added, restart those whose file changed,
/// stop those removed. A directory that could not be served as a whole is
/// refused, and the tenants already running go on as they were.
fn reload_on_hangup(tenants: Arc<tenants::Tenants>, dir: PathBuf) -> anyhow::Result<()> {
    let mut hangups = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    tenants::spawn(async move {
        while hangups.recv().await.is_some() {
            tracing::info!(directory = %dir.display(), "rereading the tenants directory");
            let reloaded = match tenants::load_dir(&dir) {
                Ok(configs) => tenants.reload(configs).await,
                Err(e) => Err(e),
            };
            match reloaded {
                Ok(reloaded) => tracing::info!(
                    started = ?reloaded.started,
                    restarted = ?reloaded.restarted,
                    stopped = ?reloaded.stopped,
                    unavailable = ?reloaded.unavailable,
                    kept = reloaded.kept.len(),
                    "tenants reloaded"
                ),
                Err(e) => tracing::error!(
                    error = %format!("{e:#}"),
                    "tenants not reloaded; the ones running go on as they were"
                ),
            }
        }
    });
    Ok(())
}

/// Migrate, or with `check` report on, the single instance's database or every
/// tenant's in `tenants`.
///
/// Migrating needs a database and nothing else. Loading a single instance's
/// full config would make a deploy script's migrate step fail for want of an S3
/// bucket, which has no bearing on whether the schema can be brought up to
/// date.
async fn migrate_databases(tenants: Option<&std::path::Path>, check: bool) -> anyhow::Result<()> {
    let targets: Vec<(String, String)> = match tenants {
        Some(dir) => tenants::load_dir(dir)?
            .into_iter()
            .map(|tenant| (format!("{}: ", tenant.source), tenant.config.database_url))
            .collect(),
        None => vec![(String::new(), migration_database_url()?)],
    };

    let mut behind = false;
    for (label, database_url) in targets {
        let db = tenants::connect(
            &database_url,
            &config::DatabasePoolConfig {
                max_connections: 1,
                ..Default::default()
            },
        )
        .await?;
        match (check, migrate::pending(&db).await?) {
            (true, None) => println!("{label}Database is up to date."),
            (true, Some(pending)) => {
                println!("{label}{pending}");
                behind = true;
            }
            (false, _) => {
                migrate::run(&db).await?;
                println!("{label}Migrations applied.");
            }
        }
    }
    if behind {
        std::process::exit(1);
    }
    Ok(())
}

/// The database to migrate: `DATABASE_URL` if set (including from `.env`),
/// otherwise whatever the server would have used.
fn migration_database_url() -> anyhow::Result<String> {
    dotenvy::dotenv().ok();
    if let Ok(url) = std::env::var("DATABASE_URL") {
        if !url.is_empty() {
            return Ok(url);
        }
    }
    Ok(config::Config::from_env()?.database_url)
}
