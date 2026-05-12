/// Conversions from DB models → Mastodon API types.
use crate::db::models;
use super::types;

const DEFAULT_AVATAR: &str = "https://r2.eunha.social/avatars/original/missing.png";
const DEFAULT_HEADER: &str = "https://r2.eunha.social/headers/original/missing.png";

pub fn account_from_db(a: &models::Account) -> types::Account {
    types::Account {
        id: a.id.to_string(),
        username: a.username.clone(),
        acct: a.acct(),
        display_name: a.display_name.clone(),
        locked: a.locked,
        bot: a.bot,
        group: false,
        discoverable: Some(a.discoverable),
        indexable: a.indexable,
        hide_collections: None,
        show_featured: None,
        show_media: None,
        show_media_replies: None,
        created_at: a.created_at.format("%Y-%m-%dT00:00:00.000Z").to_string(),
        note: a.note.clone(),
        url: a.url.clone(),
        uri: a.uri.clone(),
        avatar: a.avatar.clone().unwrap_or_else(|| DEFAULT_AVATAR.to_string()),
        avatar_static: a.avatar_static.clone().unwrap_or_else(|| DEFAULT_AVATAR.to_string()),
        avatar_description: String::new(),
        header: a.header.clone().unwrap_or_else(|| DEFAULT_HEADER.to_string()),
        header_static: a.header_static.clone().unwrap_or_else(|| DEFAULT_HEADER.to_string()),
        header_description: String::new(),
        followers_count: a.followers_count,
        following_count: a.following_count,
        statuses_count: a.statuses_count,
        last_status_at: None,
        emojis: vec![],
        fields: fields_from_db(&a.fields),
        moved: None,
        suspended: if a.suspended_at.is_some() { Some(true) } else { None },
        limited: if a.silenced_at.is_some() { Some(true) } else { None },
        noindex: if a.domain.is_none() { Some(!a.indexable) } else { None },
        memorial: None,
        source: None,
    }
}

pub fn fields_from_db(fields: &serde_json::Value) -> Vec<types::Field> {
    fields
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    Some(types::Field {
                        name: f["name"].as_str()?.to_string(),
                        value: f["value"].as_str()?.to_string(),
                        verified_at: f["verified_at"].as_str().map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn media_from_db(m: &models::MediaAttachment) -> types::MediaAttachment {
    types::MediaAttachment {
        id: m.id.to_string(),
        media_type: m.media_type.clone(),
        url: m.file_url.clone(),
        preview_url: m.preview_url.clone(),
        remote_url: m.remote_url.clone(),
        description: m.description.clone(),
        blurhash: m.blurhash.clone(),
        meta: m.meta.clone(),
    }
}

pub fn status_from_db(
    s: &models::Status,
    account: &models::Account,
    media: Vec<models::MediaAttachment>,
    reblog: Option<(models::Status, models::Account, Vec<models::MediaAttachment>)>,
    viewer_context: Option<StatusViewerContext>,
) -> types::Status {
    let reblog_status = reblog.map(|(rs, ra, rm)| {
        Box::new(status_from_db(&rs, &ra, rm, None, viewer_context.clone()))
    });

    types::Status {
        id: s.id.to_string(),
        created_at: s.created_at.to_rfc3339(),
        in_reply_to_id: s.in_reply_to_id.map(|i| i.to_string()),
        in_reply_to_account_id: s.in_reply_to_account_id.map(|i| i.to_string()),
        sensitive: s.sensitive,
        spoiler_text: s.spoiler_text.clone(),
        visibility: s.visibility.clone(),
        language: s.language.clone(),
        uri: s.uri.clone().unwrap_or_else(|| s.id.to_string()),
        url: s.url.clone(),
        replies_count: s.replies_count,
        reblogs_count: s.reblogs_count,
        favourites_count: s.favourites_count,
        quotes_count: 0,
        edited_at: s.edited_at.map(|t| t.to_rfc3339()),
        content: s.content.clone(),
        reblog: reblog_status,
        application: None,
        account: account_from_db(account),
        media_attachments: media.iter().map(media_from_db).collect(),
        mentions: vec![],
        tags: vec![],
        emojis: vec![],
        card: None,
        poll: None,
        favourited: viewer_context.as_ref().map(|c| c.favourited),
        reblogged: viewer_context.as_ref().map(|c| c.reblogged),
        muted: viewer_context.as_ref().map(|c| c.muted),
        bookmarked: viewer_context.as_ref().map(|c| c.bookmarked),
        pinned: viewer_context.as_ref().map(|c| c.pinned),
        filtered: None,
        text: None,
    }
}

#[derive(Clone)]
pub struct StatusViewerContext {
    pub favourited: bool,
    pub reblogged: bool,
    pub muted: bool,
    pub bookmarked: bool,
    pub pinned: bool,
}
