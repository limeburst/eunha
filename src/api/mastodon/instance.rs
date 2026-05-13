use axum::{extract::Extension, Json};
use crate::{
    error::AppResult,
    middleware::ResolvedInstance,
};
use super::types::*;

// ── GET /api/v1/instance/translation_languages ───────────────────────────
// Returns empty object — translation is not supported.

pub async fn get_translation_languages() -> Json<serde_json::Value> {
    Json(serde_json::json!({}))
}

// ── GET /api/v1/instance/privacy_policy ──────────────────────────────────

pub async fn get_privacy_policy(
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
) -> AppResult<Json<ExtendedDescription>> {
    Ok(Json(ExtendedDescription {
        updated_at: instance.updated_at.to_rfc3339(),
        content: instance.privacy_policy.clone(),
    }))
}

// ── GET /api/v1/instance/extended_description ────────────────────────────

pub async fn get_extended_description(
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
) -> AppResult<Json<ExtendedDescription>> {
    Ok(Json(ExtendedDescription {
        updated_at: instance.updated_at.to_rfc3339(),
        content: instance.description.clone(),
    }))
}

// ── GET /api/v1/instance ──────────────────────────────────────────────────

pub async fn get_instance_v1(
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
) -> AppResult<Json<InstanceV1>> {
    let streaming_url = format!("wss://{}/api/v1/streaming", instance.domain);

    Ok(Json(InstanceV1 {
        uri: instance.domain.clone(),
        title: instance.title.clone(),
        short_description: instance.short_description.clone(),
        description: instance.description.clone(),
        email: instance.contact_email.clone().unwrap_or_default(),
        version: "0.0.1".to_string(),
        urls: InstanceV1Urls { streaming_api: streaming_url },
        stats: InstanceV1Stats {
            user_count: 0,
            status_count: 0,
            domain_count: 0,
        },
        languages: vec!["en".to_string()],
        contact_account: None,
        rules: instance.rules.as_array()
            .map(|arr| arr.iter().enumerate().map(|(i, r)| Rule {
                id: (i + 1).to_string(),
                text: r.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                hint: r.get("hint").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            }).collect())
            .unwrap_or_default(),
    }))
}

pub async fn get_instance_v2(
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
) -> AppResult<Json<InstanceV2>> {
    let streaming_url = format!("wss://{}/api/v1/streaming", instance.domain);
    let base_url = format!("https://{}", instance.domain);

    Ok(Json(InstanceV2 {
        domain: instance.domain.clone(),
        title: instance.title.clone(),
        version: "0.0.1 (compatible; Mastodon 4.3.0)".to_string(),
        source_url: "https://github.com/limeburst/eunha".to_string(),
        description: instance.description.clone(),
        usage: InstanceUsage {
            users: InstanceUsageUsers { active_month: 0 },
        },
        thumbnail: InstanceThumbnail {
            url: instance.icon_url.clone().unwrap_or_else(|| format!("{base_url}/instance-thumbnail.png")),
            blurhash: None,
            versions: None,
        },
        icon: instance.icon_url.as_ref().map(|url| {
            vec![serde_json::json!({ "src": url })]
        }).unwrap_or_default(),
        languages: vec!["en".to_string()],
        configuration: InstanceConfiguration {
            urls: InstanceUrls {
                streaming: streaming_url,
                status: None,
                about: Some(format!("{base_url}/about")),
                privacy_policy: if instance.privacy_policy.is_empty() { None } else { Some(format!("{base_url}/api/v1/instance/privacy_policy")) },
                terms_of_service: None,
            },
            vapid: VapidConfiguration { public_key: instance.vapid_public_key.clone() },
            accounts: AccountsConfiguration {
                max_featured_tags: 10,
                max_pinned_statuses: 5,
                max_profile_fields: 4,
                max_display_name_length: 30,
                max_note_length: 500,
                max_avatar_description_length: 1500,
                max_header_description_length: 1500,
            },
            statuses: StatusesConfiguration {
                max_characters: 500,
                max_media_attachments: 4,
                characters_reserved_per_url: 23,
            },
            media_attachments: MediaConfiguration {
                supported_mime_types: vec![
                    "image/jpeg".into(),
                    "image/png".into(),
                    "image/gif".into(),
                    "image/heic".into(),
                    "image/heif".into(),
                    "image/webp".into(),
                    "image/avif".into(),
                    "video/webm".into(),
                    "video/mp4".into(),
                    "video/quicktime".into(),
                    "video/ogg".into(),
                    "audio/wave".into(),
                    "audio/wav".into(),
                    "audio/x-wav".into(),
                    "audio/x-pn-wave".into(),
                    "audio/vnd.wave".into(),
                    "audio/ogg".into(),
                    "audio/vorbis".into(),
                    "audio/mpeg".into(),
                    "audio/mp3".into(),
                    "audio/webm".into(),
                    "audio/flac".into(),
                    "audio/aac".into(),
                    "audio/m4a".into(),
                    "audio/x-m4a".into(),
                    "audio/mp4".into(),
                    "audio/3gpp".into(),
                    "video/x-ms-asf".into(),
                ],
                description_limit: 1500,
                image_size_limit: 16 * 1024 * 1024,
                image_matrix_limit: 33_177_600,
                video_size_limit: 99 * 1024 * 1024,
                video_frame_rate_limit: 120,
                video_matrix_limit: 8_294_400,
            },
            polls: PollsConfiguration {
                max_options: 4,
                max_characters_per_option: 50,
                min_expiration: 300,
                max_expiration: 2_629_746,
            },
            translation: TranslationConfiguration { enabled: false },
        },
        registrations: InstanceRegistrations {
            enabled: instance.registrations_open,
            approval_required: instance.approval_required,
            reason_required: false,
            min_age: None,
            message: None,
            url: None,
        },
        contact: InstanceContact {
            email: instance.contact_email.clone().unwrap_or_default(),
            account: None,
        },
        rules: instance.rules.as_array()
            .map(|arr| arr.iter().enumerate().map(|(i, r)| Rule {
                id: (i + 1).to_string(),
                text: r.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                hint: r.get("hint").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            }).collect())
            .unwrap_or_default(),
        api_versions: serde_json::json!({ "mastodon": 2 }),
    }))
}
