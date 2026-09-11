//! One process, any number of instances.
//!
//! Every eunha process serves a registry of tenants — an instance each, with its
//! own configuration, database pool, Redis namespace, background tasks and
//! router — and hands every request to one of them by its `Host` header before
//! anything else sees it. A single instance is a registry of one, read from
//! `config.toml` and the environment as it always was; `--tenants <dir>` reads
//! one configuration file per instance from a directory.
//!
//! What is per tenant is everything a request or a background task touches.
//! What is per process is what the listener, the SSRF-guarded resolver and the
//! shared budget of outbound deliveries impose: the address to listen on, the
//! private networks federation may reach, and how many deliveries may be in
//! flight. Tenants in one directory must agree on those, and the process
//! refuses to start when they do not.
//!
//! Sharing a process also means sharing its capacity. Among several tenants
//! each has a limit on requests in flight, past which its requests are shed
//! with 503 rather than queued, so one being flooded cannot slow the others.
//!
//! And a process takes on only what it can hold. It refuses to start with more
//! tenants than its ceiling, which is how many one crash may take down, or with
//! database pools that could open more connections than their PostgreSQL
//! server accepts — a budget that, overrun, fails whichever tenant happens to
//! ask last rather than failing the configuration at startup.
//!
//! Everything a tenant does runs in its [`span`], so every line logged names
//! the instance it was for. Tasks keep it through [`spawn`] and
//! [`spawn_blocking`]; `clippy.toml` refuses Tokio's own, which would start
//! them outside every span.

use crate::{config, migrate, state::AppState};
use anyhow::{Context as _, Result};
use axum::{
    extract::Request,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Router,
};
use futures::StreamExt as _;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    Connection as _, Executor as _, PgConnection,
};
use std::{collections::HashMap, path::Path, str::FromStr as _, sync::Arc};
use tokio::sync::Semaphore;
use tower::ServiceExt as _;
use tracing::Instrument as _;

/// The span a tenant's work runs in, so that every line it logs names the
/// instance it was for: `tenant{domain=seoul.earth}`.
///
/// At ERROR, so that no filter keeping an event drops the tenant it belongs
/// to, and a root span, so that it never nests inside another tenant's.
pub fn span(domain: &str) -> tracing::Span {
    tracing::error_span!(parent: None, "tenant", domain = %domain)
}

/// `tokio::spawn`, keeping the span the task was spawned from. A task Tokio
/// starts directly begins outside every span, so nothing it logged would say
/// which tenant it was working for.
#[allow(clippy::disallowed_methods)]
pub fn spawn<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    tokio::spawn(future.in_current_span())
}

/// `tokio::task::spawn_blocking`, keeping the span the work was handed over
/// from — and the subscriber, which a thread of the blocking pool would not
/// otherwise know when it is only the calling thread's default, as in tests.
#[allow(clippy::disallowed_methods)]
pub fn spawn_blocking<F, R>(work: F) -> tokio::task::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let span = tracing::Span::current();
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    tokio::task::spawn_blocking(move || {
        tracing::dispatcher::with_default(&dispatch, || span.in_scope(work))
    })
}

/// How many tenants are brought up at once.
const STARTUP_CONCURRENCY: usize = 8;

/// How many of a database server's tenants are tried when asking it how many
/// connections it accepts, and how long each attempt may take.
const SERVER_PROBE_ATTEMPTS: usize = 3;
const SERVER_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// A tenant's configuration, and where it was read from for error messages.
pub struct TenantConfig {
    pub source: String,
    pub config: config::Config,
}

