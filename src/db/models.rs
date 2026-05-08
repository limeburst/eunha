use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct Instance {
    pub id: Uuid,
    pub domain: String,
    pub title: String,
    pub description: String,
    pub short_description: String,
    pub contact_email: Option<String>,
    pub registrations_open: bool,
    pub approval_required: bool,
    pub private_key: String,
    pub public_key: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct Account {
    pub id: Uuid,
    pub instance_id: Uuid,
    pub username: String,
    pub domain: Option<String>,
    pub display_name: String,
    pub note: String,
    pub note_text: String,
    pub url: String,
    pub uri: String,
    pub avatar: Option<String>,
    pub avatar_static: Option<String>,
    pub header: Option<String>,
    pub header_static: Option<String>,
    pub private_key: Option<String>,
    pub public_key: String,
    pub followers_count: i64,
    pub following_count: i64,
    pub statuses_count: i64,
    pub locked: bool,
    pub bot: bool,
    pub discoverable: bool,
    pub indexable: bool,
    pub moved_to_uri: Option<String>,
    pub inbox_url: String,
    pub outbox_url: String,
    pub shared_inbox_url: Option<String>,
    pub suspended_at: Option<DateTime<Utc>>,
    pub silenced_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Account {
    pub fn is_local(&self) -> bool {
        self.domain.is_none()
    }

    pub fn acct(&self) -> String {
        match &self.domain {
            None => self.username.clone(),
            Some(d) => format!("{}@{}", self.username, d),
        }
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct User {
    pub id: Uuid,
    pub account_id: Uuid,
    pub instance_id: Uuid,
    pub email: String,
    pub email_normalized: String,
    pub password_hash: String,
    pub confirmed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct Status {
    pub id: i64,
    pub instance_id: Uuid,
    pub account_id: Uuid,
    pub text: String,
    pub content: String,
    pub spoiler_text: String,
    pub in_reply_to_id: Option<i64>,
    pub in_reply_to_account_id: Option<Uuid>,
    pub reblog_of_id: Option<i64>,
    pub visibility: String,
    pub language: Option<String>,
    pub sensitive: bool,
    pub url: Option<String>,
    pub uri: Option<String>,
    pub replies_count: i64,
    pub reblogs_count: i64,
    pub favourites_count: i64,
    pub deleted_at: Option<DateTime<Utc>>,
    pub edited_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct MediaAttachment {
    pub id: i64,
    pub account_id: Uuid,
    pub status_id: Option<i64>,
    pub media_type: String,
    pub file_key: Option<String>,
    pub file_url: Option<String>,
    pub preview_key: Option<String>,
    pub preview_url: Option<String>,
    pub remote_url: Option<String>,
    pub description: Option<String>,
    pub blurhash: Option<String>,
    pub meta: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct Follow {
    pub id: Uuid,
    pub account_id: Uuid,
    pub target_account_id: Uuid,
    pub state: String,
    pub uri: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct Notification {
    pub id: i64,
    pub account_id: Uuid,
    pub from_account_id: Uuid,
    pub notification_type: String,
    pub status_id: Option<i64>,
    pub read: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct OauthApplication {
    pub id: Uuid,
    pub instance_id: Uuid,
    pub name: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uris: String,
    pub scopes: String,
    pub website: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct OauthAccessToken {
    pub id: Uuid,
    pub application_id: Option<Uuid>,
    pub account_id: Option<Uuid>,
    pub token: String,
    pub refresh_token: Option<String>,
    pub scopes: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct Tag {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct CustomEmoji {
    pub id: Uuid,
    pub instance_id: Uuid,
    pub shortcode: String,
    pub domain: Option<String>,
    pub image_url: String,
    pub static_image_url: Option<String>,
    pub visible_in_picker: bool,
    pub disabled: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct Favourite {
    pub id: Uuid,
    pub account_id: Uuid,
    pub status_id: i64,
    pub uri: Option<String>,
    pub created_at: DateTime<Utc>,
}
