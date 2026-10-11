//! The welcome email: queued once, the first time an address is proved, in
//! the same transaction as the proof.
use diesel::prelude::*;
use serde_json::{json, Value};

use super::prompts_api_tests::Server;
use crate::db::schema::{mail_queue, users};

/// Every verification link mailed to `email` so far, oldest first.
fn verify_tokens(server: &Server, email: &str) -> Vec<String> {
    let outbox = std::fs::read_to_string(server.root.join("outbox.jsonl")).unwrap_or_default();
    outbox
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|m| m["to"] == email)
        .filter_map(|m| {
            let body = m["body"].as_str()?.to_string();
            let start = body.find("verify?token=")? + "verify?token=".len();
            Some(body[start..].split_whitespace().next()?.to_string())
        })
        .collect()
}

/// Welcomes queued for `email`, as (plain text, html).
fn welcomes(server: &Server, email: &str) -> Vec<(String, Option<String>)> {
    mail_queue::table
        .filter(mail_queue::to_address.eq(email))
        .filter(mail_queue::subject.eq("Welcome to Just Type Studio"))
        .select((mail_queue::body, mail_queue::html))
        .load(&mut server.conn())
        .unwrap()
}

async fn register(server: &Server, name: &str) -> String {
    let (status, r) = server
        .call(
            None,
            "POST",
            "/auth/register",
            Some(json!({
                "email": format!("{name}@example.com"),
                "password": "correcthorse",
                "displayName": name,
                "acceptTerms": true,
            })),
        )
        .await;
    assert_eq!(status, 200, "{r}");
    r["accessToken"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn confirming_an_address_welcomes_once() {
    let server = Server::start().await;
    let email = "cy@example.com";
    let access = register(&server, "cy").await;
    assert!(
        welcomes(&server, email).is_empty(),
        "no welcome before confirming"
    );

    let verify = |token: String| {
        server.call(
            None,
            "POST",
            "/auth/verify",
            Some(json!({ "token": token })),
        )
    };
    let token = verify_tokens(&server, email).pop().unwrap();
    assert_eq!(verify(token.clone()).await.0, 200);
    let sent = welcomes(&server, email);
    assert_eq!(sent.len(), 1);
    let (text, html) = &sent[0];
    assert!(text.contains("Hello cy"), "{text}");
    assert!(text.contains("http://justtype.test/prompts"), "{text}");
    assert!(html
        .as_deref()
        .unwrap()
        .contains("href=\"http://justtype.test/\""));

    // The same link again is refused, and sends nothing.
    assert_ne!(verify(token).await.0, 200);
    assert_eq!(welcomes(&server, email).len(), 1);

    // An address that has to be proved again — as a changed one would — is
    // confirmed, but not welcomed a second time.
    diesel::update(users::table.filter(users::email.eq(email)))
        .set(users::email_verified_at.eq(None::<String>))
        .execute(&mut server.conn())
        .unwrap();
    let resent = server
        .http
        .post(format!("{}/me/verification", server.base))
        .bearer_auth(&access)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(resent, 200);
    let token = verify_tokens(&server, email).pop().unwrap();
    assert_eq!(verify(token).await.0, 200);
    assert_eq!(welcomes(&server, email).len(), 1);
}

#[tokio::test]
async fn proving_the_address_through_a_reset_welcomes_too() {
    let server = Server::start().await;
    let email = "dee@example.com";
    register(&server, "dee").await;
    let (status, _) = server
        .call(
            None,
            "POST",
            "/auth/password/forgot",
            Some(json!({ "email": email })),
        )
        .await;
    assert_eq!(status, 200);
    let outbox = std::fs::read_to_string(server.root.join("outbox.jsonl")).unwrap();
    let reset = outbox
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|m| m["to"] == email)
        .find_map(|m| {
            let body = m["body"].as_str()?.to_string();
            let start = body.find("reset?token=")? + "reset?token=".len();
            Some(body[start..].split_whitespace().next()?.to_string())
        })
        .expect("a reset link");
    let (status, r) = server
        .call(
            None,
            "POST",
            "/auth/password/reset",
            Some(json!({ "token": reset, "password": "batterystaple" })),
        )
        .await;
    assert_eq!(status, 200, "{r}");
    assert_eq!(welcomes(&server, email).len(), 1);
}

#[test]
fn the_welcome_escapes_the_name_in_html() {
    let m = super::account::welcome_message("https://j.test", "a@b.c", "<b>Ann</b>");
    assert!(m.body.contains("Hello <b>Ann</b>"));
    let html = m.html.unwrap();
    assert!(html.contains("&lt;b&gt;Ann&lt;/b&gt;"));
    assert!(!html.contains("<b>Ann"));
    assert!(m.headers.is_empty());
}