/// Every `*.toml` in `dir`, one tenant each, in file-name order.
///
/// Each file is read on its own, without the environment: an environment
/// variable belongs to the whole process, and letting it override one tenant's
/// file would override every tenant's.
pub fn load_dir(dir: &Path) -> Result<Vec<TenantConfig>> {
    let mut paths = std::fs::read_dir(dir)
        .with_context(|| format!("reading the tenants directory {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect::<Vec<_>>();
    paths.sort();
    anyhow::ensure!(
        !paths.is_empty(),
        "no tenant configuration (*.toml) in {}",
        dir.display()
    );
    paths
        .into_iter()
        .map(|path| {
            let source = path.display().to_string();
            let config =
                config::Config::from_file(&source).with_context(|| format!("reading {source}"))?;
            Ok(TenantConfig { source, config })
        })
        .collect()
}

/// Open a pool whose connections resolve unqualified names against the eunha
/// schema first, then public. Every app query is schema-qualified, so this only
/// decides where sqlx keeps its own `_sqlx_migrations` ledger — out of `public`,
/// which stays a pure mirror of Mastodon's schema.
pub async fn connect(
    database_url: &str,
    budget: &config::DatabasePoolConfig,
) -> Result<sqlx::PgPool> {
    budget.validate()?;
    Ok(PgPoolOptions::new()
        .max_connections(budget.max_connections)
        .min_connections(budget.min_connections)
        .acquire_timeout(std::time::Duration::from_secs(
            budget.acquire_timeout_seconds,
        ))
        .idle_timeout(std::time::Duration::from_secs(budget.idle_timeout_seconds))
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                conn.execute("SET search_path TO eunha, public").await?;
                Ok(())
            })
        })
        .connect(database_url)
        .await?)
}

/// A host as tenants are looked up by: trimmed, lower-case, without a trailing
/// dot.
fn normalize_host(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// What the tenants of one process agreed on.
struct ProcessSettings {
    bind_address: String,
    delivery_concurrency: usize,
    max_tenants: usize,
    database_connections: Option<u64>,
}

/// What belongs to the process rather than to any one tenant, which every
/// tenant therefore has to agree on — and no two tenants may serve one host.
fn process_settings(configs: &[TenantConfig]) -> Result<ProcessSettings> {
    let first = configs.first().context("no tenants to serve")?;
    let delivery_concurrency = first
        .config
        .workers
        .sanitized()
        .process_delivery_concurrency;
    let mut hosts: HashMap<String, &str> = HashMap::new();
    for tenant in configs {
        anyhow::ensure!(
            tenant.config.bind_address == first.config.bind_address,
            "{} listens on {} but {} on {}: one process has one listener, so every tenant \
             must name the same bind_address",
            first.source,
            first.config.bind_address,
            tenant.source,
            tenant.config.bind_address,
        );
        anyhow::ensure!(
            tenant.config.allowed_private_networks == first.config.allowed_private_networks,
            "{} and {} allow different private networks: the SSRF-guarded resolver is shared \
             by the whole process, so every tenant must name the same allowed_private_networks",
            first.source,
            tenant.source,
        );
        let theirs = tenant
            .config
            .workers
            .sanitized()
            .process_delivery_concurrency;
        anyhow::ensure!(
            theirs == delivery_concurrency,
            "{} allows {delivery_concurrency} deliveries in flight but {} allows {theirs}: the \
             budget is shared by the whole process, so every tenant must name the same \
             process_delivery_concurrency",
            first.source,
            tenant.source,
        );
        anyhow::ensure!(
            tenant.config.limits.max_tenants() == first.config.limits.max_tenants(),
            "{} allows {} tenants in its process but {} allows {}: every tenant must name the \
             same process_max_tenants",
            first.source,
            first.config.limits.max_tenants(),
            tenant.source,
            tenant.config.limits.max_tenants(),
        );
        anyhow::ensure!(
            tenant.config.limits.process_database_connections
                == first.config.limits.process_database_connections,
            "{} and {} give the process different database connection budgets: every tenant \
             must name the same process_database_connections",
            first.source,
            tenant.source,
        );
        let host = normalize_host(&tenant.config.instance.domain);
        if let Some(other) = hosts.insert(host.clone(), &tenant.source) {
            anyhow::bail!("{other} and {} both serve {host}", tenant.source);
        }
    }
    Ok(ProcessSettings {
        bind_address: first.config.bind_address.clone(),
        delivery_concurrency,
        max_tenants: first.config.limits.max_tenants(),
        database_connections: first.config.limits.process_database_connections,
    })
}

/// Whether the process can take these tenants on at all: no more than its
/// ceiling, and pools that fit the connection budget it was given, if any.
fn admit(configs: &[TenantConfig], settings: &ProcessSettings) -> Result<()> {
    anyhow::ensure!(
        configs.len() <= settings.max_tenants,
        "{} tenants are configured but a process serves at most {}: every tenant in it goes \
         down with it, so serve the rest from another process or raise \
         process_max_tenants in [limits]",
        configs.len(),
        settings.max_tenants,
    );
    if let Some(budget) = settings.database_connections {
        let wanted = configs
            .iter()
            .map(|tenant| u64::from(tenant.config.database_pool.max_connections))
            .sum::<u64>();
        anyhow::ensure!(
            wanted <= budget,
            "the tenants' database pools may open {wanted} connections between them but the \
             process is given {budget}: lower their database_pool.max_connections, serve fewer \
             tenants here, or raise process_database_connections in [limits]",
        );
    }
    Ok(())
}

/// The connections the tenants' pools may open on one PostgreSQL server.
struct ServerDemand<'a> {
    server: String,
    connections: u64,
    sources: Vec<&'a str>,
    urls: Vec<&'a str>,
}

