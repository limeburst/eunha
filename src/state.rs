use sqlx::PgPool;
use std::sync::Arc;
use crate::config::Config;
use crate::streaming::StreamBus;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub http: reqwest::Client,
    pub streaming: StreamBus,
}

impl AppState {
    pub fn new(db: PgPool, config: Config) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("eunha/", env!("CARGO_PKG_VERSION"), " (ActivityPub)"))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");

        Self {
            db,
            config: Arc::new(config),
            http,
            streaming: StreamBus::new(),
        }
    }
}
