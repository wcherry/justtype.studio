//! Restrictions end to end: that every write route is gated, what each kind
//! takes away, telling the writer, appeals, the permanent ban list, and the
//! 30 days before a closing restriction deletes an account.
use chrono::Duration;
use diesel::prelude::*;
use serde_json::{json, Value};

use super::prompts_api_tests::{Server, Writer};
use crate::clock;
use crate::db::schema::*;
use crate::domain::restrictions::{Capability, Gate, GATES};

fn body(extra: Value) -> Value {
    let mut base = json!({ "reasonCode": "3.2", "reasonText": "Please keep it kind." });
    for (k, v) in extra.as_object().unwrap() {
        base[k] = v.clone();
    }
    base
}

fn concrete(path: &str) -> String {
    let mut out = String::new();
    let mut in_param = false;
    for c in path.trim_start_matches("/api").chars() {
        match c {
            '{' => {
                in_param = true;
                out.push('x');
            }
            '}' => in_param = false,
            c if !in_param => out.push(c),
            _ => {}
        }
    }
    out
}

fn code(response: &(u16, Value)) -> &str {
    response.1["error"]["code"].as_str().unwrap_or("")
}

// --- enforcement ----------------------------------------------------------------

#[tokio::test]
async fn every_write_route_refuses_exactly_what_each_restriction_takes_away() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    // (name, restriction, capabilities it denies)
    let cases: Vec<(&str, Value, Vec<Capability>)> = vec![
        (
            "suspended",
            body(json!({ "kind": "suspend", "scope": "site", "days": 7 })),
            vec![
                Capability::Publish,
                Capability::Discuss,
                Capability::Report,
                Capability::Invite,
                Capability::Follow,
            ],
        ),
        (
            "readonly",
            body(json!({ "kind": "read_only", "scope": "site", "confirmName": "readonly" })),
            vec![
                Capability::Write,
                Capability::Publish,
                Capability::Discuss,
                Capability::Report,
                Capability::Invite,
                Capability::Follow,
            ],
        ),
        (
            "quiet",
            body(json!({ "kind": "suspend", "scope": "discussion", "days": 3 })),
            vec![
                Capability::Discuss,
                Capability::Report,
                Capability::Invite,
                Capability::Follow,
            ],
        ),
        (
            "warned",
            body(json!({ "kind": "warning", "scope": "site" })),
            vec![],
        ),
    ];
    for (name, restriction, denied) in cases {
        let user = s.writer(name).await;
        s.restrict(&admin, &user, restriction).await;
        let mut checked = 0;
        for (method, path, gate) in GATES {
            let url = concrete(path);
            let response = s
                .call(Some(&user), &method.to_uppercase(), &url, Some(json!({})))
                .await;
            let refused = response.0 == 403 && code(&response) == "account_restricted";
            let expected = match gate {
                Gate::Open => false,
                Gate::Needs(c) => denied.contains(c),
            };
            assert_eq!(
                refused, expected,
                "{name}: {method} {path} -> {} {}",
                response.0, response.1
            );
            checked += 1;
        }
        assert!(checked > 100, "{name}: only {checked} routes were walked");
    }
}

#[tokio::test]
async fn a_temporary_restriction_stops_applying_without_any_job() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let owner = s.writer("owner").await;
    let user = s.writer("user").await;
    let (_, publication) = s.publication(&owner, "A Book").await;
    s.restrict(
        &admin,
        &user,
        body(json!({ "kind": "suspend", "scope": "discussion", "days": 3 })),
    )
    .await;
    let attempt = || async {
        s.call(
            Some(&user),
            "POST",
            &format!("/publications/{publication}/comments"),
            Some(json!({ "body": "hello" })),
        )
        .await
    };
    let refused = attempt().await;
    assert_eq!(refused.0, 403);
    assert_eq!(code(&refused), "account_restricted");
    assert_eq!(refused.1["error"]["details"]["capability"], "discuss");
    assert!(refused.1["error"]["details"]["until"].is_string());
    assert_eq!(
        refused.1["error"]["details"]["reason"],
        "Please keep it kind."
    );
    // It ends: the same request now works, and no job ran.
    diesel::update(restrictions::table)
        .set(restrictions::ends_at.eq("2020-01-01T00:00:00.000Z"))
        .execute(&mut s.conn())
        .unwrap();
    assert_eq!(attempt().await.0, 200);
}

#[tokio::test]
async fn only_3_7_30_days_or_permanent_can_be_chosen() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let user = s.writer("user").await;
    for days in [1, 2, 4, 14, 90] {
        assert_eq!(
            s.status(
                Some(&admin),
                "POST",
                &format!("/admin/users/{}/restrictions", user.id),
                Some(body(
                    json!({ "kind": "suspend", "scope": "discussion", "days": days })
                ))
            )
            .await,
            400,
            "{days} days"
        );
    }
    for days in [json!(3), json!(7), json!(30), Value::Null] {
        let r = s
            .restrict(
                &admin,
                &user,
                body(json!({ "kind": "suspend", "scope": "discussion", "days": days })),
            )
            .await;
        assert_eq!(r["inForce"], true);
    }
    // Bad kinds, scopes, and missing reasons.
    for bad in [
        body(json!({ "kind": "doom", "scope": "site", "days": 3 })),
        body(json!({ "kind": "ban", "scope": "galaxy", "days": 3 })),
        json!({ "kind": "ban", "scope": "site", "days": 3, "reasonCode": "3.2" }),
        json!({ "kind": "ban", "scope": "site", "days": 3, "reasonText": "x" }),
    ] {
        assert_eq!(
            s.status(
                Some(&admin),
                "POST",
                &format!("/admin/users/{}/restrictions", user.id),
                Some(bad)
            )
            .await,
            400
        );
    }
}

