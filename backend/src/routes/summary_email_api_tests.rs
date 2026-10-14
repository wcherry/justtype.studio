//! The summary email through the API: choosing it, what goes in it for a
//! writer with a book under review, that it goes once and only when something
//! happened, and the one-click way out.
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde_json::{json, Value};

use super::prompts_api_tests::{Server, Writer};
use crate::db::schema::*;

fn at(t: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(t).unwrap().with_timezone(&Utc)
}

/// Pretends `who` chose their setting at `when`, so a test clock in the past
/// or the future treats it as already in force.
fn chosen_at(server: &Server, who: &Writer, when: &str) {
    diesel::update(summary_email_settings::table.find(&who.id))
        .set(summary_email_settings::updated_at.eq(when))
        .execute(&mut server.conn())
        .unwrap();
}

fn outbox(server: &Server) -> Vec<Value> {
    std::fs::read_to_string(server.root.join("outbox.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|m| m["subject"].as_str().unwrap().contains("in writing"))
        .collect()
}

#[tokio::test]
async fn the_summary_says_what_happened_and_goes_once() {
    let s = Server::start().await;
    let ann = s.writer("ann").await;
    let bo = s.writer("bo").await;
    let cy = s.writer("cy").await;

    // Off until chosen; a nonsense choice or timezone is refused.
    let view = s.ok(&ann, "GET", "/me/summary-email", None).await;
    assert_eq!(view["often"], "none");
    assert_eq!(view["sendTime"], "07:00");
    for bad in [
        json!({ "often": "hourly" }),
        json!({ "often": "daily", "timezone": "Mars/Olympus" }),
    ] {
        assert_eq!(
            s.status(Some(&ann), "PATCH", "/me/summary-email", Some(bad))
                .await,
            400
        );
    }
    let view = s
        .ok(
            &ann,
            "PATCH",
            "/me/summary-email",
            Some(json!({ "often": "daily", "timezone": "Europe/London" })),
        )
        .await;
    assert_eq!(view["often"], "daily");
    assert_eq!(view["timezone"], "Europe/London");
    s.ok(
        &cy,
        "PATCH",
        "/me/summary-email",
        Some(json!({ "often": "daily", "timezone": "Europe/London" })),
    )
    .await;

    // An unconfirmed address cannot have one.
    diesel::update(users::table.find(&bo.id))
        .set(users::email_verified_at.eq(None::<String>))
        .execute(&mut s.conn())
        .unwrap();
    assert_eq!(
        s.status(
            Some(&bo),
            "PATCH",
            "/me/summary-email",
            Some(json!({ "often": "weekly" }))
        )
        .await,
        403
    );

    // Ann writes on Tuesday 20 October, her time, puts the scene up for
    // review, and Bo comments on it.
    let outline = s
        .ok(
            &ann,
            "POST",
            "/works",
            Some(json!({ "title": "Tide", "kind": "novel" })),
        )
        .await;
    let work = outline["id"].as_str().unwrap().to_string();
    let scene = outline["chapters"][0]["scenes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    s.ok(
        &ann,
        "PUT",
        &format!("/scenes/{scene}/content"),
        Some(json!({
            "content": super::prompts_api_tests::text_doc("The door was open and the rain came in."),
            "localDay": "2026-10-20",
        })),
    )
    .await;
    let review = s
        .ok(
            &ann,
            "POST",
            &format!("/works/{work}/reviews"),
            Some(json!({ "scope": "scene", "scopeRefId": scene, "audience": "everyone" })),
        )
        .await;
    let review = review["id"].as_str().unwrap().to_string();
    s.ok(
        &bo,
        "POST",
        &format!("/publications/{review}/comments"),
        Some(json!({ "body": "The rain   is <lovely>.", "sceneId": scene, "category": "praise" })),
    )
    .await;
    // Her to-do list: one ticked off that day, one still open.
    let tasks = format!("/works/{work}/tasks");
    s.ok(
        &ann,
        "POST",
        &tasks,
        Some(json!({ "body": "Make the door colder" })),
    )
    .await;
    let done = s
        .ok(
            &ann,
            "POST",
            &tasks,
            Some(json!({ "body": "Name the dog" })),
        )
        .await;
    s.ok(
        &ann,
        "PATCH",
        &format!("/tasks/{}", done["id"].as_str().unwrap()),
        Some(json!({ "state": "done" })),
    )
    .await;
    // All of it on that Tuesday, London time.
    let tuesday = "2026-10-20T12:00:00.000Z";
    diesel::update(comments::table)
        .set(comments::created_at.eq(tuesday))
        .execute(&mut s.conn())
        .unwrap();
    diesel::update(revision_tasks::table.filter(revision_tasks::done_at.is_not_null()))
        .set(revision_tasks::done_at.eq(tuesday))
        .execute(&mut s.conn())
        .unwrap();
    for w in [&ann, &cy] {
        chosen_at(&s, w, "2026-10-01T00:00:00.000Z");
    }

    // 06:59 BST on Wednesday: not yet. 07:00: Ann's goes; Cy did nothing, so
    // Cy's is recorded and not sent.
    let send = |t: &'static str| crate::summary_email::send_due(&s.state, at(t));
    assert_eq!(send("2026-10-21T05:59:00Z").await.unwrap(), 0);
    assert_eq!(send("2026-10-21T06:00:00Z").await.unwrap(), 1);
    assert_eq!(send("2026-10-21T09:00:00Z").await.unwrap(), 0, "once a day");
    let handled: Vec<(String, bool)> = summary_email_sends::table
        .inner_join(users::table)
        .order(users::display_name.asc())
        .select((users::display_name, summary_email_sends::queued))
        .load(&mut s.conn())
        .unwrap();
    assert_eq!(
        handled,
        [("ann".to_string(), true), ("cy".to_string(), false)]
    );

    crate::mail::deliver_due(&s.state.pool, &s.state.mailer)
        .await
        .unwrap();
    let mail = outbox(&s);
    assert_eq!(mail.len(), 1);
    let m = &mail[0];
    assert_eq!(m["to"], "ann@example.com");
    assert_eq!(
        m["subject"],
        "Your day in writing: 9 words, 1 new comment, 1 task done"
    );
    let body = m["body"].as_str().unwrap();
    for needle in [
        "Here is Tuesday 20 October.",
        "You wrote 9 words.",
        "Tide — 1 new comment",
        "bo: \"The rain is <lovely>.\"",
        &format!("http://justtype.test/review/{review}"),
        "Tide — 1 open, 1 done",
        "[ ] Make the door colder",
    ] {
        assert!(body.contains(needle), "missing {needle:?} in\n{body}");
    }
    assert!(
        !body.contains("Name the dog"),
        "done tasks are counted, not listed"
    );
    assert!(m["html"]
        .as_str()
        .unwrap()
        .contains("The rain is &lt;lovely&gt;."));

    // Switched to weekly on Thursday: last Monday's had gone by then, so the
    // first weekly one is the Monday after.
    s.ok(
        &ann,
        "PATCH",
        "/me/summary-email",
        Some(json!({ "often": "weekly" })),
    )
    .await;
    chosen_at(&s, &ann, "2026-10-22T10:00:00.000Z");
    assert_eq!(send("2026-10-22T11:00:00Z").await.unwrap(), 0);
    assert_eq!(
        send("2026-10-26T07:00:00Z").await.unwrap(),
        1,
        "Monday, 07:00 GMT"
    );
    assert!(summary_email_sends::table
        .find((&ann.id, "weekly:2026-10-19"))
        .select(summary_email_sends::queued)
        .first::<bool>(&mut s.conn())
        .unwrap());

    // One click, signed out, turns it off; a made-up token does not.
    crate::mail::deliver_due(&s.state.pool, &s.state.mailer)
        .await
        .unwrap();
    let mail = outbox(&s);
    let header = mail[1]["headers"]["List-Unsubscribe"].as_str().unwrap();
    let token = header.split("token=").nth(1).unwrap().trim_end_matches('>');
    let post = |q: String| {
        s.http
            .post(format!("{}/summary-email/unsubscribe?token={q}", s.base))
            .send()
    };
    assert_eq!(post(token.to_string()).await.unwrap().status(), 200);
    assert_eq!(post("nope".into()).await.unwrap().status(), 404);
    let view = s.ok(&ann, "GET", "/me/summary-email", None).await;
    assert_eq!(view["often"], "none");
}
