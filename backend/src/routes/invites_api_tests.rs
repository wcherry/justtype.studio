//! Invitations to join: who gets mailed, and that the inviter cannot tell
//! which addresses already have accounts.
use diesel::prelude::*;
use serde_json::json;

use super::prompts_api_tests::Server;
use crate::db::schema::{mail_queue, users};

/// Invitations queued for `email`, as (subject, plain text, html).
fn invitations(server: &Server, email: &str) -> Vec<(String, String, Option<String>)> {
    mail_queue::table
        .filter(mail_queue::to_address.eq(email))
        .filter(mail_queue::subject.like("%invited you to Just Type Studio"))
        .select((mail_queue::subject, mail_queue::body, mail_queue::html))
        .load(&mut server.conn())
        .unwrap()
}

#[tokio::test]
async fn an_invitation_is_mailed_once_and_says_who_from() {
    let server = Server::start().await;
    let ann = server.writer("ann").await;
    let r = server
        .ok(
            &ann,
            "POST",
            "/invites",
            Some(json!({ "emails": [" Friend@Example.com ", "friend@example.com", ""] })),
        )
        .await;
    assert_eq!(r["invited"], json!(["friend@example.com"]));
    assert_eq!(r["remainingToday"], 19);

    let sent = invitations(&server, "friend@example.com");
    assert_eq!(sent.len(), 1);
    let (subject, body, html) = &sent[0];
    assert_eq!(subject, "ann invited you to Just Type Studio");
    assert!(
        body.contains("http://justtype.test/sign-in?invited=1"),
        "{body}"
    );
    assert!(html.as_deref().unwrap().contains("Make an account"));

    // Asking again is not a second invitation, and does not count.
    let r = server
        .ok(
            &ann,
            "POST",
            "/invites",
            Some(json!({ "emails": ["friend@example.com"] })),
        )
        .await;
    assert_eq!(r["remainingToday"], 19);
    assert_eq!(invitations(&server, "friend@example.com").len(), 1);

    // Someone else asking soon after does not mail them a second time either.
    let bo = server.writer("bo").await;
    server
        .ok(
            &bo,
            "POST",
            "/invites",
            Some(json!({ "emails": ["friend@example.com"] })),
        )
        .await;
    assert_eq!(invitations(&server, "friend@example.com").len(), 1);
}

#[tokio::test]
async fn a_member_is_not_mailed_and_the_inviter_cannot_tell() {
    let server = Server::start().await;
    let ann = server.writer("ann").await;
    server.writer("cy").await;
    let member = server
        .ok(
            &ann,
            "POST",
            "/invites",
            Some(json!({ "emails": ["cy@example.com"] })),
        )
        .await;
    let stranger = server
        .ok(
            &ann,
            "POST",
            "/invites",
            Some(json!({ "emails": ["dee@example.com"] })),
        )
        .await;
    assert!(invitations(&server, "cy@example.com").is_empty());
    assert_eq!(invitations(&server, "dee@example.com").len(), 1);
    // Same shape, and both counted against the day.
    assert_eq!(member["invited"], json!(["cy@example.com"]));
    assert_eq!(stranger["invited"], json!(["dee@example.com"]));
    assert_eq!(member["remainingToday"], 19);
    assert_eq!(stranger["remainingToday"], 18);
}

#[tokio::test]
async fn invitations_are_fenced() {
    let server = Server::start().await;
    let ann = server.writer("ann").await;

    // An unconfirmed address cannot invite.
    diesel::update(users::table.find(&ann.id))
        .set(users::email_verified_at.eq(None::<String>))
        .execute(&mut server.conn())
        .unwrap();
    let (status, _) = server
        .call(
            Some(&ann),
            "POST",
            "/invites",
            Some(json!({ "emails": ["x@example.com"] })),
        )
        .await;
    assert_eq!(status, 403);
    diesel::update(users::table.find(&ann.id))
        .set(users::email_verified_at.eq(Some(crate::clock::now_iso())))
        .execute(&mut server.conn())
        .unwrap();

    for (emails, why) in [
        (json!([]), "nothing to send"),
        (json!(["ann@example.com"]), "your own address"),
        (json!(["not an address"]), "not an address"),
        (json!(["a@b.com\r\nBcc: c@d.com"]), "a header in disguise"),
        (json!(["a@b@c.com"]), "two @"),
        (
            json!((0..11)
                .map(|i| format!("p{i}@example.com"))
                .collect::<Vec<_>>()),
            "too many at once",
        ),
    ] {
        let (status, r) = server
            .call(
                Some(&ann),
                "POST",
                "/invites",
                Some(json!({ "emails": emails })),
            )
            .await;
        assert_eq!(status, 400, "{why}: {r}");
    }
    assert_eq!(
        server
            .status(
                None,
                "POST",
                "/invites",
                Some(json!({ "emails": ["x@example.com"] }))
            )
            .await,
        401
    );

    // Twenty a day, and a batch that would cross the line is refused whole.
    let first: Vec<String> = (0..10).map(|i| format!("a{i}@example.com")).collect();
    let r = server
        .ok(&ann, "POST", "/invites", Some(json!({ "emails": first })))
        .await;
    assert_eq!(r["remainingToday"], 10);
    let second: Vec<String> = (0..9).map(|i| format!("b{i}@example.com")).collect();
    server
        .ok(&ann, "POST", "/invites", Some(json!({ "emails": second })))
        .await;
    let (status, r) = server
        .call(
            Some(&ann),
            "POST",
            "/invites",
            Some(json!({ "emails": ["c0@example.com", "c1@example.com"] })),
        )
        .await;
    assert_eq!(status, 429, "{r}");
    assert!(invitations(&server, "c0@example.com").is_empty());
    let r = server
        .ok(
            &ann,
            "POST",
            "/invites",
            Some(json!({ "emails": ["c0@example.com"] })),
        )
        .await;
    assert_eq!(r["remainingToday"], 0);
}

#[test]
fn the_inviters_name_is_escaped() {
    let m = super::invites::invite_message("http://x.test", "a@b.c", "<b>Eve</b>");
    let html = m.html.unwrap();
    assert!(!html.contains("<b>Eve"), "{html}");
    assert!(html.contains("&lt;b&gt;Eve"));
}