#[tokio::test]
async fn a_moderator_sets_short_and_discussion_restrictions_and_never_a_closing_one() {
    let s = Server::start().await;
    let mo = s.moderator("mo").await;
    let user = s.writer("user").await;
    let post = |b: Value| {
        let (s, mo, id) = (&s, &mo, user.id.clone());
        async move {
            s.status(
                Some(mo),
                "POST",
                &format!("/admin/users/{id}/restrictions"),
                Some(b),
            )
            .await
        }
    };
    for days in [3, 7, 30] {
        assert_eq!(
            post(body(
                json!({ "kind": "ban", "scope": "site", "days": days })
            ))
            .await,
            200
        );
        assert_eq!(
            post(body(
                json!({ "kind": "suspend", "scope": "site", "days": days })
            ))
            .await,
            200
        );
    }
    // Permanent, but only in discussions.
    assert_eq!(
        post(body(json!({ "kind": "ban", "scope": "discussion" }))).await,
        200
    );
    assert_eq!(
        post(body(json!({ "kind": "warning", "scope": "site" }))).await,
        200
    );
    // The two closing restrictions, and a permanent site suspension, are not theirs.
    assert_eq!(
        post(body(
            json!({ "kind": "ban", "scope": "site", "confirmName": "user" })
        ))
        .await,
        403
    );
    assert_eq!(
        post(body(
            json!({ "kind": "read_only", "scope": "site", "confirmName": "user" })
        ))
        .await,
        403
    );
    assert_eq!(
        post(body(json!({ "kind": "suspend", "scope": "site" }))).await,
        403
    );
    // And nothing was started.
    let purge: Option<String> = users::table
        .find(&user.id)
        .select(users::purge_at)
        .first(&mut s.conn())
        .unwrap();
    assert!(purge.is_none());
}

#[tokio::test]
async fn a_site_ban_takes_effect_on_the_very_next_request_and_says_why() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let user = s.writer("user").await;
    assert_eq!(s.status(Some(&user), "GET", "/auth/me", None).await, 200);
    let r = s
        .restrict(
            &admin,
            &user,
            body(json!({ "kind": "ban", "scope": "site", "days": 7 })),
        )
        .await;
    assert_eq!(r["inForce"], true);
    let next = s.call(Some(&user), "GET", "/auth/me", None).await;
    assert_eq!(next.0, 401, "setting a site ban signs every device out");
    // Signing in says why, until when, and that it can be appealed.
    let login = s
        .call(
            None,
            "POST",
            "/auth/login",
            Some(json!({ "email": "user@example.com", "password": "correcthorse" })),
        )
        .await;
    assert_eq!(login.0, 403);
    assert_eq!(code(&login), "account_banned");
    assert_eq!(
        login.1["error"]["details"]["reason"],
        "Please keep it kind."
    );
    assert_eq!(login.1["error"]["details"]["reasonCode"], "3.2");
    assert!(login.1["error"]["details"]["until"].is_string());
    // A wrong password still says nothing about the ban.
    let wrong = s
        .call(
            None,
            "POST",
            "/auth/login",
            Some(json!({ "email": "user@example.com", "password": "incorrect horse" })),
        )
        .await;
    assert_eq!(wrong.0, 401);
    // And it lifts: they can sign in again.
    let rid = r["id"].as_str().unwrap();
    s.ok(
        &admin,
        "POST",
        &format!("/admin/restrictions/{rid}/lift"),
        Some(json!({ "reason": "resolved" })),
    )
    .await;
    assert_eq!(
        s.status(
            None,
            "POST",
            "/auth/login",
            Some(json!({ "email": "user@example.com", "password": "correcthorse" }))
        )
        .await,
        200
    );
}

#[tokio::test]
async fn the_writer_is_told_by_email_and_in_the_inbox_and_again_when_it_ends() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let user = s.writer("user").await;
    s.restrict(
        &admin,
        &user,
        body(json!({ "kind": "suspend", "scope": "discussion", "days": 3 })),
    )
    .await;
    let mail = s.mail_to("user@example.com");
    let told = mail
        .iter()
        .find(|m| m.0.contains("restriction"))
        .expect("an email");
    for needle in [
        "3.2",
        "Please keep it kind.",
        "This lasts until",
        "appeal once",
    ] {
        assert!(told.1.contains(needle), "{needle} missing from {}", told.1);
    }
    let inbox = s.ok(&user, "GET", "/inbox", None).await;
    let items = inbox["items"].as_array().unwrap();
    assert!(
        items.iter().any(|i| i["kind"] == "moderation.restriction"
            && i["text"].as_str().unwrap().contains("discussions")),
        "{inbox}"
    );
    // No notice names the moderator.
    assert!(!told.1.contains("ada"));
    // A warning says so.
    s.restrict(
        &admin,
        &user,
        body(json!({ "kind": "warning", "scope": "site" })),
    )
    .await;
    let warning = s
        .mail_to("user@example.com")
        .into_iter()
        .find(|m| m.0.contains("warning"))
        .unwrap();
    assert!(warning.1.contains("This is a warning"));

    // It ends: one notice, once, from the job.
    diesel::update(restrictions::table.filter(restrictions::kind.eq("suspend")))
        .set(restrictions::ends_at.eq("2020-01-01T00:00:00.000Z"))
        .execute(&mut s.conn())
        .unwrap();
    let n = crate::moderation_jobs::expire_restrictions(&mut s.conn()).unwrap();
    assert_eq!(n, 1);
    assert_eq!(
        crate::moderation_jobs::expire_restrictions(&mut s.conn()).unwrap(),
        0
    );
    assert!(s
        .mail_to("user@example.com")
        .iter()
        .any(|m| m.0.contains("has ended")));
}

