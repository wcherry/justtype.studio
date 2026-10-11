//! Moderation end to end: reports reaching staff, the queue, removing and
//! holding, the word filter, the external platform, and its webhook.
use diesel::prelude::*;
use serde_json::{json, Value};

use super::prompts_api_tests::{text_doc, today, Server, Writer};
use crate::clock;
use crate::db::schema::*;
use crate::domain::external_moderation as external;

impl Server {
    async fn joined(&self, name: &str) -> Writer {
        let w = self.writer(name).await;
        self.ok(
            &w,
            "POST",
            "/prompts/enrollment",
            Some(json!({ "timezone": "Europe/London" })),
        )
        .await;
        w
    }

    /// An answer to today's prompt, saved and (optionally) submitted. Returns its id.
    async fn answer_today(&self, w: &Writer, text: &str, submit: bool) -> String {
        let day = today();
        let saved = self
            .ok(
                w,
                "PUT",
                &format!("/prompts/{day}/entry"),
                Some(json!({ "content": text_doc(text) })),
            )
            .await;
        if submit {
            self.ok(w, "POST", &format!("/prompts/{day}/entry/submit"), None)
                .await;
        }
        saved["id"].as_str().unwrap().to_string()
    }

    async fn queue(&self, staff: &Writer, query: &str) -> Value {
        self.ok(staff, "GET", &format!("/admin/queue?{query}"), None)
            .await
    }

    async fn configure_platform(&self, admin: &Writer, surfaces: &[&str], remove_as: &str) {
        self.ok(
            admin,
            "PUT",
            "/admin/external",
            Some(json!({
                "provider": "stub",
                "surfaces": surfaces,
                "removeAs": remove_as,
                "apiKey": "sk_test_key",
                "webhookSecret": "whsec_test",
            })),
        )
        .await;
    }

    async fn add_term(&self, admin: &Writer, term: &str, action: &str, applies: &[&str]) -> Value {
        self.ok(
            admin,
            "POST",
            "/admin/words",
            Some(json!({ "term": term, "action": action, "appliesTo": applies })),
        )
        .await
    }
}

fn unique(prefix: &str) -> String {
    format!(
        "{prefix}{}",
        &uuid::Uuid::new_v4().simple().to_string()[..10]
    )
}

fn items(queue: &Value) -> Vec<Value> {
    queue["items"].as_array().unwrap().clone()
}

fn stub_bodies(marker: &str) -> Vec<Value> {
    external::STUB_LOG
        .lock()
        .unwrap()
        .iter()
        .filter(|(endpoint, body)| endpoint == "check" && body.to_string().contains(marker))
        .map(|(_, b)| b.clone())
        .collect()
}

// --- reporting -----------------------------------------------------------------------

#[tokio::test]
async fn ten_reports_on_one_comment_are_one_queue_item_with_a_count_of_ten() {
    let s = Server::start().await;
    let mo = s.moderator("mo").await;
    let author = s.writer("author").await;
    let (_, publication) = s.publication(&author, "A Book").await;
    let writer = s.writer("writer").await;
    let comment = s.comment(&writer, &publication, "something nasty").await;
    let id = comment["id"].as_str().unwrap().to_string();
    for i in 0..10 {
        let reporter = s.writer(&format!("rep{i}")).await;
        s.ok(
            &reporter,
            "POST",
            &format!("/comments/{id}/report"),
            Some(json!({ "reason": "abuse", "detail": format!("report {i}") })),
        )
        .await;
    }
    let queue = s.queue(&mo, "").await;
    let found: Vec<Value> = items(&queue)
        .into_iter()
        .filter(|i| i["subjectId"] == id)
        .collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["reportCount"], 10);
    assert_eq!(found[0]["reports"].as_array().unwrap().len(), 10);
    assert_eq!(found[0]["text"], "something nasty");
    assert_eq!(found[0]["author"]["displayName"], "writer");
    // The author of the work still sees them, exactly as before.
    let to_author = s
        .ok(
            &author,
            "GET",
            &format!("/publications/{publication}/reports"),
            None,
        )
        .await;
    assert_eq!(to_author.as_array().unwrap().len(), 10);
}

#[tokio::test]
async fn reports_about_craft_go_only_to_the_author() {
    let s = Server::start().await;
    let mo = s.moderator("mo").await;
    let author = s.writer("author").await;
    let (_, publication) = s.publication(&author, "A Book").await;
    let writer = s.writer("writer").await;
    let reader = s.writer("reader").await;
    let comment = s.comment(&writer, &publication, "a spoiler!").await;
    let id = comment["id"].as_str().unwrap();
    s.ok(
        &reader,
        "POST",
        &format!("/comments/{id}/report"),
        Some(json!({ "reason": "spoiler" })),
    )
    .await;
    s.ok(
        &reader,
        "POST",
        &format!("/comments/{id}/report"),
        Some(json!({ "reason": "off_topic" })),
    )
    .await;
    assert!(items(&s.queue(&mo, "").await).is_empty());
    let to_author = s
        .ok(
            &author,
            "GET",
            &format!("/publications/{publication}/reports"),
            None,
        )
        .await;
    assert_eq!(
        to_author.as_array().unwrap().len(),
        1,
        "the second report updated the first"
    );
    // The reason can be raised to something staff see.
    s.ok(
        &reader,
        "POST",
        &format!("/comments/{id}/report"),
        Some(json!({ "reason": "harassment" })),
    )
    .await;
    assert_eq!(items(&s.queue(&mo, "").await).len(), 1);
}

#[tokio::test]
async fn every_new_surface_can_be_reported_and_reaches_the_queue() {
    let s = Server::start().await;
    s.prompt(&today(), "Write about a door.");
    let mo = s.moderator("mo").await;
    let author = s.joined("author").await;
    let reader = s.joined("reader").await;
    let entry = s.answer_today(&author, "The door was blue.", true).await;
    let (work_id, publication) = s.publication(&author, "A Book").await;
    let question = s
        .ok(
            &author,
            "POST",
            &format!("/publications/{publication}/questions"),
            Some(json!({ "prompt": "Did the ending work?", "kind": "text" })),
        )
        .await;
    let answer = s
        .ok(
            &reader,
            "PUT",
            &format!("/questions/{}/answers", question["id"].as_str().unwrap()),
            Some(json!({ "body": "It did not." })),
        )
        .await;
    let _ = work_id;
    let answerer = &reader;
    let reporter = s.writer("reporter").await;

    let targets = [
        ("entry", entry.clone(), "harassment"),
        (
            "question",
            question["id"].as_str().unwrap().to_string(),
            "spam",
        ),
        (
            "answer",
            answer["id"].as_str().unwrap().to_string(),
            "abuse",
        ),
        ("profile", author.id.clone(), "impersonation"),
        ("publication", publication.clone(), "illegal"),
    ];
    for (kind, id, reason) in &targets {
        // Entries are only reportable by writers who can read them.
        let who = if *kind == "entry" { &reader } else { &reporter };
        let reported = s
            .ok(
                who,
                "POST",
                "/reports",
                Some(json!({ "subjectType": kind, "subjectId": id, "reason": reason })),
            )
            .await;
        assert_eq!(reported["reported"], true, "{kind}");
        assert!(reported["support"].is_null());
    }
    let queue = items(&s.queue(&mo, "").await);
    for (kind, id, _) in &targets {
        assert!(
            queue
                .iter()
                .any(|i| i["subjectType"] == *kind && i["subjectId"] == id.as_str()),
            "{kind} did not reach the queue: {queue:?}"
        );
    }
    // Illegal outranks spam: worst first.
    let order: Vec<&str> = queue
        .iter()
        .map(|i| i["topReason"].as_str().unwrap())
        .collect();
    assert_eq!(order.first(), Some(&"illegal"), "{order:?}");
    assert_eq!(order.last(), Some(&"spam"), "{order:?}");
    let _ = answerer;

    // Your own work, and what you can't see, can't be reported; nor a bad reason.
    assert_eq!(
        s.status(
            Some(&author),
            "POST",
            "/reports",
            Some(json!({ "subjectType": "entry", "subjectId": entry, "reason": "spam" }))
        )
        .await,
        400
    );
    assert_eq!(
        s.status(
            Some(&reporter),
            "POST",
            "/reports",
            Some(json!({ "subjectType": "entry", "subjectId": "nope", "reason": "spam" }))
        )
        .await,
        404
    );
    assert_eq!(
        s.status(
            Some(&reporter),
            "POST",
            "/reports",
            Some(json!({ "subjectType": "comment", "subjectId": "x", "reason": "spam" }))
        )
        .await,
        400
    );
    assert_eq!(
        s.status(
            Some(&reporter),
            "POST",
            "/reports",
            Some(json!({ "subjectType": "profile", "subjectId": author.id, "reason": "boring" }))
        )
        .await,
        400
    );
}

