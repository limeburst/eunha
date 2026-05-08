use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub bind_address: String,
    pub console_domain: String,
    pub media_storage: MediaStorageConfig,
    pub smtp: Option<SmtpConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaStorageConfig {
    Local { base_path: String, base_url: String },
    S3 {
        bucket: String,
        region: String,
        endpoint: Option<String>,
        access_key_id: String,
        secret_access_key: String,
        base_url: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        let cfg = config::Config::builder()
            .add_source(config::Environment::default().separator("__"))
            .build()?;
        Ok(cfg.try_deserialize()?)
    }
}
