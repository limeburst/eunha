//! ActivityPub vocabulary types and activity constructors for fediverse federation.
//!
//! This crate provides typed construction of common ActivityPub activities. Each
//! constructor returns a [`serde_json::Value`] that is ready to serialize and deliver.

pub use serde_json::Value;

pub const AS_CONTEXT: &str = "https://www.w3.org/ns/activitystreams";
pub const AS_PUBLIC: &str = "https://www.w3.org/ns/activitystreams#Public";

// ── Follow ────────────────────────────────────────────────────────────────────

/// Build a `Follow` activity.
///
/// The `id` should be a unique URI for this follow (e.g., `https://example.com/users/alice/follows/1`).
pub fn follow(id: &str, actor: &str, object: &str) -> Value {
    serde_json::json!({
        "@context": AS_CONTEXT,
        "id": id,
        "type": "Follow",
        "actor": actor,
        "object": object,
    })
}

// ── Accept / Reject ───────────────────────────────────────────────────────────

/// Build an `Accept(Follow)` activity sent in response to a received [`follow`].
pub fn accept_follow(
    id: &str,
    actor: &str,
    follow_id: &str,
    follow_actor: &str,
    follow_object: &str,
) -> Value {
    serde_json::json!({
        "@context": AS_CONTEXT,
        "id": id,
        "type": "Accept",
        "actor": actor,
        "object": {
            "id": follow_id,
            "type": "Follow",
            "actor": follow_actor,
            "object": follow_object,
        }
    })
}

/// Build a `Reject(Follow)` activity.
pub fn reject_follow(
    id: &str,
    actor: &str,
    follow_id: &str,
    follow_actor: &str,
    follow_object: &str,
) -> Value {
    serde_json::json!({
        "@context": AS_CONTEXT,
        "id": id,
        "type": "Reject",
        "actor": actor,
        "object": {
            "id": follow_id,
            "type": "Follow",
            "actor": follow_actor,
            "object": follow_object,
        }
    })
}

// ── Undo ──────────────────────────────────────────────────────────────────────

/// Build an `Undo(Follow)` activity used when unfollowing.
pub fn undo_follow(
    id: &str,
    actor: &str,
    follow_id: &str,
    follow_actor: &str,
    follow_object: &str,
) -> Value {
    serde_json::json!({
        "@context": AS_CONTEXT,
        "id": id,
        "type": "Undo",
        "actor": actor,
        "object": {
            "id": follow_id,
            "type": "Follow",
            "actor": follow_actor,
            "object": follow_object,
        }
    })
}

// ── Delete ────────────────────────────────────────────────────────────────────

/// Build a `Delete` activity for a local object being removed.
pub fn delete(id: &str, actor: &str, object: &str) -> Value {
    serde_json::json!({
        "@context": AS_CONTEXT,
        "id": id,
        "type": "Delete",
        "actor": actor,
        "object": {
            "id": object,
            "type": "Tombstone",
        }
    })
}

// ── Create(Note) ──────────────────────────────────────────────────────────────

/// Parameters for a Note object inside a Create activity.
pub struct NoteParams<'a> {
    pub id: &'a str,
    pub attributed_to: &'a str,
    pub content: &'a str,
    pub summary: Option<&'a str>,
    pub sensitive: bool,
    pub in_reply_to: Option<&'a str>,
    pub to: &'a [&'a str],
    pub cc: &'a [&'a str],
    pub published: &'a str,
    pub url: &'a str,
    /// FEP-044f quote URI, if this is a quote post.
    pub quote_url: Option<&'a str>,
}

/// Build a `Create(Note)` activity.
///
/// `activity_id` is the URI for the Create activity itself (typically the note URI + `/activity`).
pub fn create_note(activity_id: &str, actor: &str, note: NoteParams<'_>) -> Value {
    let to: Vec<&str> = note.to.to_vec();
    let cc: Vec<&str> = note.cc.to_vec();

    let mut note_obj = serde_json::json!({
        "id": note.id,
        "type": "Note",
        "attributedTo": note.attributed_to,
        "content": note.content,
        "sensitive": note.sensitive,
        "published": note.published,
        "url": note.url,
        "to": to,
        "cc": cc,
        "attachment": [],
        "tag": [],
    });

    if let Some(s) = note.summary {
        note_obj["summary"] = serde_json::Value::String(s.to_string());
    }
    if let Some(irt) = note.in_reply_to {
        note_obj["inReplyTo"] = serde_json::Value::String(irt.to_string());
    }
    if let Some(q) = note.quote_url {
        note_obj["quoteUrl"] = serde_json::Value::String(q.to_string());
    }

    serde_json::json!({
        "@context": [
            AS_CONTEXT,
            {
                "fep": "https://w3id.org/fep/044f#",
                "quoteUrl": { "@id": "fep:quote", "@type": "@id" },
            }
        ],
        "id": activity_id,
        "type": "Create",
        "actor": actor,
        "to": to,
        "cc": cc,
        "object": note_obj,
    })
}
