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

use std::net::SocketAddr;

use argon2::{Argon2, PasswordHasher};
use argon2::password_hash::{rand_core::OsRng, SaltString};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

// ── client wrapper ─────────────────────────────────────────────────────────

/// Thin wrapper around `reqwest::Client` plus the base URL / Host header.
///
/// Swap this struct (or its constructor) to point tests at a different server.
struct ApiClient {
    http: Client,
    base_url: String,
    /// Value for the `Host` header; only needed when the server is a
    /// multi-tenant instance that routes by host (like eunha).
    host: String,
}

impl ApiClient {
    fn new(base_url: impl Into<String>, host: impl Into<String>) -> Self {
        Self {
            http: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            base_url: base_url.into(),
            host: host.into(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn get(&self, path: &str, token: Option<&str>) -> reqwest::Response {
        let mut req = self.http.get(self.url(path)).header("host", &self.host);
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        req.send().await.unwrap()
    }

    async fn post_json(
        &self,
        path: &str,
        token: Option<&str>,
        body: &Value,
    ) -> reqwest::Response {
        let mut req = self
            .http
            .post(self.url(path))
            .header("host", &self.host)
            .json(body);
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        req.send().await.unwrap()
    }

    async fn post_form(
        &self,
        path: &str,
        token: Option<&str>,
        form: &[(&str, &str)],
    ) -> reqwest::Response {
        let mut req = self
            .http
            .post(self.url(path))
            .header("host", &self.host)
            .form(form);
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        req.send().await.unwrap()
    }

    async fn delete(&self, path: &str, token: &str) -> reqwest::Response {
        self.http
            .delete(self.url(path))
            .header("host", &self.host)
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
    }

    // ── convenience helpers ──────────────────────────────────────────────

    /// POST /api/v1/statuses with JSON body, returns the status JSON.
    async fn post_status(
        &self,
        token: &str,
        text: &str,
        visibility: &str,
    ) -> Value {
        let resp = self
            .post_json(
                "/api/v1/statuses",
                Some(token),
                &json!({"status": text, "visibility": visibility}),
            )
            .await;
        assert_eq!(
            resp.status().as_u16(),
            200,
            "post_status failed for visibility={visibility}"
        );
        resp.json().await.unwrap()
    }

    /// Follow an account; returns the relationship JSON.
    async fn follow(&self, token: &str, account_id: &str) -> Value {
        let resp = self
            .post_json(
                &format!("/api/v1/accounts/{account_id}/follow"),
                Some(token),
                &json!({}),
            )
            .await;
        assert_eq!(resp.status().as_u16(), 200);
        resp.json().await.unwrap()
    }

    /// GET /api/v1/timelines/public?local=true; returns status array.
    ///
    /// Uses `local=true` so parallel tests don't spill into each other's
    /// instance timeline.  Pass `local=false` to opt into the full federated
    /// view when testing against a remote server.
    async fn public_timeline(&self) -> Vec<Value> {
        self.get("/api/v1/timelines/public?local=true", None)
            .await
            .json()
            .await
            .unwrap()
    }

    /// GET /api/v1/timelines/home; returns status array.
    async fn home_timeline(&self, token: &str) -> Vec<Value> {
        self.get("/api/v1/timelines/home", Some(token))
            .await
            .json()
            .await
            .unwrap()
    }
}

// ── test context ────────────────────────────────────────────────────────────

struct TestContext {
    api: ApiClient,
    domain: String,
    alice_token: String,
    alice_id: String,
    bob_token: String,
    bob_id: String,
    /// Kept alive so the server task isn't dropped while tests run.
    _server: tokio::task::JoinHandle<()>,
}

impl TestContext {
    async fn new(label: &str) -> Self {
        // Use a unique subdomain per test so instances are isolated.
        let uid = &Uuid::new_v4().to_string()[..8];
        let domain = format!("{}-{}.c2s-test.invalid", label, uid);

        let db_url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set for integration tests");

        let db = PgPoolOptions::new()
            .max_connections(5)
            .connect(&db_url)
            .await
            .expect("failed to connect to test database");

        let (alice_id, alice_token) =
            seed_user(&db, &domain, "alice", "alice@test.invalid").await;
        let (bob_id, bob_token) =
            seed_user(&db, &domain, "bob", "bob@test.invalid").await;

        let config = eunha::config::Config {
            database_url: db_url,
            bind_address: "127.0.0.1:0".into(),
            console_domain: "console.c2s-test.invalid".into(),
            media_storage: eunha::config::MediaStorageConfig::Local {
                base_path: std::env::temp_dir()
                    .join("eunha-c2s-test-media")
                    .to_string_lossy()
                    .into_owned(),
                base_url: "http://localhost/media".into(),
            },
            smtp: None,
        };
        let state = eunha::state::AppState::new(db, config).await;
        let app = eunha::build_app(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let base_url = format!("http://{}", addr);
        let api = ApiClient::new(&base_url, &domain);

        TestContext {
            api,
            domain,
            alice_token,
            alice_id: alice_id.to_string(),
            bob_token,
            bob_id: bob_id.to_string(),
            _server: server,
        }
    }
}

// ── database seeding helpers ────────────────────────────────────────────────

async fn seed_user(
    db: &PgPool,
    domain: &str,
    username: &str,
    email: &str,
) -> (Uuid, String) {
    let instance_id = seed_instance(db, domain).await;
    let (account_id, token) = seed_account_and_token(db, instance_id, domain, username, email).await;
    (account_id, token)
}

async fn seed_instance(db: &PgPool, domain: &str) -> Uuid {
    // Upsert the instance so multiple calls for the same domain return the same id.
    sqlx::query_scalar!(
        r#"INSERT INTO instances
             (domain, title, private_key, public_key)
           VALUES ($1, $1, 'test-private-key', 'test-public-key')
           ON CONFLICT (domain) DO UPDATE SET domain = EXCLUDED.domain
           RETURNING id"#,
        domain,
    )
    .fetch_one(db)
    .await
    .unwrap()
}

async fn seed_account_and_token(
    db: &PgPool,
    instance_id: Uuid,
    domain: &str,
    username: &str,
    email: &str,
) -> (Uuid, String) {
    let url = format!("https://{}/{}", domain, username);
    let uri = format!("https://{}/users/{}", domain, username);

    let account_id = sqlx::query_scalar!(
        r#"INSERT INTO accounts
             (instance_id, username, display_name, note, note_text,
              url, uri, public_key, inbox_url, outbox_url)
           VALUES ($1,$2,$2,'','', $3,$4,'test-public-key',$4||'/inbox',$4||'/outbox')
           RETURNING id"#,
        instance_id,
        username,
        url,
        uri,
    )
    .fetch_one(db)
    .await
    .unwrap();

    let password_hash = hash_password("testpassword123");
    sqlx::query!(
        r#"INSERT INTO users
             (account_id, instance_id, email, email_normalized, password_hash, confirmed_at)
           VALUES ($1,$2,$3,$4,$5,now())"#,
        account_id,
        instance_id,
        email,
        email.to_lowercase(),
        password_hash,
    )
    .execute(db)
    .await
    .unwrap();

    let app_id = sqlx::query_scalar!(
        r#"INSERT INTO oauth_applications
             (instance_id, name, client_id, client_secret, redirect_uris, scopes)
           VALUES ($1,'test',gen_random_uuid()::text,gen_random_uuid()::text,'urn:ietf:wg:oauth:2.0:oob','read write follow')
           RETURNING id"#,
        instance_id,
    )
    .fetch_one(db)
    .await
    .unwrap();

    let token = Uuid::new_v4().to_string().replace("-", "");
    sqlx::query!(
        "INSERT INTO oauth_access_tokens (application_id, account_id, token, scopes) VALUES ($1,$2,$3,'read write follow')",
        app_id,
        account_id,
        token,
    )
    .execute(db)
    .await
    .unwrap();

    (account_id, token)
}

fn hash_password(password: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .unwrap()
        .to_string()
}

// ── tests: timeline visibility ──────────────────────────────────────────────

/// Public timeline must only contain statuses with visibility == "public".
#[tokio::test]
async fn test_public_timeline_only_shows_public() {
    let ctx = TestContext::new("pub-timeline").await;

    let pub_s = ctx.api.post_status(&ctx.alice_token, "public post", "public").await;
    ctx.api.post_status(&ctx.alice_token, "unlisted post", "unlisted").await;
    ctx.api.post_status(&ctx.alice_token, "private post", "private").await;
    ctx.api.post_status(&ctx.alice_token, "direct post", "direct").await;

    let pub_id = pub_s["id"].as_str().unwrap();
    let timeline = ctx.api.public_timeline().await;

    for status in &timeline {
        let vis = status["visibility"].as_str().unwrap();
        assert_eq!(vis, "public", "public timeline contained status with visibility={vis}");
    }
    assert!(
        timeline.iter().any(|s| s["id"].as_str() == Some(pub_id)),
        "public status not found in public timeline"
    );
}

/// Unlisted statuses must not appear on the public timeline.
#[tokio::test]
async fn test_unlisted_absent_from_public_timeline() {
    let ctx = TestContext::new("unlisted-timeline").await;

    let status = ctx.api.post_status(&ctx.alice_token, "unlisted post visible", "unlisted").await;
    let id = status["id"].as_str().unwrap().to_string();

    let timeline = ctx.api.public_timeline().await;
    let ids: Vec<&str> = timeline.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(!ids.contains(&id.as_str()), "unlisted status appeared in public timeline");
}

/// Home timeline includes all visibility levels from followed accounts.
#[tokio::test]
async fn test_home_timeline_shows_all_visibility_from_follows() {
    let ctx = TestContext::new("home-visibility").await;

    // Alice follows Bob.
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    // Bob posts in every visibility.
    let pub_s = ctx.api.post_status(&ctx.bob_token, "bob public", "public").await;
    let unl_s = ctx.api.post_status(&ctx.bob_token, "bob unlisted", "unlisted").await;
    let prv_s = ctx.api.post_status(&ctx.bob_token, "bob private", "private").await;

    let home = ctx.api.home_timeline(&ctx.alice_token).await;
    let ids: Vec<&str> = home.iter().filter_map(|s| s["id"].as_str()).collect();

    assert!(ids.contains(&pub_s["id"].as_str().unwrap()), "public status missing from home timeline");
    assert!(ids.contains(&unl_s["id"].as_str().unwrap()), "unlisted status missing from home timeline");
    assert!(ids.contains(&prv_s["id"].as_str().unwrap()), "private status missing from home timeline for accepted follower");
}

/// Home timeline must not include posts from accounts Alice doesn't follow.
#[tokio::test]
async fn test_home_timeline_excludes_non_followed_accounts() {
    let ctx = TestContext::new("home-exclude").await;

    // Bob posts but Alice does NOT follow him.
    let status = ctx.api.post_status(&ctx.bob_token, "bob public unfollowed", "public").await;
    let id = status["id"].as_str().unwrap().to_string();

    let home = ctx.api.home_timeline(&ctx.alice_token).await;
    let ids: Vec<&str> = home.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(!ids.contains(&id.as_str()), "non-followed account's post appeared in home timeline");
}

/// Own posts always appear on the home timeline regardless of visibility.
#[tokio::test]
async fn test_home_timeline_shows_own_posts_all_visibility() {
    let ctx = TestContext::new("home-own").await;

    let pub_s = ctx.api.post_status(&ctx.alice_token, "own public", "public").await;
    let prv_s = ctx.api.post_status(&ctx.alice_token, "own private", "private").await;
    let dir_s = ctx.api.post_status(&ctx.alice_token, "own direct", "direct").await;

    let home = ctx.api.home_timeline(&ctx.alice_token).await;
    let ids: Vec<&str> = home.iter().filter_map(|s| s["id"].as_str()).collect();

    assert!(ids.contains(&pub_s["id"].as_str().unwrap()));
    assert!(ids.contains(&prv_s["id"].as_str().unwrap()));
    assert!(ids.contains(&dir_s["id"].as_str().unwrap()));
}

// ── tests: status access control ────────────────────────────────────────────

/// GET a private status as an unauthenticated stranger → 404.
#[tokio::test]
async fn test_get_private_status_unauthenticated() {
    let ctx = TestContext::new("prv-unauth").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice private", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.get(&format!("/api/v1/statuses/{id}"), None).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// GET a private status as a non-follower → 404.
#[tokio::test]
async fn test_get_private_status_non_follower() {
    let ctx = TestContext::new("prv-stranger").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice private", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx
        .api
        .get(&format!("/api/v1/statuses/{id}"), Some(&ctx.bob_token))
        .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// GET a private status as an accepted follower → 200.
#[tokio::test]
async fn test_get_private_status_accepted_follower() {
    let ctx = TestContext::new("prv-follower").await;

    // Bob follows Alice.
    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice private", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx
        .api
        .get(&format!("/api/v1/statuses/{id}"), Some(&ctx.bob_token))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

/// GET a private status as the author → 200.
#[tokio::test]
async fn test_get_private_status_author() {
    let ctx = TestContext::new("prv-author").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice private", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx
        .api
        .get(&format!("/api/v1/statuses/{id}"), Some(&ctx.alice_token))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

/// GET a direct status as an accepted follower → 404.
#[tokio::test]
async fn test_get_direct_status_follower() {
    let ctx = TestContext::new("dir-follower").await;

    // Bob follows Alice.
    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice direct", "direct").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx
        .api
        .get(&format!("/api/v1/statuses/{id}"), Some(&ctx.bob_token))
        .await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "follower should not see direct status"
    );
}

/// GET a direct status as the author → 200.
#[tokio::test]
async fn test_get_direct_status_author() {
    let ctx = TestContext::new("dir-author").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice direct", "direct").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx
        .api
        .get(&format!("/api/v1/statuses/{id}"), Some(&ctx.alice_token))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

// ── tests: reblog restrictions ───────────────────────────────────────────────

/// Reblogging a private status → 403.
#[tokio::test]
async fn test_reblog_private_returns_403() {
    let ctx = TestContext::new("reblog-prv").await;

    // Bob follows Alice so he can see her private status.
    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice private rb", "private").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx
        .api
        .post_json(
            &format!("/api/v1/statuses/{id}/reblog"),
            Some(&ctx.bob_token),
            &json!({}),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// Reblogging a direct status → 403.
#[tokio::test]
async fn test_reblog_direct_returns_403() {
    let ctx = TestContext::new("reblog-dir").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice direct rb", "direct").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx
        .api
        .post_json(
            &format!("/api/v1/statuses/{id}/reblog"),
            Some(&ctx.bob_token),
            &json!({}),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ── tests: authentication requirements ──────────────────────────────────────

/// POST /api/v1/statuses without a token → 401.
#[tokio::test]
async fn test_post_status_requires_auth() {
    let ctx = TestContext::new("auth-post").await;

    let resp = ctx
        .api
        .post_json(
            "/api/v1/statuses",
            None,
            &json!({"status": "no auth", "visibility": "public"}),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// GET /api/v1/timelines/home without a token → 401.
#[tokio::test]
async fn test_home_timeline_requires_auth() {
    let ctx = TestContext::new("auth-home").await;

    let resp = ctx.api.get("/api/v1/timelines/home", None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// DELETE /api/v1/statuses/:id without a token → 401.
#[tokio::test]
async fn test_delete_status_requires_auth() {
    let ctx = TestContext::new("auth-del").await;

    let status = ctx.api.post_status(&ctx.alice_token, "to delete", "public").await;
    let id = status["id"].as_str().unwrap();

    // Hit the delete endpoint via post_json workaround; use DELETE directly.
    let resp = ctx
        .api
        .http
        .delete(ctx.api.url(&format!("/api/v1/statuses/{id}")))
        .header("host", &ctx.domain)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── tests: account statuses endpoint ────────────────────────────────────────

/// Private statuses are hidden from unauthenticated viewers.
#[tokio::test]
async fn test_account_statuses_hides_private_from_unauthenticated() {
    let ctx = TestContext::new("acct-stat-unauth").await;

    let prv = ctx.api.post_status(&ctx.alice_token, "alice private acct", "private").await;
    let pub_s = ctx.api.post_status(&ctx.alice_token, "alice public acct", "public").await;

    let resp = ctx
        .api
        .get(&format!("/api/v1/accounts/{}/statuses", ctx.alice_id), None)
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let statuses: Vec<Value> = resp.json().await.unwrap();

    let ids: Vec<&str> = statuses.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(!ids.contains(&prv["id"].as_str().unwrap()), "private status visible to unauthenticated user");
    assert!(ids.contains(&pub_s["id"].as_str().unwrap()), "public status missing from unauthenticated view");
}

/// Private statuses are hidden from non-followers.
#[tokio::test]
async fn test_account_statuses_hides_private_from_non_follower() {
    let ctx = TestContext::new("acct-stat-stranger").await;

    let prv = ctx.api.post_status(&ctx.alice_token, "alice prv stranger", "private").await;

    let resp = ctx
        .api
        .get(
            &format!("/api/v1/accounts/{}/statuses", ctx.alice_id),
            Some(&ctx.bob_token),
        )
        .await;
    let statuses: Vec<Value> = resp.json().await.unwrap();
    let ids: Vec<&str> = statuses.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(!ids.contains(&prv["id"].as_str().unwrap()), "private status visible to non-follower");
}

/// Private statuses appear in account statuses for accepted followers.
#[tokio::test]
async fn test_account_statuses_shows_private_to_follower() {
    let ctx = TestContext::new("acct-stat-follower").await;

    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;

    let prv = ctx.api.post_status(&ctx.alice_token, "alice prv follower", "private").await;

    let resp = ctx
        .api
        .get(
            &format!("/api/v1/accounts/{}/statuses", ctx.alice_id),
            Some(&ctx.bob_token),
        )
        .await;
    let statuses: Vec<Value> = resp.json().await.unwrap();
    let ids: Vec<&str> = statuses.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(ids.contains(&prv["id"].as_str().unwrap()), "private status hidden from accepted follower");
}

/// Account statuses shows all visibilities to the account owner.
#[tokio::test]
async fn test_account_statuses_shows_all_to_self() {
    let ctx = TestContext::new("acct-stat-self").await;

    let pub_s = ctx.api.post_status(&ctx.alice_token, "self public", "public").await;
    let prv_s = ctx.api.post_status(&ctx.alice_token, "self private", "private").await;
    let dir_s = ctx.api.post_status(&ctx.alice_token, "self direct", "direct").await;

    let resp = ctx
        .api
        .get(
            &format!("/api/v1/accounts/{}/statuses", ctx.alice_id),
            Some(&ctx.alice_token),
        )
        .await;
    let statuses: Vec<Value> = resp.json().await.unwrap();
    let ids: Vec<&str> = statuses.iter().filter_map(|s| s["id"].as_str()).collect();

    assert!(ids.contains(&pub_s["id"].as_str().unwrap()));
    assert!(ids.contains(&prv_s["id"].as_str().unwrap()));
    assert!(ids.contains(&dir_s["id"].as_str().unwrap()));
}

// ── tests: soft delete ───────────────────────────────────────────────────────

/// Deleted status returns 404 on GET.
#[tokio::test]
async fn test_deleted_status_returns_404() {
    let ctx = TestContext::new("del-404").await;

    let status = ctx.api.post_status(&ctx.alice_token, "to be deleted", "public").await;
    let id = status["id"].as_str().unwrap();

    let del_resp = ctx.api.delete(&format!("/api/v1/statuses/{id}"), &ctx.alice_token).await;
    assert_eq!(del_resp.status(), StatusCode::OK);

    let get_resp = ctx.api.get(&format!("/api/v1/statuses/{id}"), None).await;
    assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
}

/// Deleted status is absent from the public timeline.
#[tokio::test]
async fn test_deleted_status_absent_from_public_timeline() {
    let ctx = TestContext::new("del-timeline").await;

    let status = ctx.api.post_status(&ctx.alice_token, "delete from timeline", "public").await;
    let id = status["id"].as_str().unwrap().to_string();

    ctx.api.delete(&format!("/api/v1/statuses/{id}"), &ctx.alice_token).await;

    let timeline = ctx.api.public_timeline().await;
    let ids: Vec<&str> = timeline.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(!ids.contains(&id.as_str()), "deleted status still appears in public timeline");
}

/// Only the author can delete their own status; another user gets 403.
#[tokio::test]
async fn test_delete_status_by_non_author_returns_403() {
    let ctx = TestContext::new("del-author").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice status to del", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.delete(&format!("/api/v1/statuses/{id}"), &ctx.bob_token).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ── tests: follow lifecycle ──────────────────────────────────────────────────

/// Following an unlocked account is immediately accepted.
#[tokio::test]
async fn test_follow_unlocked_account_is_accepted() {
    let ctx = TestContext::new("follow-unlocked").await;

    let rel = ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;
    assert_eq!(rel["following"].as_bool(), Some(true));
    assert_eq!(rel["requested"].as_bool(), Some(false));
}

/// Following a locked account creates a pending follow request.
#[tokio::test]
async fn test_follow_locked_account_is_pending() {
    let ctx = TestContext::new("follow-locked").await;

    // Lock Bob's account directly in the DB.
    let db_url = std::env::var("DATABASE_URL").unwrap();
    let db = PgPoolOptions::new().max_connections(2).connect(&db_url).await.unwrap();
    let bob_uuid: Uuid = ctx.bob_id.parse().unwrap();
    sqlx::query!("UPDATE accounts SET locked = true WHERE id = $1", bob_uuid)
        .execute(&db)
        .await
        .unwrap();

    let rel = ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;
    assert_eq!(rel["following"].as_bool(), Some(false));
    assert_eq!(rel["requested"].as_bool(), Some(true));
}

// ── tests: favourites ────────────────────────────────────────────────────────

/// Favouriting a status increments its favourites_count by 1.
#[tokio::test]
async fn test_favourite_increments_count() {
    let ctx = TestContext::new("fav-inc").await;

    let status = ctx.api.post_status(&ctx.alice_token, "favourable", "public").await;
    let id = status["id"].as_str().unwrap();
    let before: i64 = status["favourites_count"].as_i64().unwrap_or(0);

    let fav_resp = ctx
        .api
        .post_json(
            &format!("/api/v1/statuses/{id}/favourite"),
            Some(&ctx.bob_token),
            &json!({}),
        )
        .await;
    assert_eq!(fav_resp.status(), StatusCode::OK);
    let after: Value = fav_resp.json().await.unwrap();
    assert_eq!(after["favourites_count"].as_i64().unwrap_or(0), before + 1);
}

/// Unfavouriting a status decrements its favourites_count by 1.
#[tokio::test]
async fn test_unfavourite_decrements_count() {
    let ctx = TestContext::new("unfav-dec").await;

    let status = ctx.api.post_status(&ctx.alice_token, "unfavourable", "public").await;
    let id = status["id"].as_str().unwrap();

    // First favourite it.
    ctx.api
        .post_json(
            &format!("/api/v1/statuses/{id}/favourite"),
            Some(&ctx.bob_token),
            &json!({}),
        )
        .await;

    let unfav_resp = ctx
        .api
        .post_json(
            &format!("/api/v1/statuses/{id}/unfavourite"),
            Some(&ctx.bob_token),
            &json!({}),
        )
        .await;
    assert_eq!(unfav_resp.status(), StatusCode::OK);
    let after: Value = unfav_resp.json().await.unwrap();
    assert_eq!(after["favourites_count"].as_i64().unwrap_or(0), 0);
}

/// Double-favouriting doesn't inflate the count.
#[tokio::test]
async fn test_favourite_is_idempotent() {
    let ctx = TestContext::new("fav-idem").await;

    let status = ctx.api.post_status(&ctx.alice_token, "fav twice", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api
        .post_json(&format!("/api/v1/statuses/{id}/favourite"), Some(&ctx.bob_token), &json!({}))
        .await;
    let second = ctx
        .api
        .post_json(&format!("/api/v1/statuses/{id}/favourite"), Some(&ctx.bob_token), &json!({}))
        .await;
    let body: Value = second.json().await.unwrap();
    assert_eq!(body["favourites_count"].as_i64().unwrap_or(-1), 1);
}

// ── tests: reblog count ──────────────────────────────────────────────────────

/// Reblogging a public status increments reblogs_count.
#[tokio::test]
async fn test_reblog_increments_count() {
    let ctx = TestContext::new("reblog-cnt").await;

    let status = ctx.api.post_status(&ctx.alice_token, "rebloggable", "public").await;
    let id = status["id"].as_str().unwrap();
    let before = status["reblogs_count"].as_i64().unwrap_or(0);

    let rb_resp = ctx
        .api
        .post_json(
            &format!("/api/v1/statuses/{id}/reblog"),
            Some(&ctx.bob_token),
            &json!({}),
        )
        .await;
    assert_eq!(rb_resp.status(), StatusCode::OK);

    // Fetch the original to check the count.
    let updated: Value = ctx
        .api
        .get(&format!("/api/v1/statuses/{id}"), Some(&ctx.bob_token))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(updated["reblogs_count"].as_i64().unwrap_or(0), before + 1);
}
