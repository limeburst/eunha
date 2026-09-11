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
//! What is per process is what the listener and the SSRF-guarded resolver
//! impose: the address to listen on, and the private networks federation may
//! reach. Tenants in one directory must agree on those, and the process refuses
//! to start when they do not.

use crate::{config, migrate, state::AppState};
use anyhow::{Context as _, Result};
use axum::{
    extract::Request,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Router,
};
use futures::StreamExt as _;
use sqlx::{postgres::PgPoolOptions, Executor as _};
use std::{collections::HashMap, path::Path, sync::Arc};
use tower::ServiceExt as _;

/// How many tenants are brought up at once.
const STARTUP_CONCURRENCY: usize = 8;

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

/// What belongs to the process rather than to any one tenant, which every
/// tenant therefore has to agree on — and no two tenants may serve one host.
/// Returns the address to listen on.
fn process_settings(configs: &[TenantConfig]) -> Result<String> {
    let first = configs.first().context("no tenants to serve")?;
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
        let host = normalize_host(&tenant.config.instance.domain);
        if let Some(other) = hosts.insert(host.clone(), &tenant.source) {
            anyhow::bail!("{other} and {} both serve {host}", tenant.source);
        }
    }
    Ok(first.config.bind_address.clone())
}

/// One tenant's place in the registry.
enum Slot {
    Serving(Router),
    /// It did not start: its database is behind this binary, or it failed.
    Unavailable,
}

/// Every tenant a process serves, by host.
pub struct Tenants {
    by_host: HashMap<String, Slot>,
    /// The only tenant, which answers every host, as a lone instance always has.
    only: Option<Router>,
    bind_address: String,
    states: Vec<AppState>,
}

enum Lookup<'a> {
    Serving(&'a Router),
    Unavailable,
    Unknown,
}

/// Bring every tenant up: its pool, the migration check, its state, the move of
/// its signing keys, its background tasks and its router.
///
/// A lone tenant whose database is behind this binary, or that cannot start,
/// stops the process, as a single instance always has. Among several it is left
/// out — its host answers 503 — and the others serve: one tenant's pending
/// migration should not take its neighbours down with it.
pub async fn start(configs: Vec<TenantConfig>) -> Result<Tenants> {
    let bind_address = process_settings(&configs)?;
    let lone = configs.len() == 1;
    let mut by_host = HashMap::new();
    let mut states = Vec::new();
    let mut only = None;

    let mut started = futures::stream::iter(configs)
        .map(|tenant| async move {
            let host = normalize_host(&tenant.config.instance.domain);
            let source = tenant.source.clone();
            (host, source, start_one(tenant).await)
        })
        .buffered(STARTUP_CONCURRENCY);
    while let Some((host, source, result)) = started.next().await {
        match result {
            Ok((state, router)) => {
                if lone {
                    only = Some(router.clone());
                }
                by_host.insert(host, Slot::Serving(router));
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
        bind_address,
        states,
    })
}

async fn start_one(tenant: TenantConfig) -> Result<(AppState, Router)> {
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
    let router = crate::build_app(state.clone());
    Ok((state, router))
}

impl Tenants {
    /// A registry of tenants whose states are already built, served on
    /// `bind_address` — for embedding eunha and for tests, which bring their own
    /// databases up. Nothing is connected, checked or spawned here.
    pub fn from_states(bind_address: &str, states: Vec<AppState>) -> Result<Self> {
        anyhow::ensure!(!states.is_empty(), "no tenants to serve");
        let mut by_host = HashMap::new();
        for state in &states {
            let host = normalize_host(&state.instance.domain);
            let router = crate::build_app(state.clone());
            anyhow::ensure!(
                by_host
                    .insert(host.clone(), Slot::Serving(router))
                    .is_none(),
                "two tenants both serve {host}"
            );
        }
        let only = match (states.len(), by_host.values().next()) {
            (1, Some(Slot::Serving(router))) => Some(router.clone()),
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
        if let Some(router) = &self.only {
            return Lookup::Serving(router);
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
            Some(Slot::Serving(router)) => Lookup::Serving(router),
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
            Lookup::Serving(router) => match router.clone().oneshot(req).await {
                Ok(response) => response,
                Err(never) => match never {},
            },
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(source: &str, domain: &str, bind: &str, allowed: &[&str]) -> TenantConfig {
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
"#
        ))
        .expect("test configuration parses");
        TenantConfig {
            source: source.to_string(),
            config,
        }
    }

    fn tenants(hosts: &[(&str, bool)]) -> Tenants {
        Tenants {
            by_host: hosts
                .iter()
                .map(|(host, serving)| {
                    let slot = if *serving {
                        Slot::Serving(Router::new())
                    } else {
                        Slot::Unavailable
                    };
                    (host.to_string(), slot)
                })
                .collect(),
            only: None,
            bind_address: String::new(),
            states: Vec::new(),
        }
    }

    #[test]
    fn hosts_are_matched_without_case_a_trailing_dot_or_a_port() {
        let t = tenants(&[("seoul.earth", true)]);
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
        let t = tenants(&[("seoul.earth", true), ("example.social", false)]);
        assert!(matches!(t.lookup("example.social"), Lookup::Unavailable));
    }

    #[test]
    fn a_lone_tenant_answers_every_host() {
        let mut t = tenants(&[("seoul.earth", true)]);
        t.only = Some(Router::new());
        assert!(matches!(t.lookup("localhost:3000"), Lookup::Serving(_)));
        assert!(matches!(t.lookup(""), Lookup::Serving(_)));
    }

    #[test]
    fn tenants_must_agree_on_what_the_process_owns() {
        let agreeing = [
            tenant("a.toml", "a.example", "127.0.0.1:3000", &[]),
            tenant("b.toml", "b.example", "127.0.0.1:3000", &[]),
        ];
        assert_eq!(process_settings(&agreeing).unwrap(), "127.0.0.1:3000");

        let listeners = [
            tenant("a.toml", "a.example", "127.0.0.1:3000", &[]),
            tenant("b.toml", "b.example", "127.0.0.1:3001", &[]),
        ];
        assert!(format!("{:#}", process_settings(&listeners).unwrap_err()).contains("bind_address"));

        let networks = [
            tenant("a.toml", "a.example", "127.0.0.1:3000", &[]),
            tenant("b.toml", "b.example", "127.0.0.1:3000", &["10.0.0.0/8"]),
        ];
        assert!(format!("{:#}", process_settings(&networks).unwrap_err())
            .contains("allowed_private_networks"));
    }

    #[test]
    fn no_two_tenants_may_serve_one_host() {
        let clash = [
            tenant("a.toml", "seoul.earth", "127.0.0.1:3000", &[]),
            tenant("b.toml", "Seoul.Earth.", "127.0.0.1:3000", &[]),
        ];
        let error = format!("{:#}", process_settings(&clash).unwrap_err());
        assert!(
            error.contains("a.toml") && error.contains("b.toml"),
            "{error}"
        );
    }
}