/// The tenants' pools grouped by the server they connect to. A tenant whose
/// database URL does not parse is left out: it cannot connect, so it will not
/// start, and saying why is `start_one`'s job.
fn demand_by_server(configs: &[TenantConfig]) -> Vec<ServerDemand<'_>> {
    let mut servers: Vec<ServerDemand<'_>> = Vec::new();
    for tenant in configs {
        let Ok(options) = PgConnectOptions::from_str(&tenant.config.database_url) else {
            continue;
        };
        let server = match options.get_socket() {
            Some(socket) => format!("{}:{}", socket.display(), options.get_port()),
            None => format!("{}:{}", options.get_host(), options.get_port()),
        };
        let index = match servers.iter().position(|s| s.server == server) {
            Some(index) => index,
            None => {
                servers.push(ServerDemand {
                    server,
                    connections: 0,
                    sources: Vec::new(),
                    urls: Vec::new(),
                });
                servers.len() - 1
            }
        };
        let entry = &mut servers[index];
        entry.connections += u64::from(tenant.config.database_pool.max_connections);
        entry.sources.push(&tenant.source);
        entry.urls.push(&tenant.config.database_url);
    }
    servers
}

/// Whether one server's slots hold what the tenants' pools may ask of it.
fn fits(demand: &ServerDemand<'_>, slots: u64) -> Result<()> {
    anyhow::ensure!(
        demand.connections <= slots,
        "{} may open {} database connections between them on {}, which accepts {slots} \
         (max_connections less the reserved ones): lower their database_pool.max_connections \
         or serve fewer tenants from this server",
        demand.sources.join(", "),
        demand.connections,
        demand.server,
    );
    Ok(())
}

/// Ask every PostgreSQL server the tenants use how many connections it accepts,
/// and refuse pools that add up to more.
///
/// This sees only this process. Several sharing a server have to divide it
/// between them, which is what `process_database_connections` is for. A server
/// that none of its first few tenants can reach is not checked; those tenants
/// will not start either, and trying all of them would hold up the rest.
async fn check_database_servers(configs: &[TenantConfig]) -> Result<()> {
    for demand in demand_by_server(configs) {
        let mut slots = None;
        for url in demand.urls.iter().take(SERVER_PROBE_ATTEMPTS) {
            let asked = tokio::time::timeout(SERVER_PROBE_TIMEOUT, server_slots(url)).await;
            match asked.unwrap_or_else(|_| Err(anyhow::anyhow!("no answer in time"))) {
                Ok(n) => {
                    slots = Some(n);
                    break;
                }
                Err(e) => tracing::warn!(
                    server = %demand.server,
                    error = %format!("{e:#}"),
                    "could not ask a database server how many connections it accepts"
                ),
            }
        }
        if let Some(slots) = slots {
            fits(&demand, slots)?;
        }
    }
    Ok(())
}

/// Connections a server accepts from roles that are not superusers.
async fn server_slots(database_url: &str) -> Result<u64> {
    let mut conn = PgConnection::connect(database_url).await?;
    // `reserved_connections` arrived in PostgreSQL 16; `true` makes an older
    // server answer NULL rather than fail.
    let slots: i64 = sqlx::query_scalar(
        "SELECT current_setting('max_connections')::int8 \
              - current_setting('superuser_reserved_connections')::int8 \
              - COALESCE(current_setting('reserved_connections', true), '0')::int8",
    )
    .fetch_one(&mut conn)
    .await?;
    conn.close().await.ok();
    Ok(u64::try_from(slots).unwrap_or(0))
}

/// A tenant that is serving: its router, and the limit on requests it may have
/// in flight, when it has one.
#[derive(Clone)]
struct Tenant {
    router: Router,
    in_flight: Option<Arc<Semaphore>>,
}