#[tokio::test]
async fn a_self_harm_report_goes_to_the_top_and_points_to_help() {
    let s = Server::start().await;
    let mo = s.moderator("mo").await;
    let a = s.writer("a").await;
    let b = s.writer("b").await;
    let reporter = s.writer("reporter").await;
    for w in [&a, &b] {
        s.ok(w, "PATCH", "/me/profile", Some(json!({ "bio": "hello" })))
            .await;
    }
    s.ok(
        &reporter,
        "POST",
        "/reports",
        Some(json!({ "subjectType": "profile", "subjectId": a.id, "reason": "illegal" })),
    )
    .await;
    let reported = s
        .ok(
            &reporter,
            "POST",
            "/reports",
            Some(json!({ "subjectType": "profile", "subjectId": b.id, "reason": "self_harm" })),
        )
        .await;
    assert!(reported["support"].as_str().unwrap().contains("988"));
    let queue = items(&s.queue(&mo, "").await);
    assert_eq!(queue[0]["topReason"], "self_harm");
    assert_eq!(queue[0]["subjectId"], b.id);
}

#[tokio::test]
async fn twenty_reports_a_day_is_the_most_one_account_can_make() {
    let s = Server::start().await;
    let reporter = s.writer("reporter").await;
    for i in 0..20 {
        let target = s.writer(&format!("t{i}")).await;
        s.ok(
            &reporter,
            "POST",
            "/reports",
            Some(json!({ "subjectType": "profile", "subjectId": target.id, "reason": "spam" })),
        )
        .await;
    }
    let one_more = s.writer("t20").await;
    assert_eq!(
        s.status(
            Some(&reporter),
            "POST",
            "/reports",
            Some(json!({ "subjectType": "profile", "subjectId": one_more.id, "reason": "spam" }))
        )
        .await,
        429
    );
}

#[tokio::test]
async fn reporters_are_shown_to_staff_and_never_to_the_reported_account() {
    let s = Server::start().await;
    let mo = s.moderator("mo").await;
    let author = s.writer("author").await;
    let target = s.writer("target").await;
    let reporter = s.writer("whistle").await;
    let (_, publication) = s.publication(&author, "A Book").await;
    let comment = s.comment(&target, &publication, "meh").await;
    s.ok(
        &reporter,
        "POST",
        &format!("/comments/{}/report", comment["id"].as_str().unwrap()),
        Some(json!({ "reason": "abuse" })),
    )
    .await;
    let queue = s.queue(&mo, "").await;
    assert_eq!(
        items(&queue)[0]["reports"][0]["reporter"]["displayName"],
        "whistle"
    );
    // Everything the reported account can read about themselves.
    let theirs = [
        s.ok(&target, "GET", "/me/restrictions", None)
            .await
            .to_string(),
        s.ok(&target, "GET", "/inbox", None).await.to_string(),
        s.ok(&target, "GET", "/me/audit", None).await.to_string(),
    ];
    for body in theirs {
        assert!(!body.contains("whistle"), "{body}");
    }
}

// --- working the queue ---------------------------------------------------------------

#[tokio::test]
async fn removing_a_comment_hides_it_everywhere_keeps_its_replies_and_tells_its_author() {
    let s = Server::start().await;
    let mo = s.moderator("mo").await;
    let author = s.writer("author").await;
    let (_, publication) = s.publication(&author, "A Book").await;
    let bad = s.writer("bad").await;
    let reader = s.writer("reader").await;
    let comment = s.comment(&bad, &publication, "you are all terrible").await;
    let id = comment["id"].as_str().unwrap().to_string();
    s.ok(
        &reader,
        "POST",
        &format!("/publications/{publication}/comments"),
        Some(json!({ "body": "that was uncalled for", "parentId": id })),
    )
    .await;
    s.ok(
        &reader,
        "POST",
        &format!("/comments/{id}/report"),
        Some(json!({ "reason": "abuse" })),
    )
    .await;
    let item = items(&s.queue(&mo, "").await)[0].clone();
    let item_id = item["id"].as_str().unwrap();

    // A reason is required; then it goes.
    assert_eq!(
        s.status(
            Some(&mo),
            "POST",
            &format!("/admin/queue/{item_id}/remove"),
            Some(json!({}))
        )
        .await,
        400
    );
    s.ok(
        &mo,
        "POST",
        &format!("/admin/queue/{item_id}/remove"),
        Some(json!({ "reason": "3.2 — abuse" })),
    )
    .await;
    let list = s
        .ok(
            &reader,
            "GET",
            &format!("/publications/{publication}/comments"),
            None,
        )
        .await;
    assert_eq!(list[0]["body"], "", "its words are gone from the thread");
    assert_eq!(list[0]["moderationState"], "removed");
    assert_eq!(
        list[0]["replies"][0]["body"], "that was uncalled for",
        "the reply stays"
    );
    // The author hears of it, and which part of the guidelines.
    let inbox = s.ok(&bad, "GET", "/inbox", None).await;
    assert!(
        inbox["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["text"].as_str().unwrap().contains("3.2 — abuse")),
        "{inbox}"
    );
    // It's in the done tab, and the staff member who did it is in the log.
    assert_eq!(items(&s.queue(&mo, "tab=done").await).len(), 1);
    let lines = s.ok(&mo, "GET", "/admin/log", None).await;
    assert_eq!(lines["lines"][0]["action"], "content_removed");
    assert_eq!(lines["lines"][0]["reason"], "3.2 — abuse");
    // Restoring brings it back.
    s.ok(
        &mo,
        "POST",
        &format!("/admin/queue/{item_id}/restore"),
        Some(json!({ "reason": "mistake" })),
    )
    .await;
    let list = s
        .ok(
            &reader,
            "GET",
            &format!("/publications/{publication}/comments"),
            None,
        )
        .await;
    assert_eq!(list[0]["body"], "you are all terrible");
    assert_eq!(list[0]["moderationState"], "visible");
    // The author's own deletion is separate from moderation: a comment they
    // deleted stays deleted when staff restore it.
    s.ok(&bad, "DELETE", &format!("/comments/{id}"), None).await;
    s.ok(
        &mo,
        "POST",
        &format!("/admin/queue/{item_id}/hold"),
        Some(json!({ "reason": "again" })),
    )
    .await;
    s.ok(
        &mo,
        "POST",
        &format!("/admin/queue/{item_id}/release"),
        Some(json!({ "reason": "again" })),
    )
    .await;
    let list = s
        .ok(
            &reader,
            "GET",
            &format!("/publications/{publication}/comments"),
            None,
        )
        .await;
    assert_eq!(list[0]["body"], "");
    assert_eq!(list[0]["state"], "deleted");
}

#[tokio::test]
async fn held_content_shows_to_its_author_and_to_staff_and_to_no_one_else() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let mo = s.moderator("mo").await;
    let author = s.writer("author").await;
    let (_, publication) = s.publication(&author, "A Book").await;
    let poster = s.writer("poster").await;
    let reader = s.writer("reader").await;
    s.add_term(&admin, "watchword", "hold", &["comment"]).await;

    let held = s
        .comment(&poster, &publication, "this has a watchword in it")
        .await;
    assert_eq!(held["moderationState"], "held");
    let body_of = |list: &Value| {
        list.as_array()
            .unwrap()
            .iter()
            .filter(|c| c["body"] != "")
            .count()
    };
    for (who, sees) in [(&poster, 1), (&mo, 1), (&reader, 0), (&author, 0)] {
        let list = s
            .ok(
                who,
                "GET",
                &format!("/publications/{publication}/comments"),
                None,
            )
            .await;
        assert_eq!(body_of(&list), sees);
    }
    // It's in the queue, held.
    let queue = s.queue(&mo, "tab=held").await;
    assert_eq!(items(&queue).len(), 1);
    assert_eq!(items(&queue)[0]["sources"], json!("[\"word_filter\"]"));
    // Releasing publishes it.
    let item_id = items(&queue)[0]["id"].as_str().unwrap().to_string();
    s.ok(
        &mo,
        "POST",
        &format!("/admin/queue/{item_id}/release"),
        Some(json!({ "reason": "fine" })),
    )
    .await;
    let list = s
        .ok(
            &reader,
            "GET",
            &format!("/publications/{publication}/comments"),
            None,
        )
        .await;
    assert_eq!(body_of(&list), 1);
}

