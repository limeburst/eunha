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

    /// Store `data` at the given `key` and return the key.
    pub async fn store(&self, data: &[u8], key: &str, content_type: &str) -> AppResult<String> {
        match &self.inner {
            Inner::Local { base_path, .. } => {
                let full_path = PathBuf::from(base_path).join(key);
                std::fs::create_dir_all(full_path.parent().unwrap())
                    .map_err(|e| AppError::Internal(e.into()))?;
                std::fs::write(&full_path, data)
                    .map_err(|e| AppError::Internal(e.into()))?;
            }
            Inner::S3 { client, bucket, .. } => {
                client.put_object()
                    .bucket(bucket)
                    .key(key)
                    .body(data.to_vec().into())
                    .content_type(content_type)
                    .send()
                    .await
                    .map_err(|e| AppError::Internal(anyhow::anyhow!("S3 upload: {}", e)))?;
            }
        }
        Ok(key.to_string())
    }

    pub fn public_url(&self, key: &str) -> String {
        let base_url = match &self.inner {
            Inner::Local { base_url, .. } => base_url,
            Inner::S3 { base_url, .. } => base_url,
        };
        format!("{}/{}", base_url.trim_end_matches('/'), key)
    }
}

// ── Key generation ────────────────────────────────────────────────────────
//
// Paths mirror Mastodon's layout, namespaced by instance UUID (not domain,
// which can change with custom domains).

/// `{instance_id}/accounts/avatars/{account_id_parts}/original/{random}.{ext}`
pub fn account_avatar_key(instance_id: Uuid, account_id: Uuid, content_type: &str) -> String {
    let ext = ext_for(content_type);
    format!(
        "{}/accounts/avatars/{}/original/{}.{}",
        uuid_to_path(instance_id),
        uuid_to_path(account_id),
        random_hex(),
        ext,
    )
}

/// `{instance_id}/accounts/headers/{account_id_parts}/original/{random}.{ext}`
pub fn account_header_key(instance_id: Uuid, account_id: Uuid, content_type: &str) -> String {
    let ext = ext_for(content_type);
    format!(
        "{}/accounts/headers/{}/original/{}.{}",
        uuid_to_path(instance_id),
        uuid_to_path(account_id),
        random_hex(),
        ext,
    )
}

/// `{instance_id}/media_attachments/files/{file_uuid_parts}/original/{random}.{ext}`
///
/// Uses a fresh UUID as the file identifier since the attachment DB row
/// doesn't exist yet at upload time.
pub fn media_attachment_key(instance_id: Uuid, content_type: &str) -> String {
    let ext = ext_for(content_type);
    format!(
        "{}/media_attachments/files/{}/original/{}.{}",
        uuid_to_path(instance_id),
        uuid_to_path(Uuid::new_v4()),
        random_hex(),
        ext,
    )
}

/// Split a UUID's hex digits (no hyphens) into path segments of 3 chars each.
/// e.g. `bc85c358-5bd2-...` → `bc8/5c3/585/bd2/...`
fn uuid_to_path(id: Uuid) -> String {
    let hex = id.simple().to_string(); // no hyphens
    hex.as_bytes()
        .chunks(3)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("/")
}

fn random_hex() -> String {
    let bytes = Uuid::new_v4().into_bytes();
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn ext_for(content_type: &str) -> &'static str {
    mime_guess::get_mime_extensions_str(content_type)
        .and_then(|e| e.first().copied())
        .unwrap_or("bin")
}