impl Tenant {
    fn new(state: &AppState, shared: bool) -> Self {
        Self {
            router: crate::build_app(state.clone()),
            in_flight: state
                .config
                .limits
                .request_limit(shared)
                .map(|limit| Arc::new(Semaphore::new(limit))),
        }
    }
}

/// One tenant's place in the registry.
enum Slot {
    Serving(Tenant),
    /// It did not start: its database is behind this binary, or it failed.
    Unavailable,
}

/// Every tenant a process serves, by host.
pub struct Tenants {
    by_host: HashMap<String, Slot>,
    /// The only tenant, which answers every host, as a lone instance always has.
    only: Option<Tenant>,
    bind_address: String,
    states: Vec<AppState>,
}

enum Lookup<'a> {
    Serving(&'a Tenant),
    Unavailable,
    Unknown,
}

/// Bring every tenant up: its pool, the migration check, its state, the move of
/// its signing keys, its background tasks and its router.
///
/// Nothing starts unless the process can hold all of them — see [`admit`] and
/// [`check_database_servers`]. Past that, a lone tenant whose database is
/// behind this binary, or that cannot start, stops the process, as a single
/// instance always has. Among several it is left out — its host answers 503 —
/// and the others serve: one tenant's pending migration should not take its
/// neighbours down with it.
pub async fn start(configs: Vec<TenantConfig>) -> Result<Tenants> {
    let settings = process_settings(&configs)?;
    admit(&configs, &settings)?;
    check_database_servers(&configs).await?;
    crate::federation::delivery::set_process_delivery_concurrency(settings.delivery_concurrency);
    let lone = configs.len() == 1;
    let mut by_host = HashMap::new();
    let mut states = Vec::new();
    let mut only = None;

    let mut started = futures::stream::iter(configs)
        .map(|tenant| async move {
            let host = normalize_host(&tenant.config.instance.domain);
            let source = tenant.source.clone();
            (host, source, start_one(tenant, !lone).await)
        })
        .buffered(STARTUP_CONCURRENCY);
    while let Some((host, source, result)) = started.next().await {
        match result {
            Ok((state, tenant)) => {
                if lone {
                    only = Some(tenant.clone());
                }
                by_host.insert(host, Slot::Serving(tenant));
                states.push(state);
            }
            Err(e) if lone => return Err(e),
            Err(e) => {
                tracing::error!(
                    tenant = %host,
                    source = %source,
                    error = %format!("{e:#}"),
                    "tenant not started; its host will answer 503"
                );
                by_host.insert(host, Slot::Unavailable);
            }
        }
    }

    Ok(Tenants {
        by_host,
        only,
        bind_address: settings.bind_address,
        states,
    })
}

async fn start_one(tenant: TenantConfig, shared: bool) -> Result<(AppState, Tenant)> {
    let TenantConfig { source, config } = tenant;
    let db = connect(&config.database_url, &config.database_pool)
        .await
        .with_context(|| format!("{source}: connecting to its database"))?;

    // Serving refuses a schema this binary does not know, rather than running
    // queries against a shape that has moved underneath them. `eunha migrate`
    // is the fix, and the message says so.
    if let Some(pending) = migrate::pending(&db).await? {
        anyhow::bail!("{pending}. Run `eunha migrate` before starting the server.");
    }

    let state = AppState::new(db, config)
        .await
        .with_context(|| format!("{source}: starting"))?;

    // Mastodon 4.7's post-deploy migration, which cannot be SQL: the keys it
    // moves have to be encrypted with this instance's configured secrets.
    if let Err(e) = crate::federation::keypair::migrate_local_keypairs(&state).await {
        tracing::error!(
            tenant = %state.instance.domain,
            error = %e,
            "could not move local signing keys into `keypairs`"
        );
    }
    crate::background::spawn(state.clone());
    let tenant = Tenant::new(&state, shared);
    Ok((state, tenant))
}

