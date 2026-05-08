use std::path::PathBuf;
use uuid::Uuid;

use crate::{config::MediaStorageConfig, error::{AppError, AppResult}};

pub struct Storage {
    inner: Inner,
}

enum Inner {
    Local { base_path: String, base_url: String },
    S3 { client: aws_sdk_s3::Client, bucket: String, base_url: String },
}

impl Storage {
    pub async fn from_config(config: &MediaStorageConfig) -> Self {
        match config {
            MediaStorageConfig::Local { base_path, base_url } => Storage {
                inner: Inner::Local {
                    base_path: base_path.clone(),
                    base_url: base_url.clone(),
                },
            },
            MediaStorageConfig::S3 { bucket, region, endpoint, access_key_id, secret_access_key, base_url } => {
                let creds = aws_sdk_s3::config::Credentials::new(
                    access_key_id, secret_access_key, None, None, "static",
                );
                let mut builder = aws_sdk_s3::config::Builder::new()
                    .region(aws_sdk_s3::config::Region::new(region.clone()))
                    .credentials_provider(creds)
                    .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest());
                if let Some(ep) = endpoint {
                    builder = builder.endpoint_url(ep);
                }
                let client = aws_sdk_s3::Client::from_conf(builder.build());
                Storage {
                    inner: Inner::S3 {
                        client,
                        bucket: bucket.clone(),
                        base_url: base_url.clone(),
                    },
                }
            }
        }
    }

    pub async fn store(&self, data: &[u8], _original_name: &str, content_type: &str, instance_domain: &str) -> AppResult<String> {
        let ext = mime_guess::get_mime_extensions_str(content_type)
            .and_then(|e| e.first().copied())
            .unwrap_or("bin");

        let key = format!(
            "{}/media/{}/{}.{}",
            instance_domain.trim_matches('/'),
            chrono::Utc::now().format("%Y/%m/%d"),
            Uuid::new_v4(),
            ext
        );

        match &self.inner {
            Inner::Local { base_path, .. } => {
                let full_path = PathBuf::from(base_path).join(&key);
                std::fs::create_dir_all(full_path.parent().unwrap())
                    .map_err(|e| AppError::Internal(e.into()))?;
                std::fs::write(&full_path, data)
                    .map_err(|e| AppError::Internal(e.into()))?;
            }
            Inner::S3 { client, bucket, .. } => {
                client.put_object()
                    .bucket(bucket)
                    .key(&key)
                    .body(data.to_vec().into())
                    .content_type(content_type)
                    .send()
                    .await
                    .map_err(|e| AppError::Internal(anyhow::anyhow!("S3 upload: {}", e)))?;
            }
        }

        Ok(key)
    }

    pub fn public_url(&self, key: &str) -> String {
        let base_url = match &self.inner {
            Inner::Local { base_url, .. } => base_url,
            Inner::S3 { base_url, .. } => base_url,
        };
        format!("{}/{}", base_url.trim_end_matches('/'), key)
    }
}