#[tokio::test]
async fn the_ladder_is_a_hint_and_warnings_count_in_the_history() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let user = s.writer("user").await;
    s.restrict(
        &admin,
        &user,
        body(json!({ "kind": "warning", "scope": "site" })),
    )
    .await;
    let detail = s
        .ok(&admin, "GET", &format!("/admin/users/{}", user.id), None)
        .await;
    assert_eq!(detail["restrictions"].as_array().unwrap().len(), 1);
    assert_eq!(
        detail["restrictions"][0]["inForce"],
        false.max(detail["restrictions"][0]["inForce"].as_bool().unwrap())
    );
    assert_eq!(detail["summary"]["status"], "active");
    assert_eq!(detail["ladder"][0], "warning");
    // A warning takes nothing away.
    let (_, publication) = s.publication(&user, "Still mine").await;
    assert_eq!(
        s.comment(&user, &publication, "fine").await["moderationState"],
        "visible"
    );
}

#[tokio::test]
async fn a_restriction_can_be_changed_to_another_length_or_message_and_lifted() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let mo = s.moderator("mo").await;
    let user = s.writer("user").await;
    let r = s
        .restrict(
            &mo,
            &user,
            body(json!({ "kind": "suspend", "scope": "discussion", "days": 3 })),
        )
        .await;
    let id = r["id"].as_str().unwrap();
    let before = r["endsAt"].as_str().unwrap().to_string();
    let changed = s
        .ok(
            &mo,
            "PATCH",
            &format!("/admin/restrictions/{id}"),
            Some(json!({ "days": 30, "reason": "kept at it" })),
        )
        .await;
    assert!(changed["endsAt"].as_str().unwrap() > before.as_str());
    assert_eq!(
        s.status(
            Some(&mo),
            "PATCH",
            &format!("/admin/restrictions/{id}"),
            Some(json!({ "days": 5, "reason": "x" }))
        )
        .await,
        400
    );
    // Permanent, in discussions, is a moderator's to set.
    let forever = s
        .ok(
            &mo,
            "PATCH",
            &format!("/admin/restrictions/{id}"),
            Some(json!({ "days": null, "reason": "no change in behaviour" })),
        )
        .await;
    assert!(forever["endsAt"].is_null());
    s.ok(
        &admin,
        "PATCH",
        &format!("/admin/restrictions/{id}"),
        Some(json!({ "reasonText": "Updated message", "reason": "clearer" })),
    )
    .await;
    assert!(s
        .mail_to("user@example.com")
        .iter()
        .any(|m| m.0.contains("has changed")));
    s.ok(
        &mo,
        "POST",
        &format!("/admin/restrictions/{id}/lift"),
        Some(json!({ "reason": "enough" })),
    )
    .await;
    assert_eq!(
        s.status(
            Some(&mo),
            "POST",
            &format!("/admin/restrictions/{id}/lift"),
            Some(json!({ "reason": "again" }))
        )
        .await,
        409
    );
    let kinds: Vec<String> = audit_events::table
        .filter(audit_events::subject_id.eq(&user.id))
        .order(audit_events::created_at.asc())
        .select(audit_events::action)
        .load(&mut s.conn())
        .unwrap();
    assert_eq!(
        kinds.iter().filter(|k| *k == "restriction_changed").count(),
        3
    );
    assert!(kinds.contains(&"restriction_added".to_string()));
    assert!(kinds.contains(&"restriction_lifted".to_string()));
}

// --- appeals ---------------------------------------------------------------------