impl Tenants {
    /// A registry of tenants whose states are already built, served on
    /// `bind_address` — for embedding eunha and for tests, which bring their own
    /// databases up. Nothing is connected, checked or spawned here.
    pub fn from_states(bind_address: &str, states: Vec<AppState>) -> Result<Self> {
        anyhow::ensure!(!states.is_empty(), "no tenants to serve");
        let shared = states.len() > 1;
        let mut by_host = HashMap::new();
        for state in &states {
            let host = normalize_host(&state.instance.domain);
            anyhow::ensure!(
                by_host
                    .insert(host.clone(), Slot::Serving(Tenant::new(state, shared)))
                    .is_none(),
                "two tenants both serve {host}"
            );
        }
        let only = match (shared, by_host.values().next()) {
            (false, Some(Slot::Serving(tenant))) => Some(tenant.clone()),
            _ => None,
        };
        Ok(Self {
            by_host,
            only,
            bind_address: bind_address.to_string(),
            states,
        })
    }

    /// The address every tenant agreed to be served on.
    pub fn bind_address(&self) -> &str {
        &self.bind_address
    }

    /// Every tenant that started, in the order they were configured.
    pub fn states(&self) -> &[AppState] {
        &self.states
    }

    fn lookup(&self, host: &str) -> Lookup<'_> {
        if let Some(tenant) = &self.only {
            return Lookup::Serving(tenant);
        }
        let host = normalize_host(host);
        let slot = self.by_host.get(&host).or_else(|| {
            // `Host` carries a port when the client connected to a
            // non-default one; a tenant's domain usually does not.
            let (name, port) = host.rsplit_once(':')?;
            port.bytes()
                .all(|b| b.is_ascii_digit())
                .then(|| self.by_host.get(name))
                .flatten()
        });
        match slot {
            Some(Slot::Serving(tenant)) => Lookup::Serving(tenant),
            Some(Slot::Unavailable) => Lookup::Unavailable,
            None => Lookup::Unknown,
        }
    }

    /// A router that hands each request to its tenant's router.
    pub fn into_router(self) -> Router {
        let tenants = Arc::new(self);
        Router::new().fallback(move |req: Request| {
            let tenants = tenants.clone();
            async move { tenants.dispatch(req).await }
        })
    }

    async fn dispatch(&self, req: Request) -> Response {
        // HTTP/1.1 names the host in `Host`; HTTP/2 in the request's authority.
        let host = req
            .headers()
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            .or_else(|| req.uri().authority().map(|authority| authority.as_str()))
            .unwrap_or_default();
        match self.lookup(host) {
            Lookup::Serving(tenant) => {
                // Held until the tenant's router has produced a response. A
                // streamed or upgraded response lets go of it as soon as it
                // starts, so a long-lived connection does not use up the limit.
                let _permit = match &tenant.in_flight {
                    Some(limit) => match limit.clone().try_acquire_owned() {
                        Ok(permit) => Some(permit),
                        Err(_) => return busy(),
                    },
                    None => None,
                };
                match tenant.router.clone().oneshot(req).await {
                    Ok(response) => response,
                    Err(never) => match never {},
                }
            }
            Lookup::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "This instance is unavailable.",
            )
                .into_response(),
            Lookup::Unknown => (
                StatusCode::MISDIRECTED_REQUEST,
                "No instance is served at this host.",
            )
                .into_response(),
        }
    }
}

