use std::collections::HashMap;
use once_cell::sync::Lazy;
use regex::Regex;

use super::types::StatusMention;

pub static HASHTAG_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(^|[\s,.:;!?\(\[\{/])#([a-zA-Z][a-zA-Z0-9_]*)").unwrap()
});

pub static MENTION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(^|[\s,.:;!?\(\[\{/])@([a-zA-Z0-9_]+)(?:@([a-zA-Z0-9._:\-]+))?").unwrap()
});

pub static URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new("https?://[^\\s<>&\"]+").unwrap()
});

pub fn render_content(
    text: &str,
    domain: &str,
    mention_map: &HashMap<String, (String, String)>,
) -> String {
    if text.is_empty() {
        return String::new();
    }
    text.split("\n\n")
        .map(|para| {
            let linked = linkify_entities(para, domain, mention_map);
            format!("<p>{}</p>", linked.replace('\n', "<br />"))
        })
        .collect::<Vec<_>>()
        .join("")
}

fn linkify_entities(
    text: &str,
    domain: &str,
    mention_map: &HashMap<String, (String, String)>,
) -> String {
    struct Entity {
        start: usize,
        end: usize,
        html: String,
    }

    let mut entities: Vec<Entity> = Vec::new();

    for cap in HASHTAG_RE.captures_iter(text) {
        let full = cap.get(0).unwrap();
        let prefix_len = cap.get(1).unwrap().as_str().len();
        let tag_text = &cap[2];
        let tag_lower = tag_text.to_lowercase();
        let url = format!("https://{}/tags/{}", domain, urlencoding::encode(&tag_lower));
        entities.push(Entity {
            start: full.start() + prefix_len,
            end: full.end(),
            html: format!(
                r#"<a href="{}" class="mention hashtag" rel="tag">#<span>{}</span></a>"#,
                ammonia::clean_text(&url),
                ammonia::clean_text(tag_text),
            ),
        });
    }

    for cap in MENTION_RE.captures_iter(text) {
        let full = cap.get(0).unwrap();
        let prefix_len = cap.get(1).unwrap().as_str().len();
        let username = cap[2].to_lowercase();
        let mention_domain = cap.get(3).map(|m| m.as_str().to_lowercase());
        let key = match &mention_domain {
            Some(d) => format!("{}@{}", username, d),
            None => username.clone(),
        };
        if let Some((url, display)) = mention_map.get(&key) {
            entities.push(Entity {
                start: full.start() + prefix_len,
                end: full.end(),
                html: format!(
                    r#"<span class="h-card" translate="no"><a href="{}" class="u-url mention">@<span>{}</span></a></span>"#,
                    ammonia::clean_text(url),
                    ammonia::clean_text(display),
                ),
            });
        }
    }

    for m in URL_RE.find_iter(text) {
        let url = m.as_str();
        entities.push(Entity {
            start: m.start(),
            end: m.end(),
            html: format!(
                r#"<a href="{}" target="_blank" rel="nofollow noopener noreferrer">{}</a>"#,
                ammonia::clean_text(url),
                ammonia::clean_text(url),
            ),
        });
    }

    entities.sort_by_key(|e| e.start);

    let mut result = String::with_capacity(text.len() * 2);
    let mut last_end = 0usize;
    for entity in &entities {
        if entity.start < last_end {
            continue;
        }
        result.push_str(&ammonia::clean_text(&text[last_end..entity.start]));
        result.push_str(&entity.html);
        last_end = entity.end;
    }
    result.push_str(&ammonia::clean_text(&text[last_end..]));
    result
}

/// Build a mention lookup map from a `StatusMention` slice.
/// Keys are the lowercase acct handle (`user` or `user@domain`).
pub fn mention_map_from_api(mentions: &[StatusMention]) -> HashMap<String, (String, String)> {
    let mut map = HashMap::new();
    for m in mentions {
        let key_short = m.username.to_lowercase();
        map.entry(key_short.clone())
            .or_insert_with(|| (m.url.clone(), m.acct.clone()));
        if m.acct.contains('@') {
            map.entry(m.acct.to_lowercase())
                .or_insert_with(|| (m.url.clone(), m.acct.clone()));
        }
    }
    map
}