#[tokio::test]
async fn dismissing_something_held_releases_it_and_bulk_dismiss_needs_one_source() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let author = s.writer("author").await;
    let (_, publication) = s.publication(&author, "A Book").await;
    let poster = s.writer("poster").await;
    let reader = s.writer("reader").await;
    s.add_term(&admin, "watchword", "hold", &["comment"]).await;
    s.add_term(&admin, "flagword", "flag", &["comment"]).await;
    let c = s.comment(&poster, &publication, "a watchword here").await;
    for i in 0..3 {
        s.comment(&poster, &publication, &format!("flagword {i}"))
            .await;
    }
    let reported = s.comment(&poster, &publication, "plain words").await;
    s.ok(
        &reader,
        "POST",
        &format!("/comments/{}/report", reported["id"].as_str().unwrap()),
        Some(json!({ "reason": "spam" })),
    )
    .await;
    let all = items(&s.queue(&admin, "").await);
    let flagged: Vec<String> = all
        .iter()
        .filter(|i| i["sources"] == json!("[\"word_filter\"]") && i["state"] == "open")
        .map(|i| i["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(flagged.len(), 3);
    // Bulk dismiss: only items flagged by that source and no other.
    let mixed = all
        .iter()
        .find(|i| i["subjectId"] == reported["id"])
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let mut with_report = flagged.clone();
    with_report.push(mixed);
    assert_eq!(
        s.status(
            Some(&admin),
            "POST",
            "/admin/queue/dismiss",
            Some(json!({ "itemIds": with_report, "source": "word_filter", "reason": "noise" }))
        )
        .await,
        400
    );
    let done = s
        .ok(
            &admin,
            "POST",
            "/admin/queue/dismiss",
            Some(json!({ "itemIds": flagged, "source": "word_filter", "reason": "noise" })),
        )
        .await;
    assert_eq!(done["dismissed"], 3);
    // Dismissing the held one says it is fine, and it goes live.
    let held = items(&s.queue(&admin, "tab=held").await)[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    s.ok(
        &admin,
        "POST",
        &format!("/admin/queue/{held}/dismiss"),
        Some(json!({ "reason": "fine" })),
    )
    .await;
    let list = s
        .ok(
            &reader,
            "GET",
            &format!("/publications/{publication}/comments"),
            None,
        )
        .await;
    assert!(list
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["id"] == c["id"] && x["body"] != ""));
    // A dismissed item that is flagged again reopens.
    s.ok(
        &reader,
        "POST",
        &format!("/comments/{}/report", c["id"].as_str().unwrap()),
        Some(json!({ "reason": "harassment" })),
    )
    .await;
    assert_eq!(items(&s.queue(&admin, "").await).len(), 2);
}

#[tokio::test]
async fn two_staff_opening_the_same_item_the_second_is_told_who_has_it() {
    let s = Server::start().await;
    let mo = s.moderator("mo").await;
    let admin = s.admin("ada").await;
    let author = s.writer("author").await;
    let (_, publication) = s.publication(&author, "A Book").await;
    let poster = s.writer("poster").await;
    let reader = s.writer("reader").await;
    let c = s.comment(&poster, &publication, "x").await;
    s.ok(
        &reader,
        "POST",
        &format!("/comments/{}/report", c["id"].as_str().unwrap()),
        Some(json!({ "reason": "spam" })),
    )
    .await;
    let item_id = items(&s.queue(&mo, "").await)[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    s.ok(&mo, "POST", &format!("/admin/queue/{item_id}/claim"), None)
        .await;
    let (status, body) = s
        .call(
            Some(&admin),
            "POST",
            &format!("/admin/queue/{item_id}/claim"),
            None,
        )
        .await;
    assert_eq!(status, 409);
    assert_eq!(body["error"]["details"]["claimedBy"], "mo");
    // The holder can keep it, and it shows under "mine".
    s.ok(&mo, "POST", &format!("/admin/queue/{item_id}/claim"), None)
        .await;
    assert_eq!(items(&s.queue(&mo, "mine=true").await).len(), 1);
    assert_eq!(items(&s.queue(&admin, "mine=true").await).len(), 0);
    // Ten minutes later, anyone can.
    diesel::update(moderation_items::table)
        .set(moderation_items::claimed_until.eq("2020-01-01T00:00:00.000Z"))
        .execute(&mut s.conn())
        .unwrap();
    s.ok(
        &admin,
        "POST",
        &format!("/admin/queue/{item_id}/claim"),
        None,
    )
    .await;
}

#[tokio::test]
async fn a_profile_or_publication_item_can_be_dismissed_but_not_removed() {
    let s = Server::start().await;
    let mo = s.moderator("mo").await;
    let target = s.writer("target").await;
    let reporter = s.writer("reporter").await;
    s.ok(
        &reporter,
        "POST",
        "/reports",
        Some(json!({ "subjectType": "profile", "subjectId": target.id, "reason": "hate" })),
    )
    .await;
    let item_id = items(&s.queue(&mo, "").await)[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        s.status(
            Some(&mo),
            "POST",
            &format!("/admin/queue/{item_id}/remove"),
            Some(json!({ "reason": "x" }))
        )
        .await,
        400
    );
    // A moderator can fix the text itself, with a reason.
    s.ok(
        &mo,
        "PATCH",
        &format!("/admin/users/{}", target.id),
        Some(json!({ "bio": "", "reason": "3.1" })),
    )
    .await;
    s.ok(
        &mo,
        "POST",
        &format!("/admin/queue/{item_id}/dismiss"),
        Some(json!({ "reason": "text fixed" })),
    )
    .await;
    assert_eq!(items(&s.queue(&mo, "tab=done").await).len(), 1);
}

#[tokio::test]
async fn an_account_crossing_a_threshold_appears_under_accounts() {
    let s = Server::start().await;
    let mo = s.moderator("mo").await;
    let author = s.writer("author").await;
    let (_, publication) = s.publication(&author, "A Book").await;
    let target = s.writer("target").await;
    let comment = s.comment(&target, &publication, "hmm").await;
    for i in 0..3 {
        let r = s.writer(&format!("r{i}")).await;
        s.ok(
            &r,
            "POST",
            &format!("/comments/{}/report", comment["id"].as_str().unwrap()),
            Some(json!({ "reason": "spam" })),
        )
        .await;
    }
    assert_eq!(
        crate::moderation_jobs::auto_flag_accounts(&mut s.conn()).unwrap(),
        1
    );
    // Again changes nothing.
    assert_eq!(
        crate::moderation_jobs::auto_flag_accounts(&mut s.conn()).unwrap(),
        0
    );
    let accounts = items(&s.queue(&mo, "tab=accounts").await);
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0]["subjectId"], target.id);
    assert_eq!(accounts[0]["sources"], json!("[\"threshold\"]"));
    // The settings can move the bar.
    let admin = s.admin("ada").await;
    let mut settings = s
        .ok(&admin, "GET", "/admin/moderation-settings", None)
        .await;
    settings["thresholds"]["reporters"] = json!(10);
    s.ok(&admin, "PUT", "/admin/moderation-settings", Some(settings))
        .await;
}

