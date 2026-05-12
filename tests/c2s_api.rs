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

    async fn put_json(
        &self,
        path: &str,
        token: Option<&str>,
        body: &Value,
    ) -> reqwest::Response {
        let mut req = self
            .http
            .put(self.url(path))
            .header("host", &self.host)
            .json(body);
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        req.send().await.unwrap()
    }

    async fn patch_json(
        &self,
        path: &str,
        token: Option<&str>,
        body: &Value,
    ) -> reqwest::Response {
        let mut req = self
            .http
            .patch(self.url(path))
            .header("host", &self.host)
            .json(body);
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        req.send().await.unwrap()
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
            media_storage: eunha::config::MediaStorageConfig {
                bucket: "test-bucket".into(),
                region: "us-east-1".into(),
                endpoint: None,
                access_key_id: "test-key".into(),
                secret_access_key: "test-secret".into(),
                base_url: "http://localhost/media".into(),
            },
            smtp: None,
            resend: None,
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

// ── tests: multi-instance isolation ─────────────────────────────────────────

/// The federated public timeline must be scoped to the requesting instance.
///
/// Regression test: previously the query omitted the `instance_id` filter when
/// `?local=false`, causing every instance's statuses to bleed into every other
/// instance's timeline.
#[tokio::test]
async fn test_public_timeline_scoped_to_instance() {
    let ctx_a = TestContext::new("scope-a").await;
    let ctx_b = TestContext::new("scope-b").await;

    // Post publicly on instance B.
    let b_status = ctx_b.api.post_status(&ctx_b.alice_token, "from instance B only", "public").await;
    let b_id = b_status["id"].as_str().unwrap().to_string();

    // Also post on instance A so the timeline is non-empty.
    let a_status = ctx_a.api.post_status(&ctx_a.alice_token, "from instance A only", "public").await;
    let a_id = a_status["id"].as_str().unwrap().to_string();

    // Federated timeline (no ?local) for instance A.
    let timeline: Vec<Value> = ctx_a.api
        .get("/api/v1/timelines/public", None)
        .await
        .json()
        .await
        .unwrap();

    let ids: Vec<&str> = timeline.iter().filter_map(|s| s["id"].as_str()).collect();

    assert!(
        ids.contains(&a_id.as_str()),
        "instance A's own status not found in its federated timeline"
    );
    assert!(
        !ids.contains(&b_id.as_str()),
        "instance B's status leaked into instance A's federated timeline"
    );
}

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

// ── tests: account endpoints ─────────────────────────────────────────────────

/// GET /api/v1/accounts/verify_credentials returns the current user's account.
#[tokio::test]
async fn test_verify_credentials() {
    let ctx = TestContext::new("verify-creds").await;

    let resp = ctx.api.get("/api/v1/accounts/verify_credentials", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    assert_eq!(body["username"].as_str(), Some("alice"));
    assert!(body["id"].as_str().is_some(), "id field missing");
    assert!(body["acct"].as_str().is_some(), "acct field missing");
    assert!(body["source"].is_object(), "source field missing from verify_credentials");
}

/// GET /api/v1/accounts/verify_credentials without token → 401.
#[tokio::test]
async fn test_verify_credentials_requires_auth() {
    let ctx = TestContext::new("verify-unauth").await;

    let resp = ctx.api.get("/api/v1/accounts/verify_credentials", None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// GET /api/v1/accounts/:id returns account data.
#[tokio::test]
async fn test_get_account() {
    let ctx = TestContext::new("get-acct").await;

    let resp = ctx.api.get(&format!("/api/v1/accounts/{}", ctx.alice_id), None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    assert_eq!(body["id"].as_str(), Some(ctx.alice_id.as_str()));
    assert_eq!(body["username"].as_str(), Some("alice"));
}

/// GET /api/v1/accounts/:id for unknown id → 404.
#[tokio::test]
async fn test_get_account_not_found() {
    let ctx = TestContext::new("get-acct-404").await;

    let resp = ctx.api.get("/api/v1/accounts/00000000-0000-0000-0000-000000000000", None).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// GET /api/v1/accounts/lookup?acct=alice returns Alice's account.
#[tokio::test]
async fn test_lookup_account() {
    let ctx = TestContext::new("lookup").await;

    let resp = ctx.api.get("/api/v1/accounts/lookup?acct=alice", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    assert_eq!(body["username"].as_str(), Some("alice"));
}

/// GET /api/v1/accounts/:id/followers returns a list after a follow.
#[tokio::test]
async fn test_get_account_followers() {
    let ctx = TestContext::new("acct-followers").await;

    ctx.api.follow(&ctx.bob_token, &ctx.alice_id).await;

    let resp = ctx.api.get(
        &format!("/api/v1/accounts/{}/followers", ctx.alice_id),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(list.iter().any(|a| a["id"].as_str() == Some(ctx.bob_id.as_str())));
}

/// GET /api/v1/accounts/:id/following returns a list after a follow.
#[tokio::test]
async fn test_get_account_following() {
    let ctx = TestContext::new("acct-following").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let resp = ctx.api.get(
        &format!("/api/v1/accounts/{}/following", ctx.alice_id),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(list.iter().any(|a| a["id"].as_str() == Some(ctx.bob_id.as_str())));
}

// ── tests: relationships ─────────────────────────────────────────────────────

/// GET /api/v1/accounts/relationships reflects follow state.
#[tokio::test]
async fn test_get_relationships() {
    let ctx = TestContext::new("rel-basic").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let resp = ctx.api.get(
        &format!("/api/v1/accounts/relationships?id[]={}", ctx.bob_id),
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["following"].as_bool(), Some(true));
    assert_eq!(list[0]["id"].as_str(), Some(ctx.bob_id.as_str()));
}

/// Unfollowing sets following=false in the relationship.
#[tokio::test]
async fn test_unfollow_updates_relationship() {
    let ctx = TestContext::new("rel-unfollow").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let resp = ctx.api.post_json(
        &format!("/api/v1/accounts/{}/unfollow", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let rel: Value = resp.json().await.unwrap();
    assert_eq!(rel["following"].as_bool(), Some(false));
}

/// Blocking sets blocking=true; unblocking sets it back to false.
#[tokio::test]
async fn test_block_and_unblock() {
    let ctx = TestContext::new("block").await;

    let block_resp = ctx.api.post_json(
        &format!("/api/v1/accounts/{}/block", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(block_resp.status(), StatusCode::OK);
    let rel: Value = block_resp.json().await.unwrap();
    assert_eq!(rel["blocking"].as_bool(), Some(true));

    let unblock_resp = ctx.api.post_json(
        &format!("/api/v1/accounts/{}/unblock", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(unblock_resp.status(), StatusCode::OK);
    let rel2: Value = unblock_resp.json().await.unwrap();
    assert_eq!(rel2["blocking"].as_bool(), Some(false));
}

/// Muting sets muting=true; unmuting sets it back to false.
#[tokio::test]
async fn test_mute_and_unmute() {
    let ctx = TestContext::new("mute").await;

    let mute_resp = ctx.api.post_json(
        &format!("/api/v1/accounts/{}/mute", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(mute_resp.status(), StatusCode::OK);
    let rel: Value = mute_resp.json().await.unwrap();
    assert_eq!(rel["muting"].as_bool(), Some(true));

    let unmute_resp = ctx.api.post_json(
        &format!("/api/v1/accounts/{}/unmute", ctx.bob_id),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(unmute_resp.status(), StatusCode::OK);
    let rel2: Value = unmute_resp.json().await.unwrap();
    assert_eq!(rel2["muting"].as_bool(), Some(false));
}

// ── tests: follow requests ───────────────────────────────────────────────────

/// Accepting a pending follow request changes the relationship to following=true.
#[tokio::test]
async fn test_authorize_follow_request() {
    let ctx = TestContext::new("follow-req-accept").await;

    let db_url = std::env::var("DATABASE_URL").unwrap();
    let db = PgPoolOptions::new().max_connections(2).connect(&db_url).await.unwrap();
    let bob_uuid: Uuid = ctx.bob_id.parse().unwrap();
    sqlx::query!("UPDATE accounts SET locked = true WHERE id = $1", bob_uuid)
        .execute(&db).await.unwrap();

    // Alice follows locked Bob → pending.
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    // Bob authorises Alice's follow request.
    let requests_resp = ctx.api.get("/api/v1/follow_requests", Some(&ctx.bob_token)).await;
    let requests: Vec<Value> = requests_resp.json().await.unwrap();
    assert!(!requests.is_empty(), "no pending follow requests");
    let requester_id = requests[0]["id"].as_str().unwrap().to_string();

    let accept_resp = ctx.api.post_json(
        &format!("/api/v1/follow_requests/{requester_id}/authorize"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(accept_resp.status(), StatusCode::OK);

    // Alice is now following Bob.
    let rels: Vec<Value> = ctx.api.get(
        &format!("/api/v1/accounts/relationships?id[]={}", ctx.bob_id),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    assert_eq!(rels[0]["following"].as_bool(), Some(true));
    assert_eq!(rels[0]["requested"].as_bool(), Some(false));
}

/// Rejecting a pending follow request leaves following=false, requested=false.
#[tokio::test]
async fn test_reject_follow_request() {
    let ctx = TestContext::new("follow-req-reject").await;

    let db_url = std::env::var("DATABASE_URL").unwrap();
    let db = PgPoolOptions::new().max_connections(2).connect(&db_url).await.unwrap();
    let bob_uuid: Uuid = ctx.bob_id.parse().unwrap();
    sqlx::query!("UPDATE accounts SET locked = true WHERE id = $1", bob_uuid)
        .execute(&db).await.unwrap();

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let requests: Vec<Value> = ctx.api.get("/api/v1/follow_requests", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    let requester_id = requests[0]["id"].as_str().unwrap().to_string();

    let reject_resp = ctx.api.post_json(
        &format!("/api/v1/follow_requests/{requester_id}/reject"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(reject_resp.status(), StatusCode::OK);

    let rels: Vec<Value> = ctx.api.get(
        &format!("/api/v1/accounts/relationships?id[]={}", ctx.bob_id),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    assert_eq!(rels[0]["following"].as_bool(), Some(false));
    assert_eq!(rels[0]["requested"].as_bool(), Some(false));
}

// ── tests: status thread & context ──────────────────────────────────────────

/// A reply has in_reply_to_id set to the parent status id.
#[tokio::test]
async fn test_reply_sets_in_reply_to_id() {
    let ctx = TestContext::new("reply-id").await;

    let parent = ctx.api.post_status(&ctx.alice_token, "parent post", "public").await;
    let parent_id = parent["id"].as_str().unwrap();

    let reply_resp = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.bob_token),
        &json!({"status": "reply text", "in_reply_to_id": parent_id, "visibility": "public"}),
    ).await;
    assert_eq!(reply_resp.status(), StatusCode::OK);
    let reply: Value = reply_resp.json().await.unwrap();
    assert_eq!(reply["in_reply_to_id"].as_str(), Some(parent_id));
}

/// GET /api/v1/statuses/:id/context returns ancestors and descendants.
#[tokio::test]
async fn test_status_context_ancestors_and_descendants() {
    let ctx = TestContext::new("ctx-thread").await;

    let grandparent = ctx.api.post_status(&ctx.alice_token, "grandparent", "public").await;
    let gp_id = grandparent["id"].as_str().unwrap();

    let parent: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.bob_token),
        &json!({"status": "parent", "in_reply_to_id": gp_id, "visibility": "public"}),
    ).await.json().await.unwrap();
    let p_id = parent["id"].as_str().unwrap();

    let child: Value = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({"status": "child", "in_reply_to_id": p_id, "visibility": "public"}),
    ).await.json().await.unwrap();
    let c_id = child["id"].as_str().unwrap();

    let resp = ctx.api.get(&format!("/api/v1/statuses/{p_id}/context"), None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let ctx_body: Value = resp.json().await.unwrap();

    let ancestor_ids: Vec<&str> = ctx_body["ancestors"]
        .as_array().unwrap().iter()
        .filter_map(|s| s["id"].as_str())
        .collect();
    let descendant_ids: Vec<&str> = ctx_body["descendants"]
        .as_array().unwrap().iter()
        .filter_map(|s| s["id"].as_str())
        .collect();

    assert!(ancestor_ids.contains(&gp_id), "grandparent not in ancestors");
    assert!(descendant_ids.contains(&c_id), "child not in descendants");
}

// ── tests: status edit & history ────────────────────────────────────────────

/// Editing a status changes its content.
#[tokio::test]
async fn test_edit_status_changes_content() {
    let ctx = TestContext::new("edit-content").await;

    let status = ctx.api.post_status(&ctx.alice_token, "original text", "public").await;
    let id = status["id"].as_str().unwrap();

    let edit_resp = ctx.api.put_json(
        &format!("/api/v1/statuses/{id}"),
        Some(&ctx.alice_token),
        &json!({"status": "edited text", "visibility": "public"}),
    ).await;
    assert_eq!(edit_resp.status(), StatusCode::OK);
    let edited: Value = edit_resp.json().await.unwrap();
    // content is HTML — spaces may be encoded as &#32;
    let content = edited["content"].as_str().unwrap_or("");
    assert!(
        content.contains("edited"),
        "edited content not found: {content:?}"
    );
}

/// GET /api/v1/statuses/:id/history returns at least two entries after an edit.
#[tokio::test]
async fn test_status_history_after_edit() {
    let ctx = TestContext::new("edit-history").await;

    let status = ctx.api.post_status(&ctx.alice_token, "v1 text", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api.put_json(
        &format!("/api/v1/statuses/{id}"),
        Some(&ctx.alice_token),
        &json!({"status": "v2 text", "visibility": "public"}),
    ).await;

    let resp = ctx.api.get(&format!("/api/v1/statuses/{id}/history"), None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let history: Vec<Value> = resp.json().await.unwrap();
    assert!(history.len() >= 2, "expected at least 2 history entries, got {}", history.len());
}

/// GET /api/v1/statuses/:id/source returns the original plaintext.
#[tokio::test]
async fn test_status_source_returns_text() {
    let ctx = TestContext::new("status-src").await;

    let status = ctx.api.post_status(&ctx.alice_token, "source text here", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.get(&format!("/api/v1/statuses/{id}/source"), Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["text"].as_str().unwrap_or("").contains("source text here"),
        "source text not returned"
    );
}

/// Only the author can fetch status source; stranger gets 403.
#[tokio::test]
async fn test_status_source_forbidden_for_non_author() {
    let ctx = TestContext::new("status-src-403").await;

    let status = ctx.api.post_status(&ctx.alice_token, "alice's text", "public").await;
    let id = status["id"].as_str().unwrap();

    let resp = ctx.api.get(&format!("/api/v1/statuses/{id}/source"), Some(&ctx.bob_token)).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// Status content warning (spoiler_text) round-trips correctly.
#[tokio::test]
async fn test_spoiler_text_preserved() {
    let ctx = TestContext::new("cw").await;

    let resp = ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.alice_token),
        &json!({"status": "body text", "spoiler_text": "content warning", "visibility": "public"}),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let status: Value = resp.json().await.unwrap();
    assert_eq!(status["spoiler_text"].as_str(), Some("content warning"));
}

// ── tests: bookmarks ────────────────────────────────────────────────────────

/// Bookmarking and unbookmarking a status.
#[tokio::test]
async fn test_bookmark_and_unbookmark() {
    let ctx = TestContext::new("bookmark").await;

    let status = ctx.api.post_status(&ctx.alice_token, "bookmarkable", "public").await;
    let id = status["id"].as_str().unwrap();

    let bk_resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/bookmark"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(bk_resp.status(), StatusCode::OK);
    let bk: Value = bk_resp.json().await.unwrap();
    assert_eq!(bk["bookmarked"].as_bool(), Some(true));

    let ubk_resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/unbookmark"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(ubk_resp.status(), StatusCode::OK);
    let ubk: Value = ubk_resp.json().await.unwrap();
    assert_eq!(ubk["bookmarked"].as_bool(), Some(false));
}

/// GET /api/v1/bookmarks returns the bookmarked status.
#[tokio::test]
async fn test_bookmarks_list() {
    let ctx = TestContext::new("bk-list").await;

    let status = ctx.api.post_status(&ctx.alice_token, "to bookmark", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/bookmark"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    let resp = ctx.api.get("/api/v1/bookmarks", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(list.iter().any(|s| s["id"].as_str() == Some(id)), "bookmarked status not in list");
}

// ── tests: pin / unpin ───────────────────────────────────────────────────────

/// Pinning and unpinning a status updates pinned field.
#[tokio::test]
async fn test_pin_and_unpin() {
    let ctx = TestContext::new("pin").await;

    let status = ctx.api.post_status(&ctx.alice_token, "pinnable", "public").await;
    let id = status["id"].as_str().unwrap();

    let pin_resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/pin"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(pin_resp.status(), StatusCode::OK);
    let pinned: Value = pin_resp.json().await.unwrap();
    assert_eq!(pinned["pinned"].as_bool(), Some(true));

    let unpin_resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/unpin"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;
    assert_eq!(unpin_resp.status(), StatusCode::OK);
    let unpinned: Value = unpin_resp.json().await.unwrap();
    assert_eq!(unpinned["pinned"].as_bool(), Some(false));
}

// ── tests: favourited_by / reblogged_by ──────────────────────────────────────

/// GET /api/v1/statuses/:id/favourited_by includes the account that favourited.
#[tokio::test]
async fn test_favourited_by_list() {
    let ctx = TestContext::new("fav-by").await;

    let status = ctx.api.post_status(&ctx.alice_token, "fav me", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/favourite"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;

    let resp = ctx.api.get(&format!("/api/v1/statuses/{id}/favourited_by"), None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(list.iter().any(|a| a["id"].as_str() == Some(ctx.bob_id.as_str())));
}

/// GET /api/v1/statuses/:id/reblogged_by includes the account that reblogged.
#[tokio::test]
async fn test_reblogged_by_list() {
    let ctx = TestContext::new("rb-by").await;

    let status = ctx.api.post_status(&ctx.alice_token, "reblog me", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/reblog"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;

    let resp = ctx.api.get(&format!("/api/v1/statuses/{id}/reblogged_by"), None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(list.iter().any(|a| a["id"].as_str() == Some(ctx.bob_id.as_str())));
}

/// Unreblogging decrements reblogs_count to zero.
#[tokio::test]
async fn test_unreblog() {
    let ctx = TestContext::new("unreblog").await;

    let status = ctx.api.post_status(&ctx.alice_token, "unreblog me", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/reblog"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;

    let unrb_resp = ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/unreblog"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(unrb_resp.status(), StatusCode::OK);

    let updated: Value = ctx.api.get(&format!("/api/v1/statuses/{id}"), None)
        .await.json().await.unwrap();
    assert_eq!(updated["reblogs_count"].as_i64().unwrap_or(-1), 0);
}

// ── tests: notifications ────────────────────────────────────────────────────

/// Following an account creates a notification for the target.
#[tokio::test]
async fn test_follow_creates_notification() {
    let ctx = TestContext::new("notif-follow").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let resp = ctx.api.get("/api/v1/notifications", Some(&ctx.bob_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let notifs: Vec<Value> = resp.json().await.unwrap();

    let follow_notif = notifs.iter().find(|n| n["type"].as_str() == Some("follow"));
    assert!(follow_notif.is_some(), "no follow notification found");
    assert_eq!(
        follow_notif.unwrap()["account"]["id"].as_str(),
        Some(ctx.alice_id.as_str()),
        "follow notification has wrong source account"
    );
}

/// Favouriting a status creates a notification for the status author.
#[tokio::test]
async fn test_favourite_creates_notification() {
    let ctx = TestContext::new("notif-fav").await;

    let status = ctx.api.post_status(&ctx.alice_token, "notify on fav", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/favourite"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;

    let notifs: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.alice_token))
        .await.json().await.unwrap();

    let fav_notif = notifs.iter().find(|n| n["type"].as_str() == Some("favourite"));
    assert!(fav_notif.is_some(), "no favourite notification found");
}

/// Replying to a status creates a mention notification for the parent's author.
#[tokio::test]
async fn test_reply_creates_mention_notification() {
    let ctx = TestContext::new("notif-mention").await;

    let parent = ctx.api.post_status(&ctx.alice_token, "parent status", "public").await;
    let parent_id = parent["id"].as_str().unwrap();

    ctx.api.post_json(
        "/api/v1/statuses",
        Some(&ctx.bob_token),
        &json!({"status": "reply here", "in_reply_to_id": parent_id, "visibility": "public"}),
    ).await;

    let notifs: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.alice_token))
        .await.json().await.unwrap();

    let mention = notifs.iter().find(|n| n["type"].as_str() == Some("mention"));
    assert!(mention.is_some(), "no mention notification found");
}

/// Dismissed notification no longer appears in the list.
#[tokio::test]
async fn test_dismiss_notification() {
    let ctx = TestContext::new("notif-dismiss").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let notifs: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    let notif_id = notifs[0]["id"].as_str().unwrap().to_string();

    let dismiss_resp = ctx.api.post_json(
        &format!("/api/v1/notifications/{notif_id}/dismiss"),
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(dismiss_resp.status(), StatusCode::OK);

    let after: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(!after.iter().any(|n| n["id"].as_str() == Some(notif_id.as_str())));
}

/// Clearing notifications empties the list.
#[tokio::test]
async fn test_clear_notifications() {
    let ctx = TestContext::new("notif-clear").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let clear_resp = ctx.api.post_json(
        "/api/v1/notifications/clear",
        Some(&ctx.bob_token),
        &json!({}),
    ).await;
    assert_eq!(clear_resp.status(), StatusCode::OK);

    let after: Vec<Value> = ctx.api.get("/api/v1/notifications", Some(&ctx.bob_token))
        .await.json().await.unwrap();
    assert!(after.is_empty(), "notifications not cleared");
}

// ── tests: lists ────────────────────────────────────────────────────────────

/// Full list CRUD: create → get → update → delete.
#[tokio::test]
async fn test_list_crud() {
    let ctx = TestContext::new("list-crud").await;

    // Create
    let create_resp = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.alice_token),
        &json!({"title": "My List"}),
    ).await;
    assert_eq!(create_resp.status(), StatusCode::OK);
    let list: Value = create_resp.json().await.unwrap();
    let list_id = list["id"].as_str().unwrap().to_string();
    assert_eq!(list["title"].as_str(), Some("My List"));

    // Get
    let get_resp = ctx.api.get(&format!("/api/v1/lists/{list_id}"), Some(&ctx.alice_token)).await;
    assert_eq!(get_resp.status(), StatusCode::OK);

    // Update
    let update_resp = ctx.api.put_json(
        &format!("/api/v1/lists/{list_id}"),
        Some(&ctx.alice_token),
        &json!({"title": "Renamed List"}),
    ).await;
    assert_eq!(update_resp.status(), StatusCode::OK);
    let updated: Value = update_resp.json().await.unwrap();
    assert_eq!(updated["title"].as_str(), Some("Renamed List"));

    // Delete
    let del_resp = ctx.api.delete(&format!("/api/v1/lists/{list_id}"), &ctx.alice_token).await;
    assert_eq!(del_resp.status(), StatusCode::OK);

    // Confirm deleted
    let gone_resp = ctx.api.get(&format!("/api/v1/lists/{list_id}"), Some(&ctx.alice_token)).await;
    assert_eq!(gone_resp.status(), StatusCode::NOT_FOUND);
}

/// Adding and removing accounts from a list.
#[tokio::test]
async fn test_list_add_and_remove_accounts() {
    let ctx = TestContext::new("list-accts").await;

    // Alice must follow Bob to add him to a list.
    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let list: Value = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.alice_token),
        &json!({"title": "Friends"}),
    ).await.json().await.unwrap();
    let list_id = list["id"].as_str().unwrap();

    // Add Bob
    let add_resp = ctx.api.post_json(
        &format!("/api/v1/lists/{list_id}/accounts"),
        Some(&ctx.alice_token),
        &json!({"account_ids": [ctx.bob_id]}),
    ).await;
    assert_eq!(add_resp.status(), StatusCode::OK);

    let members: Vec<Value> = ctx.api.get(
        &format!("/api/v1/lists/{list_id}/accounts"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    assert!(members.iter().any(|a| a["id"].as_str() == Some(ctx.bob_id.as_str())));

    // Remove Bob
    let remove_resp = ctx.api.http
        .delete(ctx.api.url(&format!("/api/v1/lists/{list_id}/accounts")))
        .header("host", &ctx.api.host)
        .bearer_auth(&ctx.alice_token)
        .json(&json!({"account_ids": [ctx.bob_id]}))
        .send().await.unwrap();
    assert_eq!(remove_resp.status(), StatusCode::OK);

    let after: Vec<Value> = ctx.api.get(
        &format!("/api/v1/lists/{list_id}/accounts"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();
    assert!(!after.iter().any(|a| a["id"].as_str() == Some(ctx.bob_id.as_str())));
}

/// List timeline includes posts from accounts added to the list.
#[tokio::test]
async fn test_list_timeline() {
    let ctx = TestContext::new("list-timeline").await;

    ctx.api.follow(&ctx.alice_token, &ctx.bob_id).await;

    let list: Value = ctx.api.post_json(
        "/api/v1/lists",
        Some(&ctx.alice_token),
        &json!({"title": "Watch"}),
    ).await.json().await.unwrap();
    let list_id = list["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/lists/{list_id}/accounts"),
        Some(&ctx.alice_token),
        &json!({"account_ids": [ctx.bob_id]}),
    ).await;

    let bob_status = ctx.api.post_status(&ctx.bob_token, "list-only post", "public").await;
    let bob_id = bob_status["id"].as_str().unwrap();

    let timeline: Vec<Value> = ctx.api.get(
        &format!("/api/v1/timelines/list/{list_id}"),
        Some(&ctx.alice_token),
    ).await.json().await.unwrap();

    assert!(timeline.iter().any(|s| s["id"].as_str() == Some(bob_id)));
}

// ── tests: favourites list ───────────────────────────────────────────────────

/// GET /api/v1/favourites returns statuses the user has favourited.
#[tokio::test]
async fn test_favourites_list() {
    let ctx = TestContext::new("fav-list").await;

    let status = ctx.api.post_status(&ctx.bob_token, "fav-list post", "public").await;
    let id = status["id"].as_str().unwrap();

    ctx.api.post_json(
        &format!("/api/v1/statuses/{id}/favourite"),
        Some(&ctx.alice_token),
        &json!({}),
    ).await;

    let resp = ctx.api.get("/api/v1/favourites", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list: Vec<Value> = resp.json().await.unwrap();
    assert!(list.iter().any(|s| s["id"].as_str() == Some(id)));
}

// ── tests: pagination ────────────────────────────────────────────────────────

/// max_id pagination returns only statuses older than the given id.
#[tokio::test]
async fn test_public_timeline_max_id_pagination() {
    let ctx = TestContext::new("paginate-max").await;

    let s1 = ctx.api.post_status(&ctx.alice_token, "paginate-a", "public").await;
    let s2 = ctx.api.post_status(&ctx.alice_token, "paginate-b", "public").await;
    let s3 = ctx.api.post_status(&ctx.alice_token, "paginate-c", "public").await;

    let s2_id = s2["id"].as_str().unwrap();
    let s1_id = s1["id"].as_str().unwrap();
    let s3_id = s3["id"].as_str().unwrap();

    // max_id=s2 should return only s1 (older), not s2 or s3.
    let timeline: Vec<Value> = ctx.api.get(
        &format!("/api/v1/timelines/public?local=true&max_id={s2_id}"),
        None,
    ).await.json().await.unwrap();

    let ids: Vec<&str> = timeline.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(ids.contains(&s1_id), "s1 missing from max_id page");
    assert!(!ids.contains(&s2_id), "s2 should not appear with max_id=s2");
    assert!(!ids.contains(&s3_id), "s3 should not appear with max_id=s2");
}

/// Paginated response includes a Link header with rel="next" and rel="prev".
#[tokio::test]
async fn test_public_timeline_link_header() {
    let ctx = TestContext::new("paginate-link").await;

    // Post enough statuses to trigger pagination.
    for i in 0..5 {
        ctx.api.post_status(&ctx.alice_token, &format!("link-header-test {i}"), "public").await;
    }

    let resp = ctx.api.get("/api/v1/timelines/public?local=true&limit=2", None).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let link = resp.headers().get("link").and_then(|v| v.to_str().ok()).unwrap_or("");
    assert!(link.contains("rel=\"next\""), "Link header missing rel=next: {link}");
    assert!(link.contains("rel=\"prev\""), "Link header missing rel=prev: {link}");
}

// ── tests: search ────────────────────────────────────────────────────────────

/// Search by username finds the matching account.
#[tokio::test]
async fn test_search_accounts() {
    let ctx = TestContext::new("search-acct").await;

    let resp = ctx.api.get("/api/v2/search?q=alice&type=accounts", Some(&ctx.alice_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    let accounts = body["accounts"].as_array().unwrap();
    assert!(accounts.iter().any(|a| a["username"].as_str() == Some("alice")));
}

/// Search for a status by its text returns the matching status.
#[tokio::test]
async fn test_search_statuses() {
    let ctx = TestContext::new("search-status").await;

    ctx.api.post_status(&ctx.alice_token, "uniqueterm12345", "public").await;

    let resp = ctx.api.get(
        "/api/v2/search?q=uniqueterm12345&type=statuses",
        Some(&ctx.alice_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    let statuses = body["statuses"].as_array().unwrap();
    assert!(
        statuses.iter().any(|s| s["content"].as_str().unwrap_or("").contains("uniqueterm12345")
            || s["text"].as_str().unwrap_or("").contains("uniqueterm12345")),
        "search did not find status with uniqueterm12345"
    );
}

// ── tests: instance info ─────────────────────────────────────────────────────

/// GET /api/v1/instance returns valid instance data.
#[tokio::test]
async fn test_instance_v1() {
    let ctx = TestContext::new("instance-v1").await;

    let resp = ctx.api.get("/api/v1/instance", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    assert!(body["uri"].as_str().is_some(), "uri field missing");
    assert!(body["title"].as_str().is_some(), "title field missing");
    assert!(body["version"].as_str().is_some(), "version field missing");
}

/// GET /api/v2/instance returns valid instance data including usage.
#[tokio::test]
async fn test_instance_v2() {
    let ctx = TestContext::new("instance-v2").await;

    let resp = ctx.api.get("/api/v2/instance", None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    assert!(body["domain"].as_str().is_some(), "domain field missing");
    assert!(body["version"].as_str().is_some(), "version field missing");
}

// ── tests: OAuth app registration ────────────────────────────────────────────

/// POST /api/v1/apps registers an application and returns credentials.
#[tokio::test]
async fn test_register_app() {
    let ctx = TestContext::new("oauth-app").await;

    let resp = ctx.api.post_json(
        "/api/v1/apps",
        None,
        &json!({
            "client_name": "Test App",
            "redirect_uris": "urn:ietf:wg:oauth:2.0:oob",
            "scopes": "read write"
        }),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    assert!(body["client_id"].as_str().is_some(), "client_id missing");
    assert!(body["client_secret"].as_str().is_some(), "client_secret missing");
    assert_eq!(body["name"].as_str(), Some("Test App"));
}
