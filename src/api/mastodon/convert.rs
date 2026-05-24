/// Conversions from DB models → Mastodon API types.
use crate::db::models;
use super::formatting::{mention_map_from_api, render_content};
use super::types;

pub(super) const DEFAULT_AVATAR: &str = "https://r2.eunha.social/avatars/original/missing.png";
pub(super) const DEFAULT_HEADER: &str = "https://r2.eunha.social/headers/original/missing.png";

/// Format a timestamp in the Mastodon-standard format: `YYYY-MM-DDTHH:MM:SS.mmmZ`.
/// Mastodon always uses the `Z` suffix and millisecond precision; `to_rfc3339()` produces
/// `+00:00` suffix and microsecond precision, which can confuse some clients.
pub fn mastodon_date(t: chrono::DateTime<chrono::Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

fn status_url_from_uri(uri: &str) -> Option<String> {
    let (base, rest) = uri.split_once("/users/")?;
    let (username, id) = rest.split_once("/statuses/")?;
    Some(format!("{}/@{}/{}", base, username, id))
}

pub fn account_from_db(a: &models::Account) -> types::Account {
    let (url, uri) = if a.domain.is_none() && !a.uri.is_empty() {
        // Local accounts: uri is authoritative; derive url from it
        (a.uri.replace("/users/", "/@"), a.uri.clone())
    } else {
        (a.url.clone(), a.uri.clone())
    };

    let suspended = a.suspended_at.is_some();

    types::Account {
        id: a.id.to_string(),
        username: a.username.clone(),
        acct: a.acct(),
        display_name: if suspended { String::new() } else { a.display_name.clone() },
        locked: if suspended { false } else { a.locked },
        bot: if suspended { false } else { a.bot },
        group: !suspended && a.actor_type.as_deref() == Some("Group"),
        discoverable: if suspended { Some(false) } else { a.discoverable },
        indexable: !suspended && a.indexable,
        hide_collections: Some(a.hide_collections),
        show_featured: Some(a.show_featured),
        show_media: Some(a.show_media),
        show_media_replies: Some(a.show_media_replies),
        created_at: a.created_at.format("%Y-%m-%dT00:00:00.000Z").to_string(),
        note: if suspended { String::new() } else { a.note.clone() },
        url,
        uri,
        avatar: if suspended { DEFAULT_AVATAR.to_string() } else { a.avatar.clone().unwrap_or_else(|| DEFAULT_AVATAR.to_string()) },
        avatar_static: if suspended { DEFAULT_AVATAR.to_string() } else { a.avatar_static.clone().unwrap_or_else(|| DEFAULT_AVATAR.to_string()) },
        avatar_description: if suspended { String::new() } else { a.avatar_description.clone() },
        header: if suspended { DEFAULT_HEADER.to_string() } else { a.header.clone().unwrap_or_else(|| DEFAULT_HEADER.to_string()) },
        header_static: if suspended { DEFAULT_HEADER.to_string() } else { a.header_static.clone().unwrap_or_else(|| DEFAULT_HEADER.to_string()) },
        header_description: if suspended { String::new() } else { a.header_description.clone() },
        followers_count: a.followers_count,
        following_count: a.following_count,
        statuses_count: a.statuses_count,
        last_status_at: a.last_status_at.map(|t| t.format("%Y-%m-%d").to_string()),
        emojis: vec![],
        fields: if suspended { vec![] } else { fields_from_db(&a.fields) },
        roles: vec![],
        moved: None,
        suspended: if suspended { Some(true) } else { None },
        limited: if a.silenced_at.is_some() { Some(true) } else { None },
        noindex: if a.domain.is_none() { Some(!a.indexable) } else { None },
        memorial: if a.memorial { Some(true) } else { None },
        mute_expires_at: None,
        source: None,
        role: None,
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
        url: m.file_url.clone()
            .or_else(|| m.remote_url.as_deref().filter(|s| !s.is_empty()).map(str::to_string)),
        preview_url: m.preview_url.clone(),
        remote_url: m.remote_url.as_deref().filter(|s| !s.is_empty()).map(str::to_string),
        preview_remote_url: m.thumbnail_remote_url.as_deref().filter(|s| !s.is_empty()).map(str::to_string),
        text_url: None,
        description: m.description.clone(),
        blurhash: m.blurhash.clone(),
        meta: Some(m.meta.clone().unwrap_or_else(|| serde_json::json!({}))),
    }
}

/// Render status content from raw text, matching the Mastodon convention:
/// - local statuses: render from plaintext (linkify mentions/hashtags/URLs)
/// - remote statuses: sanitize the ActivityPub HTML
fn render_status_content(
    s: &models::Status,
    account: &models::Account,
    mentions: &[types::StatusMention],
) -> String {
    if account.domain.is_none() {
        // Local: text is raw plaintext, render to annotated HTML
        let domain = s.uri.as_deref()
            .and_then(|uri| uri.strip_prefix("https://"))
            .and_then(|rest| rest.split('/').next())
            .unwrap_or("");
        let map = mention_map_from_api(mentions);
        render_content(&s.text, domain, &map)
    } else {
        // Remote: text is ActivityPub HTML, sanitize before serving
        ammonia::clean(&s.text)
    }
}

fn ap_uri_to_policy_label(uri: &str) -> &'static str {
    match uri {
        "https://www.w3.org/ns/activitystreams#Public" => "public",
        u if u.ends_with("/followers") => "followers",
        u if u.ends_with("/following") => "following",
        _ => "unsupported_policy",
    }
}