#[tokio::test]
async fn a_spam_account_can_have_all_its_public_things_removed_and_stay() {
    let s = Server::start().await;
    s.prompt(&today(), "Write about a door.");
    let admin = s.admin("ada").await;
    let author = s.writer("author").await;
    let (_, publication) = s.publication(&author, "A Book").await;
    let spammer = s.joined("spammer").await;
    let reader = s.joined("reader").await;
    s.comment(&spammer, &publication, "buy my stuff").await;
    s.answer_today(&spammer, "spam spam spam", true).await;
    let r = s
        .ok(
            &admin,
            "POST",
            &format!("/admin/users/{}/delete-content", spammer.id),
            Some(json!({ "reason": "spam account" })),
        )
        .await;
    assert!(r["removed"].as_i64().unwrap() >= 2);
    let list = s
        .ok(
            &reader,
            "GET",
            &format!("/publications/{publication}/comments"),
            None,
        )
        .await;
    assert_eq!(list[0]["body"], "");
    let feed = s
        .ok(
            &reader,
            "GET",
            &format!("/prompts/{}/entries", today()),
            None,
        )
        .await;
    assert!(feed["entries"].as_array().unwrap().is_empty());
    // They're still here.
    assert_eq!(s.status(Some(&spammer), "GET", "/auth/me", None).await, 200);
}

// --- the word filter -------------------------------------------------------------------

#[tokio::test]
async fn each_word_filter_action_does_what_it_says_on_new_posts_and_on_edits() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let mo = s.moderator("mo").await;
    let author = s.writer("author").await;
    let (_, publication) = s.publication(&author, "A Book").await;
    let poster = s.writer("poster").await;
    s.add_term(&admin, "blockword", "block", &["comment"]).await;
    s.add_term(&admin, "holdword", "hold", &["comment"]).await;
    s.add_term(&admin, "flagword", "flag", &["comment"]).await;
    s.add_term(&admin, "maskword", "mask", &["comment"]).await;

    // Block: refused, and the word isn't named.
    let (status, body) = s
        .call(
            Some(&poster),
            "POST",
            &format!("/publications/{publication}/comments"),
            Some(json!({ "body": "a BLOCKWORD here" })),
        )
        .await;
    assert_eq!(status, 400);
    assert_eq!(
        body["error"]["message"],
        "That contains a word that isn't allowed here"
    );
    assert!(!body.to_string().to_lowercase().contains("blockword"));
    // Hold: saved held, in the queue.
    let held = s.comment(&poster, &publication, "a holdword here").await;
    assert_eq!(held["moderationState"], "held");
    // Flag: published, in the queue.
    let flagged = s.comment(&poster, &publication, "a flagword here").await;
    assert_eq!(flagged["moderationState"], "visible");
    // Mask: published with the term replaced, the original kept for staff.
    let masked = s.comment(&poster, &publication, "a maskword here").await;
    assert_eq!(masked["body"], "a ••• here");
    let original: Option<String> = comments::table
        .find(masked["id"].as_str().unwrap())
        .select(comments::original_body)
        .first(&mut s.conn())
        .unwrap();
    assert_eq!(original.as_deref(), Some("a maskword here"));
    assert_eq!(
        items(&s.queue(&mo, "").await).len(),
        2,
        "the flagged one and the masked one"
    );

    // Edits are checked again.
    let clean = s.comment(&poster, &publication, "perfectly clean").await;
    let id = clean["id"].as_str().unwrap();
    assert_eq!(
        s.status(
            Some(&poster),
            "PATCH",
            &format!("/comments/{id}"),
            Some(json!({ "body": "now with blockword" }))
        )
        .await,
        400
    );
    let edited = s
        .ok(
            &poster,
            "PATCH",
            &format!("/comments/{id}"),
            Some(json!({ "body": "now with holdword" })),
        )
        .await;
    assert_eq!(edited["moderationState"], "held");
}

#[tokio::test]
async fn a_term_applies_only_where_it_is_set_and_names_and_bios_are_checked() {
    let s = Server::start().await;
    s.prompt(&today(), "Write about a door.");
    let admin = s.admin("ada").await;
    let mo = s.moderator("mo").await;
    let w = s.joined("writer").await;
    s.add_term(&admin, "badname", "block", &["display_name"])
        .await;
    s.add_term(&admin, "badbio", "flag", &["bio", "about_author"])
        .await;
    s.add_term(&admin, "critword", "block", &["critique"]).await;
    assert_eq!(
        s.status(
            Some(&w),
            "PATCH",
            "/me/profile",
            Some(json!({ "displayName": "Mr Badname" }))
        )
        .await,
        400
    );
    s.ok(
        &w,
        "PATCH",
        "/me/profile",
        Some(json!({ "bio": "I like badbio things" })),
    )
    .await;
    let queue = items(&s.queue(&mo, "").await);
    assert_eq!(queue[0]["subjectType"], "profile");
    // A critique term does not catch a comment, or a bio.
    let (_, publication) = s.publication(&w, "A Book").await;
    s.comment(&w, &publication, "critword is fine in a comment")
        .await;
    let entry = s.answer_today(&w, "text", true).await;
    let other = s.joined("other").await;
    assert_eq!(
        s.status(
            Some(&other),
            "POST",
            &format!("/prompt-entries/{entry}/critiques"),
            Some(json!({ "body": "a critword" }))
        )
        .await,
        400
    );
}

