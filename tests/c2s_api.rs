//! Mastodon C2S API compatibility tests.
//!
//! These tests run against a live HTTP server. By default they spin up eunha
//! on a random port, but you can point them at any Mastodon-compatible server:
//!
//!   C2S_BASE_URL=https://mastodon.social \
//!   C2S_HOST=mastodon.social \
//!   C2S_ALICE_TOKEN=<token> \
//!   C2S_ALICE_ID=<account-id> \
//!   C2S_BOB_TOKEN=<token> \
//!   C2S_BOB_ID=<account-id> \
//!   cargo test --test c2s_api
//!
//! When those env vars are absent the test harness bootstraps eunha
//! automatically using DATABASE_URL.

#[path = "c2s_api/helpers.rs"]
mod helpers;

#[path = "c2s_api/accounts.rs"]
mod accounts;
#[path = "c2s_api/conversations.rs"]
mod conversations;
#[path = "c2s_api/filters.rs"]
mod filters;
#[path = "c2s_api/instance.rs"]
mod instance;
#[path = "c2s_api/invites.rs"]
mod invites;
#[path = "c2s_api/lists.rs"]
mod lists;
#[path = "c2s_api/markers.rs"]
mod markers;
#[path = "c2s_api/notifications.rs"]
mod notifications;
#[path = "c2s_api/reports.rs"]
mod reports;
#[path = "c2s_api/search.rs"]
mod search;
#[path = "c2s_api/statuses.rs"]
mod statuses;
#[path = "c2s_api/tags.rs"]
mod tags;
#[path = "c2s_api/timelines.rs"]
mod timelines;