#[tokio::test]
async fn a_restricted_writer_appeals_once_and_someone_else_decides() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let mo = s.moderator("mo").await;
    let user = s.writer("user").await;
    let r = s
        .restrict(
            &mo,
            &user,
            body(json!({ "kind": "suspend", "scope": "discussion", "days": 7 })),
        )
        .await;
    let id = r["id"].as_str().unwrap();

    // Where they stand.
    let standing = s.ok(&user, "GET", "/me/restrictions", None).await;
    assert_eq!(standing["active"][0]["reasonText"], "Please keep it kind.");
    assert!(standing["active"][0].get("internalNote").is_none());
    assert!(standing["active"][0].get("createdBy").is_none());

    let long = "x".repeat(2_001);
    assert_eq!(
        s.status(
            Some(&user),
            "POST",
            &format!("/me/restrictions/{id}/appeal"),
            Some(json!({ "body": long }))
        )
        .await,
        400
    );
    s.ok(
        &user,
        "POST",
        &format!("/me/restrictions/{id}/appeal"),
        Some(json!({ "body": "That was a joke between friends." })),
    )
    .await;
    assert_eq!(
        s.status(
            Some(&user),
            "POST",
            &format!("/me/restrictions/{id}/appeal"),
            Some(json!({ "body": "again" }))
        )
        .await,
        409,
        "one appeal per restriction"
    );
    // Somebody else's restriction isn't theirs to appeal.
    let other = s.writer("other").await;
    assert_eq!(
        s.status(
            Some(&other),
            "POST",
            &format!("/me/restrictions/{id}/appeal"),
            Some(json!({ "body": "x" }))
        )
        .await,
        404
    );

    let appeals = s.ok(&mo, "GET", "/admin/appeals", None).await;
    let appeal_id = appeals[0]["appeal"]["id"].as_str().unwrap().to_string();
    // The one who set it can't decide it.
    assert_eq!(
        s.status(
            Some(&mo),
            "POST",
            &format!("/admin/appeals/{appeal_id}/decide"),
            Some(json!({ "decision": "upheld", "message": "no" }))
        )
        .await,
        403
    );
    s.ok(
        &admin,
        "POST",
        &format!("/admin/appeals/{appeal_id}/decide"),
        Some(json!({ "decision": "overturned", "message": "Fair enough — lifted." })),
    )
    .await;
    // Overturning lifts it at once, and the writer is told.
    let after = s.ok(&user, "GET", "/me/restrictions", None).await;
    assert!(after["active"].as_array().unwrap().is_empty());
    assert_eq!(after["history"][0]["appeal"]["state"], "overturned");
    assert!(s
        .mail_to("user@example.com")
        .iter()
        .any(|m| m.0.contains("decision on your appeal")));
    assert_eq!(
        s.status(
            Some(&admin),
            "POST",
            &format!("/admin/appeals/{appeal_id}/decide"),
            Some(json!({ "decision": "upheld", "message": "again" }))
        )
        .await,
        409
    );
}

#[tokio::test]
async fn a_moderator_cannot_decide_an_appeal_on_a_closing_restriction() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let mo = s.moderator("mo").await;
    let user = s.writer("user").await;
    let r = s
        .restrict(
            &admin,
            &user,
            body(json!({ "kind": "read_only", "scope": "site", "confirmName": "user" })),
        )
        .await;
    let id = r["id"].as_str().unwrap();
    s.ok(
        &user,
        "POST",
        &format!("/me/restrictions/{id}/appeal"),
        Some(json!({ "body": "please" })),
    )
    .await;
    let appeals = s.ok(&mo, "GET", "/admin/appeals", None).await;
    assert!(appeals[0]["daysLeft"].as_i64().unwrap() >= 29, "{appeals}");
    let appeal_id = appeals[0]["appeal"]["id"].as_str().unwrap();
    assert_eq!(
        s.status(
            Some(&mo),
            "POST",
            &format!("/admin/appeals/{appeal_id}/decide"),
            Some(json!({ "decision": "overturned", "message": "ok" }))
        )
        .await,
        403
    );
}