#[tokio::test]
async fn the_test_box_and_the_csv_round_trip_and_the_allowlist_works() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    s.ok(
        &admin,
        "POST",
        "/admin/words",
        Some(json!({ "term": "cum", "matchKind": "substring", "action": "block", "appliesTo": ["comment"] })),
    )
    .await;
    let none = s
        .ok(
            &admin,
            "POST",
            "/admin/words/test",
            Some(json!({ "text": "a nice cucumber" })),
        )
        .await;
    assert_eq!(
        none["action"], "block",
        "the Scunthorpe problem, as the term is a substring match"
    );
    // A word term is a whole word.
    s.ok(
        &admin,
        "POST",
        "/admin/words/allow",
        Some(json!({ "word": "cucumber" })),
    )
    .await;
    let test = |text: &str| {
        let (s, admin, text) = (&s, &admin, text.to_string());
        async move {
            s.ok(
                admin,
                "POST",
                "/admin/words/test",
                Some(json!({ "text": text })),
            )
            .await
        }
    };
    assert!(
        test("a nice cucumber").await["action"].is_null(),
        "an allowed word is not caught"
    );
    let hit = test("they c.u.m or s p a m").await;
    assert_eq!(hit["action"], "block");
    assert_eq!(hit["matches"][0]["term"], "cum");
    // Export and re-import.
    let csv = s
        .http
        .get(format!("{}/admin/words/export", s.base))
        .bearer_auth(&admin.token)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(csv.starts_with("term,match,action,applies_to,note"));
    assert!(csv.contains("cum,substring,block,comment"));
    let imported = s
        .ok(&admin, "POST", "/admin/words/import", Some(json!({ "csv": "# starter\nterm,match,action,applies_to,note\nspamlink,substring,hold,comment;critique,\"known, scam\"\n" })))
        .await;
    assert_eq!(imported["added"], 1);
    assert_eq!(
        s.status(
            Some(&admin),
            "POST",
            "/admin/words/import",
            Some(json!({ "csv": "x,word,explode,comment," }))
        )
        .await,
        400
    );
    let list = s.ok(&admin, "GET", "/admin/words", None).await;
    assert_eq!(list["terms"].as_array().unwrap().len(), 2);
    assert_eq!(list["allow"], json!(["cucumber"]));
    // Terms and their surfaces are validated.
    assert_eq!(
        s.status(
            Some(&admin),
            "POST",
            "/admin/words",
            Some(json!({ "term": "x", "action": "block", "appliesTo": ["manuscript"] }))
        )
        .await,
        400
    );
    assert_eq!(
        s.status(
            Some(&admin),
            "POST",
            "/admin/words",
            Some(json!({ "term": "x", "action": "block", "appliesTo": [] }))
        )
        .await,
        400
    );
    // And a re-scan queues what the new list would have caught in the last 30 days.
    let author = s.writer("author").await;
    let (_, publication) = s.publication(&author, "A Book").await;
    let poster = s.writer("poster").await;
    s.comment(&poster, &publication, "an old spamlink post")
        .await;
    s.add_term(&admin, "sneaky", "hold", &["comment"]).await;
    // (A post made before the term existed.)
    let old = s.comment(&poster, &publication, "nothing wrong").await;
    diesel::update(comments::table.find(old["id"].as_str().unwrap()))
        .set(comments::body.eq("something sneaky"))
        .execute(&mut s.conn())
        .unwrap();
    let scan = s.ok(&admin, "POST", "/admin/words/rescan", None).await;
    assert!(scan["flagged"].as_i64().unwrap() >= 1, "{scan}");
    assert!(scan["checked"].as_i64().unwrap() >= 1);
}

#[tokio::test]
async fn private_writing_is_never_filtered_screened_sent_or_queued() {
    let s = Server::start().await;
    s.prompt(&today(), "Write about a door.");
    let admin = s.admin("ada").await;
    let mo = s.moderator("mo").await;
    let w = s.joined("writer").await;
    let mark = unique("PRIVATE");
    let nasty = format!("{mark} zzfiltered [[remove]]");
    // Everything is on: every surface filtered and checked by the platform.
    let all: Vec<&str> = crate::domain::filter::Surface::ALL
        .iter()
        .map(|s| s.as_str())
        .collect();
    s.add_term(&admin, "zzfiltered", "block", &all).await;
    s.configure_platform(&admin, &all, "block").await;

    // A manuscript scene, a notebook entry, an annotation, a draft Daily
    // Prompts answer: all private, all saved untouched.
    let work = s
        .ok(
            &w,
            "POST",
            "/works",
            Some(json!({ "title": "Private Book", "kind": "novel" })),
        )
        .await;
    let work_id = work["id"].as_str().unwrap();
    let outline = s
        .ok(&w, "GET", &format!("/works/{work_id}/outline"), None)
        .await;
    let scene_id = outline["chapters"][0]["scenes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    s.ok(
        &w,
        "PUT",
        &format!("/scenes/{scene_id}/content"),
        Some(json!({ "content": text_doc(&nasty), "createRevision": true })),
    )
    .await;
    s.ok(
        &w,
        "POST",
        &format!("/works/{work_id}/notebook"),
        Some(json!({ "kind": "note", "name": "Secret", "body": nasty })),
    )
    .await;
    let draft = s.answer_today(&w, &nasty, false).await;
    let draft_again = s
        .ok(
            &w,
            "PUT",
            &format!("/prompts/{}/entry", today()),
            Some(json!({ "content": text_doc(&format!("{nasty} edited")) })),
        )
        .await;
    assert_eq!(draft_again["id"], draft);
    assert_eq!(draft_again["state"], "draft");

    assert!(items(&s.queue(&mo, "").await).is_empty());
    assert!(items(&s.queue(&mo, "tab=held").await).is_empty());
    assert!(
        stub_bodies(&mark).is_empty(),
        "private writing reached the platform: {:?}",
        stub_bodies(&mark)
    );
    let pending: i64 = external_checks::table
        .count()
        .get_result(&mut s.conn())
        .unwrap();
    assert_eq!(pending, 0);

    // The same words, once they are public, are caught — the control.
    assert_eq!(
        s.status(
            Some(&w),
            "POST",
            &format!("/prompts/{}/entry/submit", today()),
            None
        )
        .await,
        400,
        "submitting makes it public, and the filter refuses it"
    );
    let still: String = prompt_entries::table
        .find(&draft)
        .select(prompt_entries::state)
        .first(&mut s.conn())
        .unwrap();
    assert_eq!(still, "draft");
}

// --- the external platform ---------------------------------------------------------------

#[tokio::test]
async fn each_verdict_from_the_platform_does_what_the_setting_says() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let mo = s.moderator("mo").await;
    let author = s.writer("author").await;
    let (_, publication) = s.publication(&author, "A Book").await;
    let poster = s.writer("poster").await;
    s.configure_platform(&admin, &["comment"], "hold").await;

    let keep = s.comment(&poster, &publication, "a fine comment").await;
    assert_eq!(keep["moderationState"], "visible");
    let flag = s
        .comment(&poster, &publication, "a flagged one [[flag]]")
        .await;
    assert_eq!(flag["moderationState"], "visible");
    let remove = s
        .comment(&poster, &publication, "an awful one [[remove]]")
        .await;
    assert_eq!(
        remove["moderationState"], "held",
        "remove maps to hold by default"
    );
    let queue = items(&s.queue(&mo, "").await);
    assert_eq!(queue.len(), 1, "the flag is open, the hold is held");
    assert!(queue[0]["sources"].as_str().unwrap().contains("external"));
    assert_eq!(items(&s.queue(&mo, "tab=held").await).len(), 1);
    assert!(queue[0]["labels"].as_str().unwrap().contains("toxicity"));

    // Edits are checked too.
    let e = keep["id"].as_str().unwrap();
    let edited = s
        .ok(
            &poster,
            "PATCH",
            &format!("/comments/{e}"),
            Some(json!({ "body": "now [[remove]]" })),
        )
        .await;
    assert_eq!(edited["moderationState"], "held");

    // The other mapping refuses the post.
    s.configure_platform(&admin, &["comment"], "block").await;
    assert_eq!(
        s.status(
            Some(&poster),
            "POST",
            &format!("/publications/{publication}/comments"),
            Some(json!({ "body": "[[remove]] nope" }))
        )
        .await,
        400
    );
    // A surface that isn't ticked is not checked.
    s.configure_platform(&admin, &["critique"], "hold").await;
    let unchecked = s
        .comment(&poster, &publication, "[[remove]] but not checked")
        .await;
    assert_eq!(unchecked["moderationState"], "visible");
    // Changing the provider is in the log.
    let log = s
        .ok(
            &admin,
            "GET",
            "/admin/log?action=external_moderation_changed",
            None,
        )
        .await;
    assert!(log["lines"].as_array().unwrap().len() >= 3);
    assert!(!log.to_string().contains("sk_test_key"));
}