/// A tenant at its limit of requests in flight: shed, not queued, so that the
/// requests piling up against one tenant cannot hold on to the process.
fn busy() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(header::RETRY_AFTER, "1")],
        "This instance is busy; try again shortly.",
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(
        source: &str,
        domain: &str,
        bind: &str,
        allowed: &[&str],
        extra: &str,
    ) -> TenantConfig {
        let allowed = allowed
            .iter()
            .map(|net| format!("\"{net}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let config: config::Config = toml::from_str(&format!(
            r#"
database_url = "postgres:///unused"
redis_url = "redis://127.0.0.1/0"
bind_address = "{bind}"
allowed_private_networks = [{allowed}]

[media_storage]
bucket = "b"
region = "auto"
endpoint = "http://127.0.0.1:9"
access_key_id = "k"
secret_access_key = "s"
base_url = "http://127.0.0.1:9"

[resend]
api_key = ""
from = "admin@example.test"

[instance]
domain = "{domain}"
title = "t"
contact_email = "admin@example.test"
vapid_private_key = ""
vapid_public_key = ""

{extra}
"#
        ))
        .expect("test configuration parses");
        TenantConfig {
            source: source.to_string(),
            config,
        }
    }

    fn serving(router: Router, limit: Option<usize>) -> Tenant {
        Tenant {
            router,
            in_flight: limit.map(|limit| Arc::new(Semaphore::new(limit))),
        }
    }

    fn tenants(hosts: Vec<(&str, Option<Tenant>)>) -> Tenants {
        Tenants {
            by_host: hosts
                .into_iter()
                .map(|(host, tenant)| {
                    let slot = match tenant {
                        Some(tenant) => Slot::Serving(tenant),
                        None => Slot::Unavailable,
                    };
                    (host.to_string(), slot)
                })
                .collect(),
            only: None,
            bind_address: String::new(),
            states: Vec::new(),
        }
    }

    fn request(host: &str) -> Request {
        Request::builder()
            .uri("/")
            .header(header::HOST, host)
            .body(axum::body::Body::empty())
            .unwrap()
    }

    #[test]
    fn hosts_are_matched_without_case_a_trailing_dot_or_a_port() {
        let t = tenants(vec![("seoul.earth", Some(serving(Router::new(), None)))]);
        for host in [
            "seoul.earth",
            "Seoul.Earth",
            "seoul.earth.",
            "seoul.earth:443",
        ] {
            assert!(matches!(t.lookup(host), Lookup::Serving(_)), "{host}");
        }
        assert!(matches!(t.lookup("example.social"), Lookup::Unknown));
        assert!(matches!(
            t.lookup("seoul.earth:not-a-port"),
            Lookup::Unknown
        ));
    }

    #[test]
    fn a_tenant_that_did_not_start_is_unavailable_not_unknown() {
        let t = tenants(vec![
            ("seoul.earth", Some(serving(Router::new(), None))),
            ("example.social", None),
        ]);
        assert!(matches!(t.lookup("example.social"), Lookup::Unavailable));
    }

    #[test]
    fn a_lone_tenant_answers_every_host() {
        let mut t = tenants(vec![("seoul.earth", Some(serving(Router::new(), None)))]);
        t.only = Some(serving(Router::new(), None));
        assert!(matches!(t.lookup("localhost:3000"), Lookup::Serving(_)));
        assert!(matches!(t.lookup(""), Lookup::Serving(_)));
    }

    #[test]
    fn tenants_must_agree_on_what_the_process_owns() {
        let agreeing = [
            tenant("a.toml", "a.example", "127.0.0.1:3000", &[], ""),
            tenant("b.toml", "b.example", "127.0.0.1:3000", &[], ""),
        ];
        let settings = process_settings(&agreeing).unwrap();
        assert_eq!(settings.bind_address, "127.0.0.1:3000");
        assert_eq!(settings.delivery_concurrency, 256);

        let listeners = [
            tenant("a.toml", "a.example", "127.0.0.1:3000", &[], ""),
            tenant("b.toml", "b.example", "127.0.0.1:3001", &[], ""),
        ];
        assert!(process_settings(&listeners)
            .err()
            .is_some_and(|e| format!("{e:#}").contains("bind_address")));

        let networks = [
            tenant("a.toml", "a.example", "127.0.0.1:3000", &[], ""),
            tenant("b.toml", "b.example", "127.0.0.1:3000", &["10.0.0.0/8"], ""),
        ];
        assert!(process_settings(&networks)
            .err()
            .is_some_and(|e| format!("{e:#}").contains("allowed_private_networks")));

        let deliveries = [
            tenant("a.toml", "a.example", "127.0.0.1:3000", &[], ""),
            tenant(
                "b.toml",
                "b.example",
                "127.0.0.1:3000",
                &[],
                "[workers]\nprocess_delivery_concurrency = 32",
            ),
        ];
        assert!(process_settings(&deliveries)
            .err()
            .is_some_and(|e| format!("{e:#}").contains("process_delivery_concurrency")));

        let ceilings = [
            tenant("a.toml", "a.example", "127.0.0.1:3000", &[], ""),
            tenant(
                "b.toml",
                "b.example",
                "127.0.0.1:3000",
                &[],
                "[limits]\nprocess_max_tenants = 10",
            ),
        ];
        assert!(process_settings(&ceilings)
            .err()
            .is_some_and(|e| format!("{e:#}").contains("process_max_tenants")));

        let budgets = [
            tenant(
                "a.toml",
                "a.example",
                "127.0.0.1:3000",
                &[],
                "[limits]\nprocess_database_connections = 100",
            ),
            tenant("b.toml", "b.example", "127.0.0.1:3000", &[], ""),
        ];
        assert!(process_settings(&budgets)
            .err()
            .is_some_and(|e| format!("{e:#}").contains("process_database_connections")));
    }

    #[test]
    fn a_process_refuses_more_tenants_than_its_ceiling() {
        let lone = [tenant("a.toml", "a.example", "127.0.0.1:3000", &[], "")];
        let settings = process_settings(&lone).unwrap();
        assert_eq!(settings.max_tenants, config::DEFAULT_PROCESS_MAX_TENANTS);
        admit(&lone, &settings).unwrap();

        let ceiling = "[limits]\nprocess_max_tenants = 2";
        let three = [
            tenant("a.toml", "a.example", "127.0.0.1:3000", &[], ceiling),
            tenant("b.toml", "b.example", "127.0.0.1:3000", &[], ceiling),
            tenant("c.toml", "c.example", "127.0.0.1:3000", &[], ceiling),
        ];
        let settings = process_settings(&three).unwrap();
        let error = admit(&three, &settings)
            .err()
            .map(|e| format!("{e:#}"))
            .unwrap_or_default();
        assert!(
            error.contains("3 tenants") && error.contains("process_max_tenants"),
            "{error}"
        );
        admit(&three[..2], &settings).unwrap();
    }

    #[test]
    fn a_process_refuses_pools_past_the_budget_it_was_given() {
        let limits = |budget: u64| {
            format!(
                "[limits]\nprocess_database_connections = {budget}\n\
                 [database_pool]\nmax_connections = 20"
            )
        };
        let pair = |budget| {
            [
                tenant(
                    "a.toml",
                    "a.example",
                    "127.0.0.1:3000",
                    &[],
                    &limits(budget),
                ),
                tenant(
                    "b.toml",
                    "b.example",
                    "127.0.0.1:3000",
                    &[],
                    &limits(budget),
                ),
            ]
        };

        let over = pair(30);
        let error = admit(&over, &process_settings(&over).unwrap())
            .err()
            .map(|e| format!("{e:#}"))
            .unwrap_or_default();
        assert!(
            error.contains("40 connections") && error.contains("given 30"),
            "{error}"
        );

        let exact = pair(40);
        admit(&exact, &process_settings(&exact).unwrap()).unwrap();
    }

    #[test]
    fn pools_are_counted_against_the_server_they_connect_to() {
        let pool = "[database_pool]\nmax_connections = 20";
        let mut configs = [
            tenant("a.toml", "a.example", "127.0.0.1:3000", &[], pool),
            tenant("b.toml", "b.example", "127.0.0.1:3000", &[], pool),
            tenant("c.toml", "c.example", "127.0.0.1:3000", &[], pool),
            tenant("d.toml", "d.example", "127.0.0.1:3000", &[], pool),
        ];
        configs[0].config.database_url = "postgres://eunha@db1.internal/a".into();
        configs[1].config.database_url = "postgres://eunha@db2.internal/b".into();
        configs[2].config.database_url = "postgres://other@db1.internal:5432/c".into();
        configs[3].config.database_url = "not a database url".into();

        let servers = demand_by_server(&configs);
        assert_eq!(servers.len(), 2, "d.toml cannot connect, so is not counted");
        let db1 = &servers[0];
        assert_eq!(db1.server, "db1.internal:5432");
        assert_eq!(db1.connections, 40, "a role does not make another server");
        assert_eq!(db1.sources, ["a.toml", "c.toml"]);
        assert_eq!(servers[1].connections, 20);

        let error = fits(db1, 39)
            .err()
            .map(|e| format!("{e:#}"))
            .unwrap_or_default();
        assert!(
            error.contains("a.toml, c.toml") && error.contains("db1.internal:5432"),
            "{error}"
        );
        fits(db1, 40).unwrap();
    }

    #[test]
    fn no_two_tenants_may_serve_one_host() {
        let clash = [
            tenant("a.toml", "seoul.earth", "127.0.0.1:3000", &[], ""),
            tenant("b.toml", "Seoul.Earth.", "127.0.0.1:3000", &[], ""),
        ];
        let error = process_settings(&clash)
            .err()
            .map(|e| format!("{e:#}"))
            .unwrap_or_default();
        assert!(
            error.contains("a.toml") && error.contains("b.toml"),
            "{error}"
        );
    }

    #[test]
    fn a_lone_instance_has_no_request_limit_unless_it_names_one() {
        let lone = tenant("a.toml", "a.example", "127.0.0.1:3000", &[], "");
        assert_eq!(lone.config.limits.request_limit(false), None);
        assert_eq!(
            lone.config.limits.request_limit(true),
            Some(config::DEFAULT_SHARED_MAX_CONCURRENT_REQUESTS)
        );
        let named = tenant(
            "b.toml",
            "b.example",
            "127.0.0.1:3000",
            &[],
            "[limits]\nmax_concurrent_requests = 8",
        );
        assert_eq!(named.config.limits.request_limit(false), Some(8));
        assert_eq!(named.config.limits.request_limit(true), Some(8));
    }

    /// A tenant with every request slot taken sheds the next request with 503
    /// and `Retry-After`, while a neighbour answers as usual — and the slot is
    /// free again once the held request finishes.
    #[tokio::test]
    async fn a_tenant_at_its_limit_sheds_load_and_its_neighbour_does_not() {
        let gate = Arc::new(tokio::sync::Notify::new());
        let held = {
            let gate = gate.clone();
            Router::new().route(
                "/",
                axum::routing::get(move || {
                    let gate = gate.clone();
                    async move {
                        gate.notified().await;
                        "released"
                    }
                }),
            )
        };
        let calm = Router::new().route("/", axum::routing::get(|| async { "calm" }));
        let busy_tenant = serving(held, Some(1));
        let busy_limit = busy_tenant.in_flight.clone().unwrap();
        let t = Arc::new(tenants(vec![
            ("busy.example", Some(busy_tenant)),
            ("calm.example", Some(serving(calm, Some(1)))),
        ]));

        let first = {
            let t = t.clone();
            spawn(async move { t.dispatch(request("busy.example")).await })
        };
        for _ in 0..200 {
            if busy_limit.available_permits() == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(
            busy_limit.available_permits(),
            0,
            "the first request holds the slot"
        );

        let shed = t.dispatch(request("busy.example")).await;
        assert_eq!(shed.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(shed.headers()[header::RETRY_AFTER], "1");

        let neighbour = t.dispatch(request("calm.example")).await;
        assert_eq!(
            neighbour.status(),
            StatusCode::OK,
            "the neighbour is not held up"
        );

        gate.notify_one();
        assert_eq!(first.await.unwrap().status(), StatusCode::OK);
        assert_eq!(busy_limit.available_permits(), 1, "the slot is released");

        gate.notify_one();
        let again = t.dispatch(request("busy.example")).await;
        assert_eq!(
            again.status(),
            StatusCode::OK,
            "and the tenant serves again"
        );
    }

    /// Log output kept in memory, to read back what was logged and in which
    /// span.
    #[derive(Clone, Default)]
    struct Captured(Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Captured {
        fn line_with(&self, needle: &str) -> String {
            String::from_utf8_lossy(&self.0.lock().unwrap())
                .lines()
                .find(|line| line.contains(needle))
                .unwrap_or_else(|| panic!("nothing logged {needle:?}"))
                .to_string()
        }
    }

    /// Work a tenant hands to a task or to the blocking pool still logs as that
    /// tenant, where a task Tokio starts directly logs as nobody — which is
    /// why `clippy.toml` refuses the direct form.
    #[tokio::test]
    async fn spawned_work_logs_as_the_tenant_that_spawned_it() {
        use tracing::Instrument as _;

        let logs = Captured::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .with_writer({
                let logs = logs.clone();
                move || logs.clone()
            })
            .finish();
        let _default = tracing::subscriber::set_default(subscriber);

        async {
            spawn(async { tracing::warn!("from a task") })
                .await
                .unwrap();
            spawn_blocking(|| tracing::warn!("from the blocking pool"))
                .await
                .unwrap();
            #[allow(clippy::disallowed_methods)]
            let bare = tokio::spawn(async { tracing::warn!("from a bare task") });
            bare.await.unwrap();
        }
        .instrument(span("a.example"))
        .await;

        for message in ["from a task", "from the blocking pool"] {
            let line = logs.line_with(message);
            assert!(line.contains("tenant{domain=a.example}"), "{line}");
        }
        let bare = logs.line_with("from a bare task");
        assert!(!bare.contains("tenant{"), "{bare}");
    }
}