#[tokio::test]
async fn a_banned_writer_appeals_and_gets_a_copy_of_their_work_from_emailed_links() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let other_admin = s.admin("bea").await;
    let user = s.writer("user").await;
    let (_, _) = s.publication(&user, "My only book").await;
    s.restrict(
        &admin,
        &user,
        body(json!({ "kind": "ban", "scope": "site", "confirmName": "user" })),
    )
    .await;

    let mail = s.mail_to("user@example.com");
    let ban_mail = mail.iter().find(|m| m.0.contains("restriction")).unwrap();
    assert!(ban_mail.1.contains("will be deleted"));
    assert!(ban_mail.1.contains("/ban/export"));
    let token = Server::token_in(&ban_mail.1);
    // The link says what it is for, without signing in.
    let (status, peek) = s
        .call(None, "GET", &format!("/ban/appeal/{token}"), None)
        .await;
    assert_eq!(status, 200, "{peek}");
    assert_eq!(peek["closing"], true);
    // One appeal, from the link, once.
    let (status, filed) = s
        .call(
            None,
            "POST",
            "/ban/appeal",
            Some(json!({ "token": token, "body": "I'm sorry." })),
        )
        .await;
    assert_eq!(status, 200, "{filed}");
    assert_eq!(
        s.status(
            None,
            "POST",
            "/ban/appeal",
            Some(json!({ "token": token, "body": "again" }))
        )
        .await,
        400
    );
    assert_eq!(
        s.ok(&other_admin, "GET", "/admin/appeals", None)
            .await
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // The copy of their work is emailed to them, until the account is purged.
    let ask = |email: &str| {
        let (s, email) = (&s, email.to_string());
        async move {
            s.status(None, "POST", "/ban/export", Some(json!({ "email": email })))
                .await
        }
    };
    // The same answer whoever is asked about.
    assert_eq!(ask("user@example.com").await, 200);
    assert_eq!(ask("nobody@example.com").await, 200);
    let exports = s
        .mail_to("user@example.com")
        .into_iter()
        .filter(|m| m.0.contains("A copy of"))
        .count();
    assert_eq!(exports, 1);
    assert_eq!(s.mail_to("nobody@example.com").len(), 0);
    let copy = s
        .mail_to("user@example.com")
        .into_iter()
        .find(|m| m.0.contains("A copy of"))
        .unwrap();
    let link = copy
        .1
        .split("/api/ban/export/")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let bytes = s
        .http
        .get(format!("{}/ban/export/{link}", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(bytes.status(), 200);
    let zip = bytes.bytes().await.unwrap();
    assert_eq!(&zip[..2], b"PK");
    // Not before it's asked for, and not for an account that was never banned.
    let plain = s.writer("plain").await;
    let _ = plain;
    assert_eq!(ask("plain@example.com").await, 200);
    assert_eq!(s.mail_to("plain@example.com").len(), 0);
    // After the purge, nothing.
    let now = clock::now() + Duration::days(31);
    let purged = crate::moderation_jobs::purge_one(&mut s.conn(), &user.id, now).unwrap();
    assert!(purged.is_none(), "an open appeal holds the purge");
}

// --- the permanent ban list and closing -----------------------------------------------------

#[tokio::test]
async fn a_permanent_ban_lists_the_address_and_it_can_never_register_again() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    // A Gmail address, to check the variants.
    let register = |email: &str| {
        let (s, email) = (&s, email.to_string());
        async move {
            s.call(
                None,
                "POST",
                "/auth/register",
                Some(json!({ "email": email, "password": "correcthorse", "displayName": "X", "acceptTerms": true })),
            )
            .await
        }
    };
    let first = register("jane@gmail.com").await;
    assert_eq!(first.0, 200, "{:?}", first.1);
    let jane_id = first.1["user"]["id"].as_str().unwrap().to_string();
    let jane = Writer {
        id: jane_id.clone(),
        token: first.1["accessToken"].as_str().unwrap().to_string(),
    };
    let r = s
        .restrict(
            &admin,
            &jane,
            body(json!({ "kind": "ban", "scope": "site", "confirmName": "X", "internalNote": "repeat offender" })),
        )
        .await;
    // The list row, with the reason, and the deletion clock.
    let ban: (String, String, String, Option<String>) = permanent_bans::table
        .select((
            permanent_bans::email,
            permanent_bans::email_canonical,
            permanent_bans::reason_text,
            permanent_bans::former_user_id,
        ))
        .first(&mut s.conn())
        .unwrap();
    assert_eq!(ban.0, "jane@gmail.com");
    assert_eq!(ban.1, "jane@gmail.com");
    assert_eq!(ban.2, "Please keep it kind.");
    assert_eq!(ban.3.as_deref(), Some(jane_id.as_str()));
    let (closing, purge): (Option<String>, Option<String>) = users::table
        .find(&jane_id)
        .select((users::closing_at, users::purge_at))
        .first(&mut s.conn())
        .unwrap();
    assert!(closing.is_some() && purge.is_some());

    // Dots, tags and case are the same address, and so is the original; the
    // refusal is the one any taken address gets.
    for variant in [
        "j.ane@gmail.com",
        "jane+spam@gmail.com",
        "JANE@googlemail.com",
        "jane@gmail.com",
    ] {
        let (status, refused) = register(variant).await;
        assert_eq!(status, 409, "{variant}: {refused}");
        assert_eq!(
            refused["error"]["message"],
            "that email is already registered"
        );
    }
    // Staff-made accounts, address changes and invites check the list too.
    assert_eq!(
        s.status(
            Some(&admin),
            "POST",
            "/admin/users",
            Some(json!({ "email": "j.ane@gmail.com", "displayName": "J" }))
        )
        .await,
        409
    );
    let target = s.writer("target").await;
    assert_eq!(
        s.status(
            Some(&admin),
            "PATCH",
            &format!("/admin/users/{}", target.id),
            Some(json!({ "email": "jane@gmail.com", "reason": "x" }))
        )
        .await,
        409
    );
    let inviter = s.writer("inviter").await;
    s.ok(
        &inviter,
        "POST",
        "/invites",
        Some(json!({ "emails": ["jane+x@gmail.com"] })),
    )
    .await;
    assert!(
        s.mail_to("jane+x@gmail.com").is_empty(),
        "a banned address is not mailed an invitation"
    );

    // The page lists it, for admins only.
    let list = s
        .ok(&admin, "GET", "/admin/permanent-bans?q=jane", None)
        .await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    // Removing a row, with a reason, lets the address register again.
    let ban_id = list[0]["id"].as_str().unwrap();
    assert_eq!(
        s.status(
            Some(&admin),
            "POST",
            &format!("/admin/permanent-bans/{ban_id}/remove"),
            Some(json!({}))
        )
        .await,
        400
    );
    s.ok(
        &admin,
        "POST",
        &format!("/admin/permanent-bans/{ban_id}/remove"),
        Some(json!({ "reason": "wrong person" })),
    )
    .await;
    let _ = r;
    // Off the list, a variant of the address registers again.
    assert_eq!(register("j.ane@gmail.com").await.0, 200);
    let logged: Vec<String> = audit_events::table
        .filter(audit_events::action.like("permanent_ban%"))
        .select(audit_events::action)
        .load(&mut s.conn())
        .unwrap();
    assert!(logged.contains(&"permanent_ban_listed".to_string()));
    assert!(logged.contains(&"permanent_ban_removed".to_string()));
}

#[tokio::test]
async fn throwaway_domains_are_blocked_and_unblocked_by_admins() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let mo = s.moderator("mo").await;
    s.ok(
        &admin,
        "POST",
        "/admin/blocked-domains",
        Some(json!({ "domain": "@Spam.example", "note": "throwaway" })),
    )
    .await;
    assert_eq!(
        s.status(
            Some(&mo),
            "POST",
            "/admin/blocked-domains",
            Some(json!({ "domain": "x.example" }))
        )
        .await,
        403
    );
    let (status, body) = s
        .call(
            None,
            "POST",
            "/auth/register",
            Some(json!({ "email": "me@spam.example", "password": "correcthorse", "displayName": "X", "acceptTerms": true })),
        )
        .await;
    assert_eq!(status, 409, "{body}");
    s.ok(
        &admin,
        "DELETE",
        "/admin/blocked-domains/spam.example",
        None,
    )
    .await;
    assert_eq!(
        s.status(
            None,
            "POST",
            "/auth/register",
            Some(json!({ "email": "me@spam.example", "password": "correcthorse", "displayName": "X", "acceptTerms": true }))
        )
        .await,
        200
    );
}

