//! `eunha accounts create`, which is `tootctl accounts create`.
//!
//! An instance's first account is made this way rather than by signing up:
//! Mastodon's `mastodon:setup` and `tootctl accounts create --role Owner` both
//! write a confirmed account holding the seeded Owner role, and hand back a
//! random password to sign in with.

use reqwest::StatusCode;
use serde_json::{json, Value};

use eunha::accounts::{create_from_command, CreateOptions};

use crate::helpers::TestContext;

fn owner(username: &str) -> CreateOptions {
    CreateOptions {
        username: username.to_string(),
        email: format!("{username}@example.com"),
        role: Some("Owner".to_string()),
        confirmed: true,
        approve: true,
    }
}

async fn create(ctx: &TestContext, options: CreateOptions) -> anyhow::Result<String> {
    create_from_command(
        &ctx.db,
        ctx.state.encryptor.as_ref(),
        &ctx.state.instance,
        options,
    )
    .await
}

/// The roles `db/seeds/03_roles.rb` creates from `config/roles.yml`.
#[tokio::test]
async fn test_the_default_roles_are_seeded() {
    let ctx = TestContext::new("roles-seeded").await;
    let roles: Vec<(String, i32, i64, bool)> = sqlx::query_as(
        "SELECT name, position, permissions, highlighted FROM user_roles
         WHERE id <> -99 AND name IN ('Moderator', 'Admin', 'Owner')
         ORDER BY position",
    )
    .fetch_all(&ctx.db)
    .await
    .unwrap();

    let moderator = (1 << 3) | (1 << 2) | (1 << 20) | (1 << 10) | (1 << 4) | (1 << 8);
    let admin = moderator
        | (1 << 18)
        | (1 << 19)
        | (1 << 5)
        | (1 << 6)
        | (1 << 7)
        | (1 << 9)
        | (1 << 12)
        | (1 << 11)
        | (1 << 13)
        | (1 << 14)
        | (1 << 15)
        | (1 << 17);
    assert_eq!(
        roles,
        vec![
            ("Moderator".to_string(), 10, moderator, true),
            ("Admin".to_string(), 100, admin, true),
            ("Owner".to_string(), 1000, 1, true),
        ]
    );
}

/// The owner can sign in with the password printed for them, holds the Owner
/// role, and is the instance's contact — the highest-ranked local account.
#[tokio::test]
async fn test_an_owner_created_on_the_command_line_can_sign_in() {
    let ctx = TestContext::new("accounts-create-owner").await;
    let password = create(&ctx, owner("gardener")).await.unwrap();
    assert_eq!(password.len(), 32);

    let (confirmed, approved, role): (bool, bool, String) = sqlx::query_as(
        "SELECT u.confirmed_at IS NOT NULL, u.approved, r.name
         FROM users u JOIN accounts a ON a.id = u.account_id
         JOIN user_roles r ON r.id = u.role_id
         WHERE a.username = 'gardener' AND a.domain IS NULL",
    )
    .fetch_one(&ctx.db)
    .await
    .unwrap();
    assert!(confirmed);
    assert!(approved);
    assert_eq!(role, "Owner");

    let app: Value = ctx
        .api
        .post_json(
            "/api/v1/apps",
            None,
            &json!({
                "client_name": "Owner sign-in",
                "redirect_uris": "urn:ietf:wg:oauth:2.0:oob",
                "scopes": "read"
            }),
        )
        .await
        .json()
        .await
        .unwrap();
    let token = ctx
        .api
        .post_json(
            "/oauth/token",
            None,
            &json!({
                "grant_type": "password",
                "client_id": app["client_id"],
                "client_secret": app["client_secret"],
                "username": "gardener@example.com",
                "password": password,
                "scope": "read",
            }),
        )
        .await;
    assert_eq!(token.status(), StatusCode::OK);
    let token: Value = token.json().await.unwrap();

    let me: Value = ctx
        .api
        .get(
            "/api/v1/accounts/verify_credentials",
            token["access_token"].as_str(),
        )
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(me["username"], "gardener");
    assert_eq!(me["role"]["name"], "Owner");

    // Signing needs the key to have been sealed into `keypairs`.
    let key = eunha::federation::keypair::signing_key(
        &ctx.state,
        me["id"].as_str().unwrap().parse().unwrap(),
    )
    .await
    .unwrap();
    assert!(key.private_key.contains("PRIVATE KEY"));

    let instance: Value = ctx
        .api
        .get("/api/v2/instance", None)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(instance["contact"]["account"]["username"], "gardener");
}

/// Without `--approve`, an account is approved as a sign-up would be:
/// `User#set_approved` leaves it pending where the instance requires approval.
#[tokio::test]
async fn test_without_approve_an_approval_required_instance_leaves_it_pending() {
    let ctx = TestContext::with_approval_required("accounts-create-pending").await;
    create(
        &ctx,
        CreateOptions {
            approve: false,
            ..owner("pending")
        },
    )
    .await
    .unwrap();

    let approved: bool = sqlx::query_scalar(
        "SELECT u.approved FROM users u JOIN accounts a ON a.id = u.account_id
         WHERE a.username = 'pending'",
    )
    .fetch_one(&ctx.db)
    .await
    .unwrap();
    assert!(!approved);
}

/// The registration checks are bypassed, but the account's own validations are
/// not, and nothing is written when one fails.
#[tokio::test]
async fn test_invalid_accounts_are_refused() {
    let ctx = TestContext::new("accounts-create-invalid").await;

    let refusals = [
        (
            CreateOptions {
                confirmed: false,
                ..owner("unconfirmed")
            },
            "--confirmed",
        ),
        (owner("Alice"), "username has already been taken"),
        (owner("has-hyphen"), "letters, numbers and underscores"),
        (owner(&"a".repeat(31)), "too long"),
        (
            CreateOptions {
                email: "ALICE@test.invalid".to_string(),
                ..owner("another_alice")
            },
            "email has already been taken",
        ),
        (
            CreateOptions {
                email: "not-an-address".to_string(),
                ..owner("no_address")
            },
            "email is invalid",
        ),
        (
            CreateOptions {
                role: Some("Gardener".to_string()),
                ..owner("no_role")
            },
            "cannot find user role",
        ),
    ];

    for (options, message) in refusals {
        let username = options.username.clone();
        let error = create(&ctx, options)
            .await
            .expect_err(&format!("{username} should be refused"));
        assert!(
            error.to_string().contains(message),
            "{username}: expected `{message}`, got `{error}`"
        );
    }

    let local: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts WHERE domain IS NULL")
        .fetch_one(&ctx.db)
        .await
        .unwrap();
    assert_eq!(local, 2, "only alice and bob");
}
