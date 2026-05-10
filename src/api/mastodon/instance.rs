use axum::{extract::Extension, Json};
use crate::{
    error::AppResult,
    middleware::ResolvedInstance,
};
use super::types::*;

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
        rules: vec![],
    }))
}

pub async fn get_instance_v2(
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
) -> AppResult<Json<InstanceV2>> {
    let streaming_url = format!("wss://{}/api/v1/streaming", instance.domain);

    Ok(Json(InstanceV2 {
        domain: instance.domain.clone(),
        title: instance.title.clone(),
        version: "0.0.1".to_string(),
        source_url: "https://github.com/limeburst/eunha".to_string(),
        description: instance.description.clone(),
        usage: InstanceUsage {
            users: InstanceUsageUsers { active_month: 0 },
        },
        thumbnail: InstanceThumbnail {
            url: format!("https://{}/instance-thumbnail.png", instance.domain),
            blurhash: None,
            versions: None,
        },
        languages: vec!["en".to_string()],
        configuration: InstanceConfiguration {
            urls: InstanceUrls { streaming: streaming_url },
            accounts: AccountsConfiguration {
                max_featured_tags: 10,
                max_pinned_statuses: 5,
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
            message: None,
            url: if instance.registrations_open {
                let domain = instance.custom_domain.as_deref().unwrap_or(&instance.domain);
                Some(format!("https://{}/auth/signup", domain))
            } else {
                None
            },
        },
        contact: InstanceContact {
            email: instance.contact_email.clone().unwrap_or_default(),
            account: None,
        },
        rules: vec![],
    }))
}