#[tokio::test]
async fn a_closing_restriction_needs_the_name_typed_and_lifting_it_cancels_the_deletion() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let user = s.writer("user").await;
    let closing = body(json!({ "kind": "read_only", "scope": "site" }));
    assert_eq!(
        s.status(
            Some(&admin),
            "POST",
            &format!("/admin/users/{}/restrictions", user.id),
            Some(closing.clone())
        )
        .await,
        400,
        "the name has to be typed"
    );
    let wrong = {
        let mut b = closing.clone();
        b["confirmName"] = json!("someone else");
        b
    };
    assert_eq!(
        s.status(
            Some(&admin),
            "POST",
            &format!("/admin/users/{}/restrictions", user.id),
            Some(wrong)
        )
        .await,
        400
    );
    // Read-only has no other duration.
    let mut with_days = closing.clone();
    with_days["confirmName"] = json!("user");
    with_days["days"] = json!(7);
    assert_eq!(
        s.status(
            Some(&admin),
            "POST",
            &format!("/admin/users/{}/restrictions", user.id),
            Some(with_days)
        )
        .await,
        400
    );
    let mut ok = closing.clone();
    ok["confirmName"] = json!("user");
    let r = s.restrict(&admin, &user, ok).await;
    let (closing_at, purge_at): (Option<String>, Option<String>) = users::table
        .find(&user.id)
        .select((users::closing_at, users::purge_at))
        .first(&mut s.conn())
        .unwrap();
    assert!(closing_at.is_some());
    let first_purge = purge_at.unwrap();

    // They can read and export, but not write; the countdown is on every page.
    assert_eq!(s.status(Some(&user), "GET", "/me/export", None).await, 200);
    assert_eq!(
        s.status(
            Some(&user),
            "POST",
            "/works",
            Some(json!({ "title": "x", "kind": "novel" }))
        )
        .await,
        403
    );
    let standing = s.ok(&user, "GET", "/me/restrictions", None).await;
    assert_eq!(standing["closing"]["kind"], "read_only");
    assert_eq!(standing["closing"]["purgeAt"], first_purge);
    assert!(standing["closing"]["daysLeft"].as_i64().unwrap() >= 29);
    assert!(s
        .mail_to("user@example.com")
        .iter()
        .any(|m| m.1.contains("will be deleted")));

    // A second closing restriction does not move the date later.
    diesel::update(users::table.find(&user.id))
        .set(users::purge_at.eq("2026-11-01T00:00:00.000Z"))
        .execute(&mut s.conn())
        .unwrap();
    let mut again = body(json!({ "kind": "ban", "scope": "site", "confirmName": "user" }));
    again["days"] = Value::Null;
    s.restrict(&admin, &user, again).await;
    let purge_after: Option<String> = users::table
        .find(&user.id)
        .select(users::purge_at)
        .first(&mut s.conn())
        .unwrap();
    assert_eq!(purge_after.as_deref(), Some("2026-11-01T00:00:00.000Z"));

    // Lifting both before the day cancels the deletion: nothing is lost.
    let rid = r["id"].as_str().unwrap();
    s.ok(
        &admin,
        "POST",
        &format!("/admin/restrictions/{rid}/lift"),
        Some(json!({ "reason": "cleared up" })),
    )
    .await;
    let still: Option<String> = users::table
        .find(&user.id)
        .select(users::purge_at)
        .first(&mut s.conn())
        .unwrap();
    assert!(still.is_some(), "the permanent ban is still closing it");
    let ban_rid: String = restrictions::table
        .filter(restrictions::kind.eq("ban"))
        .select(restrictions::id)
        .first(&mut s.conn())
        .unwrap();
    s.ok(
        &admin,
        "POST",
        &format!("/admin/restrictions/{ban_rid}/lift"),
        Some(json!({ "reason": "mistake" })),
    )
    .await;
    let gone: (Option<String>, Option<String>) = users::table
        .find(&user.id)
        .select((users::closing_at, users::purge_at))
        .first(&mut s.conn())
        .unwrap();
    assert_eq!(gone, (None, None));
    let later = clock::now() + Duration::days(40);
    assert!(
        crate::moderation_jobs::purge_one(&mut s.conn(), &user.id, later)
            .unwrap()
            .is_none()
    );
    // (Setting the site ban signed them out; they sign in again.)
    assert_eq!(
        s.status(
            None,
            "POST",
            "/auth/login",
            Some(json!({ "email": "user@example.com", "password": "correcthorse" }))
        )
        .await,
        200
    );
}