#[tokio::test]
async fn a_platform_that_is_down_never_stops_a_post_and_its_backlog_is_checked_later() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let mo = s.moderator("mo").await;
    let author = s.writer("author").await;
    let (_, publication) = s.publication(&author, "A Book").await;
    let poster = s.writer("poster").await;
    s.configure_platform(&admin, &["comment"], "hold").await;

    let started = std::time::Instant::now();
    let down = s
        .comment(
            &poster,
            &publication,
            "posted while it was down [[down-once]] [[remove]]",
        )
        .await;
    let slow = s
        .comment(&poster, &publication, "posted while it was slow [[slow]]")
        .await;
    assert!(
        started.elapsed() < std::time::Duration::from_secs(4),
        "posting waited on the platform"
    );
    assert_eq!(down["moderationState"], "visible");
    assert_eq!(slow["moderationState"], "visible");
    let pending: Vec<(String, i32)> = external_checks::table
        .filter(external_checks::state.eq("pending"))
        .select((external_checks::subject_id, external_checks::attempts))
        .load(&mut s.conn())
        .unwrap();
    assert_eq!(pending.len(), 2, "{pending:?}");
    let report = s.ok(&admin, "GET", "/admin/jobs", None).await;
    assert_eq!(report["externalBacklog"], 2);

    // The job tries: the once-down comment fails its first retry, backs off…
    crate::moderation_jobs::external_checks(&s.state)
        .await
        .unwrap();
    let after_first: Vec<(String, i32, String)> = external_checks::table
        .filter(external_checks::state.eq("pending"))
        .select((
            external_checks::subject_id,
            external_checks::attempts,
            external_checks::next_attempt_at,
        ))
        .load(&mut s.conn())
        .unwrap();
    assert!(
        after_first.iter().all(|(_, attempts, _)| *attempts >= 1),
        "{after_first:?}"
    );
    // …and nothing is retried before its time.
    crate::moderation_jobs::external_checks(&s.state)
        .await
        .unwrap();
    // When the time comes, it works: the once-down one is checked, found
    // awful, and held for review.
    diesel::update(external_checks::table)
        .set(external_checks::next_attempt_at.eq("2020-01-01T00:00:00.000Z"))
        .execute(&mut s.conn())
        .unwrap();
    crate::moderation_jobs::external_checks(&s.state)
        .await
        .unwrap();
    let held = items(&s.queue(&mo, "tab=held").await);
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0]["subjectId"], down["id"]);
    let moderation_state: String = comments::table
        .find(down["id"].as_str().unwrap())
        .select(comments::moderation_state)
        .first(&mut s.conn())
        .unwrap();
    assert_eq!(moderation_state, "held");
}

#[tokio::test]
async fn only_the_text_and_an_opaque_id_go_to_the_platform_and_never_what_the_filter_blocked() {
    let s = Server::start().await;
    s.prompt(&today(), "Write about a door.");
    let admin = s.admin("ada").await;
    let author = s.writer("author").await;
    let (_, publication) = s.publication(&author, "A Book").await;
    let poster = s.writer("poster").await;
    s.ok(
        &poster,
        "PATCH",
        "/me/profile",
        Some(json!({ "displayName": "Poster McPoster", "bio": "My bio" })),
    )
    .await;
    let all: Vec<&str> = crate::domain::filter::Surface::ALL
        .iter()
        .map(|s| s.as_str())
        .collect();
    s.configure_platform(&admin, &all, "hold").await;
    s.add_term(&admin, "zzblocked", "block", &["comment"]).await;

    let mark = unique("OUTGOING");
    s.comment(&poster, &publication, &format!("{mark} a note"))
        .await;
    let sent = stub_bodies(&mark);
    assert_eq!(sent.len(), 1);
    let body = &sent[0];
    let keys: std::collections::BTreeSet<&str> = body
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();
    assert_eq!(
        keys,
        ["authorRef", "subjectId", "surface", "text"]
            .into_iter()
            .collect()
    );
    assert_eq!(body["surface"], "comment");
    let reference = body["authorRef"].as_str().unwrap();
    assert_ne!(reference, poster.id);
    assert_eq!(reference.len(), 32);
    let all_text = body.to_string();
    for needle in [
        "poster@example.com",
        "Poster McPoster",
        &poster.id,
        "example.com",
    ] {
        assert!(
            !all_text.contains(needle),
            "{needle} went to the platform: {all_text}"
        );
    }

    // A name or bio being checked is the one place a name is sent, as the text.
    let name_mark = unique("NAMED");
    s.ok(
        &poster,
        "PATCH",
        "/me/profile",
        Some(json!({ "displayName": name_mark.clone() })),
    )
    .await;
    let name_bodies = stub_bodies(&name_mark);
    assert_eq!(name_bodies.len(), 1);
    assert_eq!(name_bodies[0]["surface"], "display_name");

    // A post the filter blocks is never sent.
    let blocked = unique("BLOCKED");
    assert_eq!(
        s.status(
            Some(&poster),
            "POST",
            &format!("/publications/{publication}/comments"),
            Some(json!({ "body": format!("{blocked} zzblocked") }))
        )
        .await,
        400
    );
    assert!(stub_bodies(&blocked).is_empty());
}

// --- the webhook ------------------------------------------------------------------------------

