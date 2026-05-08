/// Mastodon REST API serialization types.
/// Reference: https://docs.joinmastodon.org/entities/
use serde::{Deserialize, Serialize};

// ── Account ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Account {
    pub id: String,
    pub username: String,
    pub acct: String,
    pub display_name: String,
    pub locked: bool,
    pub bot: bool,
    pub discoverable: Option<bool>,
    pub indexable: bool,
    pub created_at: String,         // ISO 8601 date (midnight UTC, day precision)
    pub note: String,               // HTML
    pub url: String,
    pub uri: String,
    pub avatar: String,
    pub avatar_static: String,
    pub header: String,
    pub header_static: String,
    pub followers_count: i64,
    pub following_count: i64,
    pub statuses_count: i64,
    pub last_status_at: Option<String>,
    pub emojis: Vec<CustomEmoji>,
    pub fields: Vec<Field>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moved: Option<Box<Account>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<AccountSource>,  // only on CredentialAccount
}

#[derive(Debug, Serialize)]
pub struct AccountSource {
    pub privacy: String,
    pub sensitive: bool,
    pub language: Option<String>,
    pub note: String,               // plain text
    pub fields: Vec<Field>,
    pub follow_requests_count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub value: String,
    pub verified_at: Option<String>,
}

// ── Status ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Status {
    pub id: String,
    pub created_at: String,
    pub in_reply_to_id: Option<String>,
    pub in_reply_to_account_id: Option<String>,
    pub sensitive: bool,
    pub spoiler_text: String,
    pub visibility: String,
    pub language: Option<String>,
    pub uri: String,
    pub url: Option<String>,
    pub replies_count: i64,
    pub reblogs_count: i64,
    pub favourites_count: i64,
    pub edited_at: Option<String>,
    pub content: String,
    pub reblog: Option<Box<Status>>,
    pub application: Option<Application>,
    pub account: Account,
    pub media_attachments: Vec<MediaAttachment>,
    pub mentions: Vec<StatusMention>,
    pub tags: Vec<StatusTag>,
    pub emojis: Vec<CustomEmoji>,
    pub card: Option<PreviewCard>,
    pub poll: Option<Poll>,
    // Viewer-specific (None when not authenticated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favourited: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reblogged: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bookmarked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filtered: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Serialize)]
pub struct StatusMention {
    pub id: String,
    pub username: String,
    pub acct: String,
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct StatusTag {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct Application {
    pub name: String,
    pub website: Option<String>,
}

// ── Media ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct MediaAttachment {
    pub id: String,
    #[serde(rename = "type")]
    pub media_type: String,
    pub url: Option<String>,
    pub preview_url: Option<String>,
    pub remote_url: Option<String>,
    pub description: Option<String>,
    pub blurhash: Option<String>,
    pub meta: Option<serde_json::Value>,
}

// ── Instance ───────────────────────────────────────────────────────────────

/// Mastodon v2 Instance entity
#[derive(Debug, Serialize)]
pub struct InstanceV2 {
    pub domain: String,
    pub title: String,
    pub version: String,
    pub source_url: String,
    pub description: String,
    pub usage: InstanceUsage,
    pub thumbnail: InstanceThumbnail,
    pub languages: Vec<String>,
    pub configuration: InstanceConfiguration,
    pub registrations: InstanceRegistrations,
    pub contact: InstanceContact,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Serialize)]
pub struct InstanceUsage {
    pub users: InstanceUsageUsers,
}

#[derive(Debug, Serialize)]
pub struct InstanceUsageUsers {
    pub active_month: i64,
}