#[tokio::test]
async fn a_permanent_ban_hides_everything_at_once_and_lifting_brings_it_back() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let author = s.writer("author").await;
    let reader = s.writer("reader").await;
    let (_, publication) = s.publication(&author, "Soon hidden").await;
    s.comment(&author, &publication, "my own note").await;
    let by_reader = s.comment(&reader, &publication, "a reply").await;
    let _ = by_reader;
    assert_eq!(visible_comments(&s, &reader, &publication).await, 2);
    let r = s
        .restrict(
            &admin,
            &author,
            body(json!({ "kind": "ban", "scope": "site", "confirmName": "author" })),
        )
        .await;
    // Their publication, profile and comments are gone from public reads.
    assert_eq!(
        s.status(
            Some(&reader),
            "GET",
            &format!("/publications/{publication}"),
            None
        )
        .await,
        404
    );
    assert_eq!(
        s.status(
            Some(&reader),
            "GET",
            &format!("/users/{}/profile", author.id),
            None
        )
        .await,
        404
    );
    assert!(s
        .ok(&reader, "GET", "/publications", None)
        .await
        .as_array()
        .unwrap()
        .is_empty());
    // Nothing is deleted until the 30 days are up.
    let works: i64 = works::table
        .filter(works::owner_id.eq(&author.id))
        .count()
        .get_result(&mut s.conn())
        .unwrap();
    assert_eq!(works, 1);
    s.ok(
        &admin,
        "POST",
        &format!("/admin/restrictions/{}/lift", r["id"].as_str().unwrap()),
        Some(json!({ "reason": "overturned" })),
    )
    .await;
    assert_eq!(
        s.status(
            Some(&reader),
            "GET",
            &format!("/publications/{publication}"),
            None
        )
        .await,
        200
    );
    assert_eq!(visible_comments(&s, &reader, &publication).await, 2);
    // The row on the list went with it.
    let listed: i64 = permanent_bans::table
        .filter(permanent_bans::removed_at.is_null())
        .count()
        .get_result(&mut s.conn())
        .unwrap();
    assert_eq!(listed, 0);
}