fn build_quote_approval(
    s: &models::Status,
    viewer: Option<&StatusViewerContext>,
) -> types::QuoteApproval {
    let policy = s.interaction_policy.as_ref();
    let always_uris: Vec<String> = policy
        .and_then(|p| p.get("can_quote"))
        .and_then(|cq| cq.get("always"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
        .unwrap_or_else(|| {
            if matches!(s.visibility, crate::db::models::vis::PUBLIC | crate::db::models::vis::UNLISTED) {
                vec!["https://www.w3.org/ns/activitystreams#Public".to_string()]
            } else {
                vec![]
            }
        });
    let with_approval_uris: Vec<String> = policy
        .and_then(|p| p.get("can_quote"))
        .and_then(|cq| cq.get("with_approval"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();

    let automatic: Vec<String> = always_uris.iter().map(|u| ap_uri_to_policy_label(u).to_owned()).collect();
    let manual: Vec<String> = with_approval_uris.iter().map(|u| ap_uri_to_policy_label(u).to_owned()).collect();

    let current_user = match viewer {
        None => "unknown".to_string(),
        Some(ctx) => {
            let public_uri = "https://www.w3.org/ns/activitystreams#Public";
            if always_uris.iter().any(|u| u == public_uri) {
                "automatic".to_string()
            } else if always_uris.iter().any(|u| u.ends_with("/followers")) && ctx.follows_author {
                "automatic".to_string()
            } else if always_uris.iter().any(|u| u.ends_with("/following")) && ctx.author_follows {
                "automatic".to_string()
            } else if with_approval_uris.iter().any(|u| u == public_uri) {
                "manual".to_string()
            } else if with_approval_uris.iter().any(|u| u.ends_with("/followers")) && ctx.follows_author {
                "manual".to_string()
            } else if with_approval_uris.iter().any(|u| u.ends_with("/following")) && ctx.author_follows {
                "manual".to_string()
            } else {
                "denied".to_string()
            }
        }
    };

    types::QuoteApproval {
        automatic,
        manual,
        current_user,
    }
}

pub fn status_from_db(
    s: &models::Status,
    account: &models::Account,
    media: Vec<models::MediaAttachment>,
    reblog: Option<(models::Status, models::Account, Vec<models::MediaAttachment>)>,
    viewer_context: Option<StatusViewerContext>,
    mentions: &[types::StatusMention],
    reblog_mentions: &[types::StatusMention],
) -> types::Status {
    status_from_db_with_app(s, account, media, reblog, viewer_context, None, mentions, reblog_mentions)
}

pub fn status_from_db_with_app(
    s: &models::Status,
    account: &models::Account,
    media: Vec<models::MediaAttachment>,
    reblog: Option<(models::Status, models::Account, Vec<models::MediaAttachment>)>,
    viewer_context: Option<StatusViewerContext>,
    application: Option<types::Application>,
    mentions: &[types::StatusMention],
    reblog_mentions: &[types::StatusMention],
) -> types::Status {
    let content = render_status_content(s, account, mentions);
    let reblog_status = reblog.map(|(rs, ra, rm)| {
        Box::new(status_from_db(&rs, &ra, rm, None, viewer_context.clone(), reblog_mentions, &[]))
    });

    // Mastodon: the author always sees their own raw `sensitive` flag; sensitization
    // from account-level flags is only applied to other viewers.
    let is_author = viewer_context.as_ref().map(|c| c.account_id) == Some(account.id);
    let sensitive = if is_author {
        s.sensitive
    } else {
        s.sensitive || account.sensitized_at.is_some()
    };

    // Mastodon omits viewer-dependent fields entirely for unauthenticated responses.
    // `pinned` is further restricted to the author's own view.
    let (favourited, reblogged, muted, bookmarked, pinned, filtered) =
        if let Some(ref ctx) = viewer_context {
            (
                Some(ctx.favourited),
                Some(ctx.reblogged),
                Some(ctx.muted),
                Some(ctx.bookmarked),
                if is_author
                    && s.reblog_of_id.is_none()
                    && matches!(
                        s.visibility,
                        crate::db::models::vis::PUBLIC
                            | crate::db::models::vis::UNLISTED
                            | crate::db::models::vis::PRIVATE
                    )
                {
                    Some(ctx.pinned)
                } else {
                    None
                },
                Some(vec![]),
            )
        } else {
            (None, None, None, None, None, None)
        };

    types::Status {
        id: s.id.to_string(),
        created_at: s.created_at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        in_reply_to_id: s.in_reply_to_id.map(|i| i.to_string()),
        in_reply_to_account_id: s.in_reply_to_account_id.map(|i| i.to_string()),
        sensitive,
        spoiler_text: s.spoiler_text.clone(),
        visibility: crate::db::models::vis::to_str(s.visibility).to_owned(),
        language: s.language.clone(),
        uri: s.uri.clone().unwrap_or_else(|| s.id.to_string()),
        url: {
            let uri_str = s.uri.as_deref();
            s.url.as_deref()
                .filter(|&u| uri_str.map_or(true, |uri| u != uri))
                .map(String::from)
                .or_else(|| status_url_from_uri(uri_str?))
        },
        replies_count: s.replies_count,
        reblogs_count: s.reblogs_count,
        favourites_count: s.favourites_count,
        quotes_count: s.quotes_count,
        edited_at: s.edited_at.map(|t| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()),
        content,
        reblog: reblog_status,
        application,
        account: account_from_db(account),
        media_attachments: media.iter()
            .map(media_from_db)
            .filter(|m| m.url.is_some() || m.remote_url.as_deref().map_or(false, |u| !u.is_empty()))
            .collect(),
        mentions: mentions.to_vec(),
        tags: vec![],
        emojis: vec![],
        card: None,
        poll: None,
        quote: None,
        quote_approval: build_quote_approval(s, viewer_context.as_ref()),
        tagged_collections: vec![],
        favourited,
        reblogged,
        muted,
        bookmarked,
        pinned,
        filtered,
        text: None,
    }
}

#[derive(Clone)]
pub struct StatusViewerContext {
    pub account_id: i64,
    pub follows_author: bool,
    pub author_follows: bool,
    pub favourited: bool,
    pub reblogged: bool,
    pub muted: bool,
    pub bookmarked: bool,
    pub pinned: bool,
}