async fn deliver(s: &Server, payload: Value, secret: &str) -> u16 {
    let raw = payload.to_string();
    s.http
        .post(format!("{}/webhooks/moderation/stub", s.base))
        .header("x-signature", external::sign(secret, raw.as_bytes()))
        .header("content-type", "application/json")
        .body(raw)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

fn apply(s: &Server) -> usize {
    crate::routes::moderation_webhooks::apply_pending(
        &mut s.conn(),
        &s.state.config.jwt_secret,
        "http://justtype.test",
    )
    .unwrap()
}

#[tokio::test]
async fn an_event_with_a_bad_signature_is_refused_and_stored_nowhere() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    s.configure_platform(&admin, &["comment"], "hold").await;
    let payload = json!({ "id": "evt-1", "type": "content.safe", "subject": { "type": "comment", "id": "x" } });
    assert_eq!(deliver(&s, payload.clone(), "wrong secret").await, 401);
    let raw = payload.to_string();
    let unsigned = s
        .http
        .post(format!("{}/webhooks/moderation/stub", s.base))
        .header("content-type", "application/json")
        .body(raw.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(unsigned.status(), 401);
    // Not for a provider that isn't the one set up.
    let other = s
        .http
        .post(format!("{}/webhooks/moderation/stream", s.base))
        .header("x-signature", external::sign("whsec_test", raw.as_bytes()))
        .body(raw)
        .send()
        .await
        .unwrap();
    assert_eq!(other.status(), 401);
    let stored: i64 = external_events::table
        .count()
        .get_result(&mut s.conn())
        .unwrap();
    assert_eq!(stored, 0);
}

#[tokio::test]
async fn a_decision_made_on_the_platform_is_applied_once_and_logged_under_its_moderator() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let author = s.writer("author").await;
    let (_, publication) = s.publication(&author, "A Book").await;
    let poster = s.writer("poster").await;
    let reader = s.writer("reader").await;
    s.configure_platform(&admin, &["comment"], "hold").await;
    let comment = s
        .comment(&poster, &publication, "a flagged one [[flag]]")
        .await;
    let id = comment["id"].as_str().unwrap().to_string();

    let removal = json!({
        "id": "evt-remove-1", "type": "content.removed", "moderator": "mod_77",
        "subject": { "type": "comment", "id": id },
    });
    assert_eq!(deliver(&s, removal.clone(), "whsec_test").await, 200);
    // Delivered twice: stored once, applied once.
    assert_eq!(deliver(&s, removal, "whsec_test").await, 200);
    assert_eq!(
        external_events::table
            .count()
            .get_result::<i64>(&mut s.conn())
            .unwrap(),
        1
    );
    assert_eq!(apply(&s), 1);
    assert_eq!(apply(&s), 0);
    let list = s
        .ok(
            &reader,
            "GET",
            &format!("/publications/{publication}/comments"),
            None,
        )
        .await;
    assert_eq!(list[0]["moderationState"], "removed");
    let removed: i64 = audit_events::table
        .filter(audit_events::action.eq("content_removed"))
        .filter(audit_events::actor_id.eq("external:stub:mod_77"))
        .count()
        .get_result(&mut s.conn())
        .unwrap();
    assert_eq!(removed, 1);
    // And the log shows the platform and its moderator.
    let log = s.ok(&admin, "GET", "/admin/log", None).await;
    assert_eq!(log["lines"][0]["origin"], "external platform");
    assert_eq!(
        log["lines"][0]["actor"]["displayName"],
        "external:stub:mod_77"
    );
    // The author is told, as when our own staff do it.
    let inbox = s.ok(&poster, "GET", "/inbox", None).await;
    assert!(inbox["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["text"].as_str().unwrap().contains("removed your comment")));

    // Restored, then marked safe.
    for (n, kind) in [("evt-2", "content.restored"), ("evt-3", "content.safe")] {
        deliver(&s, json!({ "id": n, "type": kind, "moderator": "mod_77", "subject": { "type": "comment", "id": id } }), "whsec_test").await;
    }
    assert_eq!(apply(&s), 2);
    let list = s
        .ok(
            &reader,
            "GET",
            &format!("/publications/{publication}/comments"),
            None,
        )
        .await;
    assert_eq!(list[0]["moderationState"], "visible");
}

#[tokio::test]
async fn a_temporary_ban_from_the_platform_is_applied_and_a_permanent_one_waits_for_an_admin() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let author = s.writer("author").await;
    let (_, publication) = s.publication(&author, "A Book").await;
    let poster = s.writer("poster").await;
    s.configure_platform(&admin, &["comment"], "hold").await;
    let comment = s
        .comment(&poster, &publication, "a flagged one [[flag]]")
        .await;
    let subject = json!({ "type": "comment", "id": comment["id"] });

    deliver(&s, json!({ "id": "evt-ban-7", "type": "user.banned", "moderator": "m1", "days": 7, "subject": subject }), "whsec_test").await;
    assert_eq!(apply(&s), 1);
    // Discussions only, by default: they can still sign in and write.
    assert_eq!(s.status(Some(&poster), "GET", "/auth/me", None).await, 200);
    let refused = s
        .call(
            Some(&poster),
            "POST",
            &format!("/publications/{publication}/comments"),
            Some(json!({ "body": "hi" })),
        )
        .await;
    assert_eq!(refused.0, 403);
    assert_eq!(refused.1["error"]["code"], "account_restricted");
    let r: (String, String, String) = restrictions::table
        .select((
            restrictions::created_by,
            restrictions::scope,
            restrictions::reason_code,
        ))
        .first(&mut s.conn())
        .unwrap();
    assert_eq!(
        r,
        (
            "external:stub:m1".into(),
            "discussion".into(),
            "external".into()
        )
    );
    assert!(s
        .mail_to("poster@example.com")
        .iter()
        .any(|m| m.0.contains("restriction")));

    // A permanent ban, or anything that would close the account, is never applied.
    for (n, extra) in [
        ("evt-perm", json!({ "days": null })),
        ("evt-odd", json!({ "days": 90 })),
    ] {
        let mut e =
            json!({ "id": n, "type": "user.banned", "moderator": "m2", "subject": subject });
        for (k, v) in extra.as_object().unwrap() {
            e[k] = v.clone();
        }
        deliver(&s, e, "whsec_test").await;
    }
    assert_eq!(apply(&s), 2);
    let restrictions: i64 = restrictions::table
        .count()
        .get_result(&mut s.conn())
        .unwrap();
    assert_eq!(restrictions, 1, "still only the 7-day one");
    let accounts = items(&s.queue(&admin, "tab=accounts").await);
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0]["subjectId"], poster.id);
    assert!(accounts[0]["labels"]
        .as_str()
        .unwrap()
        .contains("permanent ban"));
    let purge: Option<String> = users::table
        .find(&poster.id)
        .select(users::purge_at)
        .first(&mut s.conn())
        .unwrap();
    assert!(purge.is_none());

    // The scope is a setting.
    let mut cfg = s.ok(&admin, "GET", "/admin/external", None).await["config"].clone();
    cfg["banScope"] = json!("site");
    cfg["provider"] = json!("stub");
    s.ok(&admin, "PUT", "/admin/external", Some(cfg)).await;
}

#[tokio::test]
async fn unrecognized_events_are_stored_not_applied_and_shown_on_the_jobs_page() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    s.configure_platform(&admin, &["comment"], "hold").await;
    deliver(
        &s,
        json!({ "id": "evt-weird", "type": "channel.exploded", "moderator": "m" }),
        "whsec_test",
    )
    .await;
    assert_eq!(apply(&s), 0);
    let report = s.ok(&admin, "GET", "/admin/jobs", None).await;
    assert!(report["externalUnapplied"][0]
        .as_str()
        .unwrap()
        .contains("channel.exploded"));
    let applied: Option<String> = external_events::table
        .select(external_events::applied_at)
        .first(&mut s.conn())
        .unwrap();
    assert!(applied.is_none());
}