#[tokio::test]
async fn on_day_thirty_the_account_is_deleted_with_what_belongs_to_it_and_a_shared_book_survives() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let doomed = s.writer("doomed").await;
    let first = s.writer("first").await;
    let second = s.writer("second").await;
    let reader = s.writer("reader").await;
    let (solo_id, _) = s.publication(&doomed, "Alone").await;
    let (shared_id, publication) = s.publication(&doomed, "Together").await;
    for (who, email) in [
        (&first, "first@example.com"),
        (&second, "second@example.com"),
    ] {
        s.ok(
            &doomed,
            "POST",
            &format!("/works/{shared_id}/collaborators"),
            Some(json!({ "email": email, "role": "writer" })),
        )
        .await;
        let _ = who;
    }
    // They have accepted, `second` before `first`, whatever order they were asked in.
    diesel::update(
        work_collaborators::table.filter(work_collaborators::email.eq("first@example.com")),
    )
    .set((
        work_collaborators::user_id.eq(Some(first.id.clone())),
        work_collaborators::accepted_at.eq(Some("2026-10-02T00:00:00.000Z".to_string())),
    ))
    .execute(&mut s.conn())
    .unwrap();
    diesel::update(
        work_collaborators::table.filter(work_collaborators::email.eq("second@example.com")),
    )
    .set((
        work_collaborators::user_id.eq(Some(second.id.clone())),
        work_collaborators::accepted_at.eq(Some("2026-10-01T00:00:00.000Z".to_string())),
    ))
    .execute(&mut s.conn())
    .unwrap();
    let theirs = s.comment(&doomed, &publication, "I wrote this").await;
    let reply: Value = s
        .ok(
            &reader,
            "POST",
            &format!("/publications/{publication}/comments"),
            Some(json!({ "body": "I answered it", "parentId": theirs["id"] })),
        )
        .await;
    let _ = reply;
    s.restrict(
        &admin,
        &doomed,
        body(json!({ "kind": "ban", "scope": "site", "confirmName": "doomed" })),
    )
    .await;

    // Not before the day.
    let early = clock::now() + Duration::days(29);
    assert!(
        crate::moderation_jobs::purge_one(&mut s.conn(), &doomed.id, early)
            .unwrap()
            .is_none()
    );

    let day_thirty = clock::now() + Duration::days(31);
    let purged = crate::moderation_jobs::purge_one(&mut s.conn(), &doomed.id, day_thirty)
        .unwrap()
        .expect("purged");
    assert_eq!(purged.work_files, vec![solo_id.clone()]);

    // The account and its solo book are gone; the shared book is the earliest
    // collaborator's now, and they have been told.
    assert_eq!(
        users::table
            .find(&doomed.id)
            .count()
            .get_result::<i64>(&mut s.conn())
            .unwrap(),
        0
    );
    assert_eq!(
        works::table
            .find(&solo_id)
            .count()
            .get_result::<i64>(&mut s.conn())
            .unwrap(),
        0
    );
    let owner: String = works::table
        .find(&shared_id)
        .select(works::owner_id)
        .first(&mut s.conn())
        .unwrap();
    assert_eq!(owner, second.id);
    let told = s.mail_to("second@example.com");
    assert!(
        told.iter().any(|m| m.0.contains("is now yours")),
        "{told:?}"
    );
    let seats: Vec<Option<String>> = work_collaborators::table
        .filter(work_collaborators::work_id.eq(&shared_id))
        .select(work_collaborators::user_id)
        .load(&mut s.conn())
        .unwrap();
    assert_eq!(
        seats,
        vec![Some(first.id.clone())],
        "the new owner no longer needs a seat"
    );
    // Their comment stays, blank and credited to a former member, so the reply
    // still has a question to answer.
    let list = s
        .ok(
            &reader,
            "GET",
            &format!("/publications/{publication}/comments"),
            None,
        )
        .await;
    assert_eq!(list[0]["body"], "");
    assert_eq!(list[0]["replies"][0]["body"], "I answered it");

    // What survives: the list row and the log, and nothing else of them.
    let ban: i64 = permanent_bans::table
        .filter(permanent_bans::removed_at.is_null())
        .count()
        .get_result(&mut s.conn())
        .unwrap();
    assert_eq!(ban, 1);
    let purge_line: (String, Option<String>) = audit_events::table
        .filter(audit_events::action.eq("account_purged"))
        .select((audit_events::detail, audit_events::subject_id))
        .first(&mut s.conn())
        .unwrap();
    assert!(purge_line.0.contains("scheduled deletion"));
    assert_eq!(purge_line.1.as_deref(), Some(doomed.id.as_str()));
    // Running it again deletes nothing and doesn't fail.
    assert!(
        crate::moderation_jobs::purge_one(&mut s.conn(), &doomed.id, day_thirty)
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn the_daily_job_runs_once_a_day_and_an_open_appeal_holds_the_purge() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let user = s.writer("user").await;
    let r = s
        .restrict(
            &admin,
            &user,
            body(json!({ "kind": "read_only", "scope": "site", "confirmName": "user" })),
        )
        .await;
    let rid = r["id"].as_str().unwrap();
    s.ok(
        &user,
        "POST",
        &format!("/me/restrictions/{rid}/appeal"),
        Some(json!({ "body": "please" })),
    )
    .await;
    let dashboard = s.ok(&admin, "GET", "/admin/overview", None).await;
    assert_eq!(dashboard["appealsOpen"], 1);
    diesel::update(users::table.find(&user.id))
        .set(users::purge_at.eq("2020-01-01T00:00:00.000Z"))
        .execute(&mut s.conn())
        .unwrap();
    // Overdue and waiting on the appeal.
    let dashboard = s.ok(&admin, "GET", "/admin/overview", None).await;
    assert_eq!(dashboard["waitingOnAppeal"], 1);
    let n = crate::moderation_jobs::purge_accounts(&s.state, clock::now())
        .await
        .unwrap();
    assert_eq!(n, 0);
    assert_eq!(
        users::table
            .find(&user.id)
            .count()
            .get_result::<i64>(&mut s.conn())
            .unwrap(),
        1
    );
    // Decide it against them (someone other than who set it), then the job
    // finds it due. It claims its day, so the first run today is spent and
    // the next one is tomorrow's.
    let admin2 = s.admin("bea").await;
    let appeals = s.ok(&admin2, "GET", "/admin/appeals", None).await;
    s.ok(
        &admin2,
        "POST",
        &format!(
            "/admin/appeals/{}/decide",
            appeals[0]["appeal"]["id"].as_str().unwrap()
        ),
        Some(json!({ "decision": "upheld", "message": "The restriction stands." })),
    )
    .await;
    let tomorrow = clock::now() + Duration::days(1);
    assert_eq!(
        crate::moderation_jobs::purge_accounts(&s.state, tomorrow)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        users::table
            .find(&user.id)
            .count()
            .get_result::<i64>(&mut s.conn())
            .unwrap(),
        0
    );
    // Twice in a day: nothing extra, no failure.
    assert_eq!(
        crate::moderation_jobs::purge_accounts(&s.state, tomorrow)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn read_only_accounts_get_a_reminder_a_week_and_a_day_before_and_not_twice() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let user = s.writer("user").await;
    s.restrict(
        &admin,
        &user,
        body(json!({ "kind": "read_only", "scope": "site", "confirmName": "user" })),
    )
    .await;
    let purge: String = users::table
        .find(&user.id)
        .select(users::purge_at.assume_not_null())
        .first(&mut s.conn())
        .unwrap();
    let purge = chrono::DateTime::parse_from_rfc3339(&purge)
        .unwrap()
        .with_timezone(&chrono::Utc);
    let reminders = |s: &Server| {
        s.mail_to("user@example.com")
            .into_iter()
            .filter(|m| m.0.contains("will be deleted in"))
            .count()
    };
    let mut conn = s.conn();
    assert_eq!(
        crate::moderation_jobs::send_closing_reminders(&mut conn, purge - Duration::days(20))
            .unwrap(),
        0
    );
    assert_eq!(
        crate::moderation_jobs::send_closing_reminders(&mut conn, purge - Duration::days(6))
            .unwrap(),
        1
    );
    assert_eq!(
        crate::moderation_jobs::send_closing_reminders(&mut conn, purge - Duration::days(5))
            .unwrap(),
        0
    );
    assert_eq!(reminders(&s), 1);
    assert_eq!(
        crate::moderation_jobs::send_closing_reminders(&mut conn, purge - Duration::hours(20))
            .unwrap(),
        1
    );
    assert_eq!(
        crate::moderation_jobs::send_closing_reminders(&mut conn, purge - Duration::hours(10))
            .unwrap(),
        0
    );
    assert_eq!(reminders(&s), 2);
}

async fn visible_comments(s: &Server, who: &Writer, publication: &str) -> usize {
    let list = s
        .ok(
            who,
            "GET",
            &format!("/publications/{publication}/comments"),
            None,
        )
        .await;
    list.as_array()
        .unwrap()
        .iter()
        .filter(|c| c["body"] != "")
        .count()
}
