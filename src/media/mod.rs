use uuid::Uuid;

use crate::{config::MediaStorageConfig, error::{AppError, AppResult}};

pub struct Storage {
    client: aws_sdk_s3::Client,
    bucket: String,
    base_url: String,
}

impl Storage {
    pub async fn from_config(config: &MediaStorageConfig) -> Self {
        let creds = aws_sdk_s3::config::Credentials::new(
            &config.access_key_id,
            &config.secret_access_key,
            None,
            None,
            "static",
        );
        let mut builder = aws_sdk_s3::config::Builder::new()
            .region(aws_sdk_s3::config::Region::new(config.region.clone()))
            .credentials_provider(creds)
            .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest());
        if let Some(ep) = &config.endpoint {
            builder = builder.endpoint_url(ep);
        }
        let client = aws_sdk_s3::Client::from_conf(builder.build());
        Storage {
            client,
            bucket: config.bucket.clone(),
            base_url: config.base_url.clone(),
        }
    }

    pub async fn store(&self, data: &[u8], key: &str, content_type: &str) -> AppResult<String> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(data.to_vec().into())
            .content_type(content_type)
            .send()
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("S3 upload: {}", e)))?;
        Ok(key.to_string())
    }

    pub fn public_url(&self, key: &str) -> String {
        format!("{}/{}", self.base_url.trim_end_matches('/'), key)
    }
}

// ── Key generation ────────────────────────────────────────────────────────

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

fn uuid_to_path(id: Uuid) -> String {
    let hex = id.simple().to_string();
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