#[derive(Debug, Serialize)]
pub struct InstanceThumbnail {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blurhash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub versions: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct InstanceConfiguration {
    pub urls: InstanceUrls,
    pub accounts: AccountsConfiguration,
    pub statuses: StatusesConfiguration,
    pub media_attachments: MediaConfiguration,
    pub polls: PollsConfiguration,
    pub translation: TranslationConfiguration,
}

#[derive(Debug, Serialize)]
pub struct InstanceUrls {
    pub streaming: String,
}

#[derive(Debug, Serialize)]
pub struct AccountsConfiguration {
    pub max_featured_tags: u32,
    pub max_pinned_statuses: u32,
}

#[derive(Debug, Serialize)]
pub struct StatusesConfiguration {
    pub max_characters: u32,
    pub max_media_attachments: u32,
    pub characters_reserved_per_url: u32,
}

#[derive(Debug, Serialize)]
pub struct MediaConfiguration {
    pub supported_mime_types: Vec<String>,
    pub image_size_limit: u64,
    pub image_matrix_limit: u64,
    pub video_size_limit: u64,
    pub video_frame_rate_limit: u64,
    pub video_matrix_limit: u64,
}

#[derive(Debug, Serialize)]
pub struct PollsConfiguration {
    pub max_options: u32,
    pub max_characters_per_option: u32,
    pub min_expiration: u64,
    pub max_expiration: u64,
}

#[derive(Debug, Serialize)]
pub struct TranslationConfiguration {
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct InstanceRegistrations {
    pub enabled: bool,
    pub approval_required: bool,
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InstanceContact {
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<Account>,
}

#[derive(Debug, Serialize)]
pub struct Rule {
    pub id: String,
    pub text: String,
    pub hint: String,
}

// ── Notification ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Notification {
    pub id: String,
    #[serde(rename = "type")]
    pub notification_type: String,
    pub created_at: String,
    pub account: Account,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
}

// ── OAuth ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CredentialApplication {
    pub id: String,
    pub name: String,
    pub website: Option<String>,
    pub scopes: Vec<String>,
    pub redirect_uris: Vec<String>,
    pub client_id: String,
    pub client_secret: String,
    pub vapid_key: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Token {
    pub access_token: String,
    pub token_type: String,
    pub scope: String,
    pub created_at: i64,
}

// ── Emoji ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CustomEmoji {
    pub shortcode: String,
    pub url: String,
    pub static_url: String,
    pub visible_in_picker: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

// ── Poll ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Poll {
    pub id: String,
    pub expires_at: Option<String>,
    pub expired: bool,
    pub multiple: bool,
    pub votes_count: i64,
    pub voters_count: Option<i64>,
    pub options: Vec<PollOption>,
    pub emojis: Vec<CustomEmoji>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub own_votes: Option<Vec<i32>>,
}

#[derive(Debug, Serialize)]
pub struct PollOption {
    pub title: String,
    pub votes_count: Option<i64>,
}

// ── Preview Card ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PreviewCard {
    pub url: String,
    pub title: String,
    pub description: String,
    #[serde(rename = "type")]
    pub card_type: String,
    pub author_name: String,
    pub author_url: String,
    pub provider_name: String,
    pub provider_url: String,
    pub html: String,
    pub width: i32,
    pub height: i32,
    pub image: Option<String>,
    pub embed_url: String,
    pub blurhash: Option<String>,
}

// ── Relationship ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Relationship {
    pub id: String,
    pub following: bool,
    pub showing_reblogs: bool,
    pub notifying: bool,
    pub languages: Vec<String>,
    pub followed_by: bool,
    pub blocking: bool,
    pub blocked_by: bool,
    pub muting: bool,
    pub muting_notifications: bool,
    pub requested: bool,
    pub requested_by: bool,
    pub domain_blocking: bool,
    pub endorsed: bool,
    pub note: String,
}

// ── Search ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SearchResults {
    pub accounts: Vec<Account>,
    pub statuses: Vec<Status>,
    pub hashtags: Vec<Tag>,
}

#[derive(Debug, Serialize)]
pub struct Tag {
    pub name: String,
    pub url: String,
    pub history: Vec<TagHistory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub following: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct TagHistory {
    pub day: String,
    pub accounts: String,
    pub uses: String,
}

// ── Pagination ─────────────────────────────────────────────────────────────

/// Query parameters used by timeline/list endpoints.
#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub max_id: Option<String>,
    pub since_id: Option<String>,
    pub min_id: Option<String>,
    pub limit: Option<String>,
}

impl PaginationParams {
    pub fn limit_clamped(&self, default: i64, max: i64) -> i64 {
        self.limit.as_deref()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(default)
            .min(max)
            .max(1)
    }
}