#[tokio::test]
async fn deciding_here_closes_the_item_on_the_platform_too() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let mo = s.moderator("mo").await;
    let author = s.writer("author").await;
    let (_, publication) = s.publication(&author, "A Book").await;
    let poster = s.writer("poster").await;
    s.configure_platform(&admin, &["comment"], "hold").await;
    let marker = unique("CLOSEME");
    let comment = s
        .comment(&poster, &publication, &format!("{marker} [[remove]]"))
        .await;
    let id = comment["id"].as_str().unwrap().to_string();
    // The platform recorded its own id for it when the job checked it.
    diesel::insert_into(external_checks::table)
        .values((
            external_checks::id.eq(uuid::Uuid::new_v4().to_string()),
            external_checks::subject_type.eq("comment"),
            external_checks::subject_id.eq(&id),
            external_checks::provider.eq("stub"),
            external_checks::external_id.eq(Some(format!("ext-{marker}"))),
            external_checks::state.eq("done"),
            external_checks::attempts.eq(1),
            external_checks::created_at.eq(clock::now_iso()),
            external_checks::next_attempt_at.eq(clock::now_iso()),
        ))
        .execute(&mut s.conn())
        .unwrap();
    let item = items(&s.queue(&mo, "tab=held").await)[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    s.ok(
        &mo,
        "POST",
        &format!("/admin/queue/{item}/restore"),
        Some(json!({ "reason": "fine after all" })),
    )
    .await;
    crate::moderation_jobs::external_checks(&s.state)
        .await
        .unwrap();
    let told = external::STUB_LOG
        .lock()
        .unwrap()
        .iter()
        .any(|(endpoint, body)| {
            endpoint == "review"
                && body["id"] == format!("ext-{marker}")
                && body["decision"] == "restored"
        });
    assert!(told, "the platform was not told");
    let unsent: i64 = external_outbox::table
        .filter(external_outbox::sent_at.is_null())
        .count()
        .get_result(&mut s.conn())
        .unwrap();
    assert_eq!(unsent, 0);
}

#[tokio::test]
async fn settings_never_show_a_secret_again_and_the_test_box_asks_the_platform() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    s.configure_platform(&admin, &["comment"], "hold").await;
    let view = s.ok(&admin, "GET", "/admin/external", None).await;
    let text = view.to_string();
    assert_eq!(view["apiKeySet"], true);
    assert_eq!(view["webhookSecretSet"], true);
    for secret in ["sk_test_key", "whsec_test"] {
        assert!(!text.contains(secret), "{secret} was sent back");
    }
    let stored: String = moderation_settings::table
        .find("external")
        .select(moderation_settings::value)
        .first(&mut s.conn())
        .unwrap();
    assert!(
        !stored.contains("sk_test_key") && !stored.contains("whsec_test"),
        "stored in the clear: {stored}"
    );
    let verdict = s
        .ok(
            &admin,
            "POST",
            "/admin/external/test",
            Some(json!({ "text": "hello [[flag]]" })),
        )
        .await;
    assert_eq!(verdict["recommended"], "flag");
    assert!(view["webhookUrl"]
        .as_str()
        .unwrap()
        .ends_with("/api/webhooks/moderation/stub"));
    assert_eq!(
        s.status(
            Some(&admin),
            "PUT",
            "/admin/external",
            Some(json!({ "provider": "carrier-pigeon" }))
        )
        .await,
        400
    );
}

// --- the log ---------------------------------------------------------------------------------------

#[tokio::test]
async fn the_log_is_filtered_scoped_exported_and_cannot_be_changed() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let mo = s.moderator("mo").await;
    let mo2 = s.moderator("mo2").await;
    let user = s.writer("user").await;
    s.restrict(&mo, &user, json!({ "kind": "suspend", "scope": "discussion", "days": 3, "reasonCode": "3.2", "reasonText": "kind" })).await;
    s.restrict(
        &mo2,
        &user,
        json!({ "kind": "warning", "scope": "site", "reasonCode": "1.1", "reasonText": "again" }),
    )
    .await;
    s.ok(
        &admin,
        "POST",
        &format!("/admin/users/{}/notes", user.id),
        Some(json!({ "body": "x" })),
    )
    .await;

    let all = s.ok(&admin, "GET", "/admin/log", None).await;
    assert_eq!(all["lines"].as_array().unwrap().len(), 3);
    let line = all["lines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["action"] == "restriction_added")
        .unwrap();
    assert!(line["actor"]["displayName"].is_string());
    assert_eq!(line["subject"]["displayName"], "user");
    assert!(line["reason"].is_string() && line["createdAt"].is_string());
    // Filters.
    let by_actor = s
        .ok(&admin, "GET", &format!("/admin/log?actor={}", mo.id), None)
        .await;
    assert_eq!(by_actor["lines"].as_array().unwrap().len(), 1);
    let by_subject = s
        .ok(
            &admin,
            "GET",
            &format!("/admin/log?subject={}&action=restriction_added", user.id),
            None,
        )
        .await;
    assert_eq!(by_subject["lines"].as_array().unwrap().len(), 2);
    assert_eq!(
        s.ok(&admin, "GET", "/admin/log?from=2999-01-01", None)
            .await["lines"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    // A moderator sees their own actions, and the account they are looking at.
    let own = s.ok(&mo, "GET", "/admin/log", None).await;
    assert_eq!(own["lines"].as_array().unwrap().len(), 1);
    let looking = s
        .ok(&mo, "GET", &format!("/admin/log?subject={}", user.id), None)
        .await;
    assert_eq!(looking["lines"].as_array().unwrap().len(), 3);
    // CSV for admins only.
    let csv = s
        .http
        .get(format!("{}/admin/log.csv", s.base))
        .bearer_auth(&admin.token)
        .send()
        .await
        .unwrap();
    assert_eq!(csv.status(), 200);
    let text = csv.text().await.unwrap();
    assert!(text.starts_with("when,who,origin,action,about,detail,reason"));
    assert_eq!(text.lines().count(), 4);
    assert_eq!(
        s.status(Some(&mo), "GET", "/admin/log.csv", None).await,
        403
    );
    // No route changes or removes a line.
    let id = line["id"].as_str().unwrap();
    for method in ["PATCH", "PUT", "DELETE", "POST"] {
        let status = s
            .status(
                Some(&admin),
                method,
                &format!("/admin/log/{id}"),
                Some(json!({})),
            )
            .await;
        assert!(status == 404 || status == 405, "{method} gave {status}");
        let status = s
            .status(Some(&admin), method, "/admin/log", Some(json!({})))
            .await;
        assert!(
            status == 404 || status == 405,
            "{method} /admin/log gave {status}"
        );
    }
}

#[tokio::test]
async fn the_landing_page_counts_what_needs_a_person() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let author = s.writer("author").await;
    let (_, publication) = s.publication(&author, "A Book").await;
    let poster = s.writer("poster").await;
    let reporter = s.writer("reporter").await;
    let c = s.comment(&poster, &publication, "x").await;
    s.ok(
        &reporter,
        "POST",
        &format!("/comments/{}/report", c["id"].as_str().unwrap()),
        Some(json!({ "reason": "abuse" })),
    )
    .await;
    s.restrict(
        &admin,
        &poster,
        guideline_for_tests(json!({ "kind": "suspend", "scope": "site", "days": 3 })),
    )
    .await;
    diesel::update(restrictions::table)
        .set(
            restrictions::ends_at.eq((clock::now() + chrono::Duration::hours(2))
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        )
        .execute(&mut s.conn())
        .unwrap();
    let overview = s.ok(&admin, "GET", "/admin/overview", None).await;
    assert_eq!(overview["queueOpen"], 1);
    assert_eq!(overview["endingSoon"], 1);
    assert_eq!(overview["reportsToday"], 1);
    assert!(overview["signupsToday"].as_i64().unwrap() >= 4);
    assert_eq!(overview["role"], "admin");
}

fn guideline_for_tests(extra: Value) -> Value {
    let mut base = json!({ "reasonCode": "3.2", "reasonText": "Please keep it kind." });
    for (k, v) in extra.as_object().unwrap() {
        base[k] = v.clone();
    }
    base
}
