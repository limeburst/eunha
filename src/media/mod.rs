use std::path::PathBuf;
use uuid::Uuid;

use crate::{config::MediaStorageConfig, error::{AppError, AppResult}};

pub struct Storage {
    config: MediaStorageConfig,
}

impl Storage {
    pub fn from_config(config: &MediaStorageConfig) -> Self {
        Self { config: config.clone() }
    }

    pub async fn store(&self, data: &[u8], original_name: &str, content_type: &str) -> AppResult<String> {
        let ext = mime_guess::get_mime_extensions_str(content_type)
            .and_then(|e| e.first().copied())
            .unwrap_or("bin");

        let key = format!(
            "media/{}/{}.{}",
            chrono::Utc::now().format("%Y/%m/%d"),
            Uuid::new_v4(),
            ext
        );

        match &self.config {
            MediaStorageConfig::Local { base_path, .. } => {
                let full_path = PathBuf::from(base_path).join(&key);
                std::fs::create_dir_all(full_path.parent().unwrap())
                    .map_err(|e| AppError::Internal(e.into()))?;
                std::fs::write(&full_path, data)
                    .map_err(|e| AppError::Internal(e.into()))?;
            }
            MediaStorageConfig::S3 { .. } => {
                // TODO: implement S3 upload via aws-sdk-s3 or object_store
                return Err(AppError::Internal(anyhow::anyhow!("S3 storage not yet implemented")));
            }
        }

        Ok(key)
    }

    pub fn public_url(&self, key: &str) -> String {
        match &self.config {
            MediaStorageConfig::Local { base_url, .. } => {
                format!("{}/{}", base_url.trim_end_matches('/'), key)
            }
            MediaStorageConfig::S3 { base_url, .. } => {
                format!("{}/{}", base_url.trim_end_matches('/'), key)
            }
        }
    }
}
