//! Site administration through the API, end to end: staff roles, managing
//! accounts, restrictions and their enforcement, appeals, the permanent ban
//! list, and closing an account.
//!
//! Time is moved by editing the timestamps in the database, not the process
//! clock, because the tests share a process and the clock is global.
use diesel::prelude::*;
use serde_json::{json, Value};

use super::prompts_api_tests::{Server, Writer};
use crate::clock;
use crate::db::schema::*;

impl Server {
    /// A writer who is an administrator.
    pub(super) async fn admin(&self, name: &str) -> Writer {
        let w = self.writer(name).await;
        diesel::update(users::table.find(&w.id))
            .set(users::is_admin.eq(true))
            .execute(&mut self.conn())
            .unwrap();
        w
    }

    pub(super) async fn moderator(&self, name: &str) -> Writer {
        let w = self.writer(name).await;
        diesel::update(users::table.find(&w.id))
            .set(users::is_moderator.eq(true))
            .execute(&mut self.conn())
            .unwrap();
        w
    }

    /// Mail queued for `to`, newest last: (subject, body).
    pub(super) fn mail_to(&self, to: &str) -> Vec<(String, String)> {
        mail_queue::table
            .filter(mail_queue::to_address.eq(to))
            .order(mail_queue::created_at.asc())
            .select((mail_queue::subject, mail_queue::body))
            .load(&mut self.conn())
            .unwrap()
    }

    /// The token in the last link in a body, after `?token=`.
    pub(super) fn token_in(body: &str) -> String {
        body.split("token=")
            .nth(1)
            .expect("a link with a token")
            .split(|c: char| !c.is_ascii_alphanumeric())
            .next()
            .unwrap()
            .to_string()
    }

    /// A short-story publication, public, with one scene. Returns the work id
    /// and the publication id.
    pub(super) async fn publication(&self, owner: &Writer, title: &str) -> (String, String) {
        let work = self
            .ok(
                owner,
                "POST",
                "/works",
                Some(json!({ "title": title, "kind": "short_story" })),
            )
            .await;
        let work_id = work["id"].as_str().unwrap().to_string();
        let version = self
            .ok(
                owner,
                "POST",
                &format!("/works/{work_id}/versions"),
                Some(json!({ "label": "v1" })),
            )
            .await;
        let version_id = version["id"].as_str().unwrap().to_string();
        self.ok(
            owner,
            "POST",
            &format!("/versions/{version_id}/publish"),
            Some(json!({})),
        )
        .await;
        let publication = self
            .ok(
                owner,
                "POST",
                &format!("/versions/{version_id}/publications"),
                Some(json!({ "visibility": "public" })),
            )
            .await;
        (work_id, publication["id"].as_str().unwrap().to_string())
    }

    pub(super) async fn comment(&self, who: &Writer, publication: &str, text: &str) -> Value {
        self.ok(
            who,
            "POST",
            &format!("/publications/{publication}/comments"),
            Some(json!({ "body": text })),
        )
        .await
    }

    /// Restricts `user` and returns the restriction.
    pub(super) async fn restrict(&self, staff: &Writer, user: &Writer, body: Value) -> Value {
        self.ok(
            staff,
            "POST",
            &format!("/admin/users/{}/restrictions", user.id),
            Some(body),
        )
        .await
    }
}

fn guideline(extra: Value) -> Value {
    let mut base = json!({
        "reasonCode": "3.2",
        "reasonText": "Please keep it kind.",
    });
    for (k, v) in extra.as_object().unwrap() {
        base[k] = v.clone();
    }
    base
}

// --- staff roles ---------------------------------------------------------------

/// Every `/api/admin/*` route and whether a moderator is let in. A route that
/// is not here fails `every_admin_route_is_in_the_role_table`.
const ROLE_TABLE: &[(&str, &str, bool)] = &[
    ("get", "/api/admin/overview", true),
    ("get", "/api/admin/users", true),
    ("post", "/api/admin/users", false),
    ("get", "/api/admin/users/{user_id}", true),
    ("patch", "/api/admin/users/{user_id}", true), // text fixes only; the rest is checked in the handler
    ("delete", "/api/admin/users/{user_id}", false),
    ("post", "/api/admin/users/{user_id}/notes", true),
    ("post", "/api/admin/users/{user_id}/moderator", false),
    ("post", "/api/admin/users/{user_id}/setup-email", false),
    ("post", "/api/admin/users/{user_id}/password-reset", false),
    (
        "post",
        "/api/admin/users/{user_id}/sessions/revoke-all",
        false,
    ),
    (
        "delete",
        "/api/admin/users/{user_id}/sessions/{session_id}",
        false,
    ),
    ("post", "/api/admin/users/{user_id}/deactivate", false),
    ("post", "/api/admin/users/{user_id}/reactivate", false),
    // Sign-up addresses are PII, and bulk deactivation is account management.
    ("get", "/api/admin/signups", false),
    ("post", "/api/admin/users/bulk-deactivate", false),
    ("post", "/api/admin/users/{user_id}/delete-content", false),
    ("post", "/api/admin/users/{user_id}/restrictions", true),
    ("patch", "/api/admin/restrictions/{restriction_id}", true),
    (
        "post",
        "/api/admin/restrictions/{restriction_id}/lift",
        true,
    ),
    ("get", "/api/admin/appeals", true),
    ("post", "/api/admin/appeals/{appeal_id}/decide", true),
    ("get", "/api/admin/permanent-bans", false),
    ("post", "/api/admin/permanent-bans", false),
    ("post", "/api/admin/permanent-bans/{ban_id}/remove", false),
    ("get", "/api/admin/blocked-domains", false),
    ("post", "/api/admin/blocked-domains", false),
    ("delete", "/api/admin/blocked-domains/{domain}", false),
    ("get", "/api/admin/queue", true),
    ("post", "/api/admin/queue/dismiss", true),
    ("post", "/api/admin/queue/{item_id}/claim", true),
    ("post", "/api/admin/queue/{item_id}/dismiss", true),
    ("post", "/api/admin/queue/{item_id}/remove", true),
    ("post", "/api/admin/queue/{item_id}/restore", true),
    ("post", "/api/admin/queue/{item_id}/hold", true),
    ("post", "/api/admin/queue/{item_id}/release", true),
    ("get", "/api/admin/words", false),
    ("post", "/api/admin/words", false),
    ("patch", "/api/admin/words/{term_id}", false),
    ("delete", "/api/admin/words/{term_id}", false),
    ("post", "/api/admin/words/test", false),
    ("post", "/api/admin/words/import", false),
    ("get", "/api/admin/words/export", false),
    ("post", "/api/admin/words/rescan", false),
    ("post", "/api/admin/words/allow", false),
    ("delete", "/api/admin/words/allow/{word}", false),
    ("get", "/api/admin/external", false),
    ("put", "/api/admin/external", false),
    ("post", "/api/admin/external/test", false),
    ("get", "/api/admin/moderation-settings", false),
    ("put", "/api/admin/moderation-settings", false),
    ("get", "/api/admin/log", true),
    ("get", "/api/admin/log.csv", false),
    // The Daily Prompts pages are the administrators' alone.
    ("get", "/api/admin/prompts", false),
    ("post", "/api/admin/prompts", false),
    ("patch", "/api/admin/prompts/{prompt_id}", false),
    ("delete", "/api/admin/prompts/{prompt_id}", false),
    ("post", "/api/admin/prompts/reorder", false),
    ("get", "/api/admin/jobs", false),
];

#[test]
fn every_admin_route_is_in_the_role_table() {
    let doc = crate::routes::openapi_document();
    let mut real = std::collections::BTreeSet::new();
    for (path, item) in &doc.paths.paths {
        if !path.starts_with("/api/admin/") {
            continue;
        }
        for (method, present) in [
            ("get", item.get.is_some()),
            ("post", item.post.is_some()),
            ("put", item.put.is_some()),
            ("patch", item.patch.is_some()),
            ("delete", item.delete.is_some()),
        ] {
            if present {
                real.insert((method.to_string(), path.clone()));
            }
        }
    }
    let table: std::collections::BTreeSet<(String, String)> = ROLE_TABLE
        .iter()
        .map(|(m, p, _)| (m.to_string(), p.to_string()))
        .collect();
    assert_eq!(
        real, table,
        "the role table and the admin routes have drifted"
    );
}

#[tokio::test]
async fn each_staff_route_answers_to_the_role_table() {
    let s = Server::start().await;
    let writer = s.writer("wren").await;
    let moderator = s.moderator("mo").await;
    let admin = s.admin("ada").await;
    for (method, path, moderator_allowed) in ROLE_TABLE {
        let url = path
            .trim_start_matches("/api")
            .replace("{user_id}", &writer.id);
        let url = concrete_path(&url);
        for (who, name, allowed) in [
            (None, "signed out", false),
            (Some(&writer), "a writer", false),
            (Some(&moderator), "a moderator", *moderator_allowed),
            (Some(&admin), "an admin", true),
        ] {
            let body = if *method == "get" {
                None
            } else if *method == "patch" && *path == "/api/admin/users/{user_id}" {
                // A mixed route: a moderator may fix text, nothing else.
                Some(json!({ "displayName": "x", "reason": "r" }))
            } else {
                Some(universal_body())
            };
            let (status, text) = s.call(who, &method.to_uppercase(), &url, body).await;
            if allowed {
                assert!(
                    status != 403 && status != 401,
                    "{name} should be let into {method} {path}, got {status}: {text}"
                );
            } else {
                assert_eq!(
                    status, 403,
                    "{name} should be refused {method} {path}: {text}"
                );
            }
        }
    }
}

/// One JSON body that satisfies every staff route's request type, so that a
/// refusal in the table is the permission check and not a malformed body.
fn universal_body() -> Value {
    json!({
        "email": "someone@example.com", "displayName": "x", "reason": "r", "moderator": true,
        "body": "x", "term": "x", "action": "flag", "appliesTo": ["comment"], "word": "x",
        "domain": "x.example", "reasonCode": "1", "reasonText": "x", "kind": "warning",
        "scope": "site", "text": "x", "csv": "", "provider": "none", "surfaces": [],
        "confirmName": "x", "decision": "upheld", "message": "x", "ids": [], "itemIds": [],
        "source": "x", "signOut": false,
        "thresholds": {
            "reporters": 3, "reporterDays": 7, "removals": 2, "removalDays": 30,
            "commentsFirstHour": 5
        },
        "ladder": ["warning"],
    })
}

/// `{param}` segments become `x`.
fn concrete_path(path: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in path.chars() {
        match c {
            '{' => {
                inside = true;
                out.push('x');
            }
            '}' => inside = false,
            c if !inside => out.push(c),
            _ => {}
        }
    }
    out
}

#[tokio::test]
async fn nobody_makes_themselves_staff_through_a_request_body() {
    let s = Server::start().await;
    let w = s.writer("wren").await;
    s.ok(
        &w,
        "PATCH",
        "/me/profile",
        Some(json!({ "isModerator": true, "isAdmin": true, "displayName": "Wren" })),
    )
    .await;
    s.ok(
        &w,
        "PATCH",
        "/me/settings",
        Some(json!({ "isModerator": true })),
    )
    .await;
    let me = s.ok(&w, "GET", "/auth/me", None).await;
    assert_eq!(me["isModerator"], false);
    assert_eq!(me["isAdmin"], false);
    assert_eq!(s.status(Some(&w), "GET", "/admin/queue", None).await, 403);
}

#[tokio::test]
async fn staff_cannot_act_on_themselves_or_each_other() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let other_admin = s.admin("bea").await;
    let mo = s.moderator("mo").await;
    let mo2 = s.moderator("mo2").await;
    let body = guideline(json!({ "kind": "suspend", "scope": "discussion", "days": 3 }));
    // Not themselves.
    for (who, target) in [(&admin, &admin), (&mo, &mo)] {
        assert_eq!(
            s.status(
                Some(who),
                "POST",
                &format!("/admin/users/{}/restrictions", target.id),
                Some(body.clone())
            )
            .await,
            403
        );
    }
    // Nor edit or delete themselves.
    assert_eq!(
        s.status(
            Some(&admin),
            "PATCH",
            &format!("/admin/users/{}", admin.id),
            Some(json!({ "displayName": "Me", "reason": "x" }))
        )
        .await,
        403
    );
    assert_eq!(
        s.status(
            Some(&admin),
            "DELETE",
            &format!("/admin/users/{}", admin.id),
            Some(json!({ "confirmName": "ada", "reason": "x" }))
        )
        .await,
        403
    );
    // Not another admin; and a moderator can't touch a moderator.
    assert_eq!(
        s.status(
            Some(&admin),
            "POST",
            &format!("/admin/users/{}/restrictions", other_admin.id),
            Some(body.clone())
        )
        .await,
        403
    );
    assert_eq!(
        s.status(
            Some(&mo),
            "POST",
            &format!("/admin/users/{}/restrictions", mo2.id),
            Some(body.clone())
        )
        .await,
        403
    );
    // An admin can act on a moderator.
    assert_eq!(
        s.status(
            Some(&admin),
            "POST",
            &format!("/admin/users/{}/restrictions", mo.id),
            Some(body.clone())
        )
        .await,
        200
    );
    // And an admin makes and unmakes a moderator, with the grant recorded.
    let writer = s.writer("wren").await;
    s.ok(
        &admin,
        "POST",
        &format!("/admin/users/{}/moderator", writer.id),
        Some(json!({ "moderator": true, "reason": "joins the team" })),
    )
    .await;
    assert_eq!(
        s.status(Some(&writer), "GET", "/admin/queue", None).await,
        200
    );
    s.ok(
        &admin,
        "POST",
        &format!("/admin/users/{}/moderator", writer.id),
        Some(json!({ "moderator": false, "reason": "left" })),
    )
    .await;
    assert_eq!(
        s.status(Some(&writer), "GET", "/admin/queue", None).await,
        403
    );
    let actions: Vec<String> = audit_events::table
        .filter(audit_events::subject_id.eq(&writer.id))
        .select(audit_events::action)
        .load(&mut s.conn())
        .unwrap();
    assert!(actions.contains(&"moderator_granted".to_string()));
    assert!(actions.contains(&"moderator_revoked".to_string()));
}

#[tokio::test]
async fn a_moderator_never_sees_an_address_or_any_other_personal_detail() {
    let s = Server::start().await;
    let mo = s.moderator("mo").await;
    let admin = s.admin("ada").await;
    let target = s.writer("tess").await;
    diesel::update(users::table.find(&target.id))
        .set((
            users::legal_name.eq(Some("Tessa Quillfeather")),
            users::mailing_address.eq(Some("12 Inkwell Lane")),
            users::phone.eq(Some("555-0142")),
        ))
        .execute(&mut s.conn())
        .unwrap();
    let (_, publication) = s.publication(&target, "A Short Thing").await;
    let reader = s.writer("rae").await;
    let comment = s.comment(&target, &publication, "hello").await;
    s.restrict(
        &admin,
        &target,
        guideline(json!({ "kind": "suspend", "scope": "discussion", "days": 7 })),
    )
    .await;
    s.ok(
        &reader,
        "POST",
        &format!("/comments/{}/report", comment["id"].as_str().unwrap()),
        Some(json!({ "reason": "abuse" })),
    )
    .await;

    let forbidden = [
        "tess@example.com",
        "Tessa Quillfeather",
        "12 Inkwell Lane",
        "555-0142",
        "rae@example.com",
        "legalName",
        "mailingAddress",
        "\"phone\"",
        "\"ip\"",
        "127.0.0.1",
    ];
    for path in [
        "/admin/users".to_string(),
        format!("/admin/users/{}", target.id),
        "/admin/queue".to_string(),
        "/admin/queue?tab=done".to_string(),
        "/admin/log".to_string(),
        "/admin/appeals".to_string(),
    ] {
        let body = s.ok(&mo, "GET", &path, None).await.to_string();
        for needle in forbidden {
            // Field names for details a moderator never gets are absent or
            // null; the values must never appear.
            if needle.starts_with('"') || needle.ends_with("Name") || needle.ends_with("Address") {
                continue;
            }
            assert!(
                !body.contains(needle),
                "{path} shows a moderator {needle}: {body}"
            );
        }
        assert!(!body.contains("\"sessions\":["), "{path}");
    }
    // The detail page names those fields as null, and has no sessions.
    let detail = s
        .ok(&mo, "GET", &format!("/admin/users/{}", target.id), None)
        .await;
    assert!(detail["summary"]["email"].is_null());
    assert!(detail["summary"]["legalName"].is_null());
    assert!(detail["summary"]["phone"].is_null());
    assert!(detail["sessions"].is_null());
    // An administrator gets all of it.
    let detail = s
        .ok(&admin, "GET", &format!("/admin/users/{}", target.id), None)
        .await;
    assert_eq!(detail["summary"]["email"], "tess@example.com");
    assert_eq!(detail["summary"]["legalName"], "Tessa Quillfeather");
    assert!(!detail["sessions"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn searching_finds_names_and_for_admins_addresses_and_ids() {
    let s = Server::start().await;
    let mo = s.moderator("mo").await;
    let admin = s.admin("ada").await;
    let tess = s.writer("tessellate").await;
    s.writer("other").await;

    let names = |v: &Value| -> Vec<String> {
        v["users"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["displayName"].as_str().unwrap().to_string())
            .collect()
    };
    let by_part = s.ok(&admin, "GET", "/admin/users?q=ssell", None).await;
    assert_eq!(names(&by_part), vec!["tessellate"]);
    let by_email = s
        .ok(
            &admin,
            "GET",
            "/admin/users?q=tessellate%40example.com",
            None,
        )
        .await;
    assert_eq!(names(&by_email), vec!["tessellate"]);
    let by_id = s
        .ok(&admin, "GET", &format!("/admin/users?q={}", tess.id), None)
        .await;
    assert_eq!(names(&by_id), vec!["tessellate"]);

    // A moderator searching an address finds nothing, and is not told whether
    // it exists — the same empty list as for an address nobody has.
    let a = s
        .ok(&mo, "GET", "/admin/users?q=tessellate%40example.com", None)
        .await;
    let b = s
        .ok(&mo, "GET", "/admin/users?q=nobody%40example.com", None)
        .await;
    assert_eq!(a, b);
    assert!(names(&a).is_empty());
    let by_name = s.ok(&mo, "GET", "/admin/users?q=tessel", None).await;
    assert_eq!(names(&by_name), vec!["tessellate"]);
}

#[tokio::test]
async fn paging_does_not_repeat_or_skip_when_accounts_arrive_between_pages() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    for i in 0..55 {
        let id = uuid::Uuid::new_v4().to_string();
        diesel::insert_into(users::table)
            .values((
                users::id.eq(&id),
                users::email.eq(format!("p{i:02}@example.com")),
                users::display_name.eq(format!("Person {i:02}")),
                users::password_hash.eq("!"),
                users::created_at.eq(format!("2026-01-01T00:{:02}:00.000Z", i)),
                users::updated_at.eq("2026-01-01T00:00:00.000Z"),
            ))
            .execute(&mut s.conn())
            .unwrap();
    }
    let first = s.ok(&admin, "GET", "/admin/users", None).await;
    let first_ids: Vec<String> = first["users"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(first_ids.len(), 50);
    let cursor = first["nextCursor"].as_str().unwrap().to_string();
    // Someone signs up between the pages.
    s.writer("latecomer").await;
    let second = s
        .ok(
            &admin,
            "GET",
            &format!("/admin/users?cursor={}", urlencode(&cursor)),
            None,
        )
        .await;
    let second_ids: Vec<String> = second["users"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["id"].as_str().unwrap().to_string())
        .collect();
    assert!(
        second_ids.iter().all(|id| !first_ids.contains(id)),
        "a row appeared twice"
    );
    // 55 made here + ada + the former member placeholder is excluded + the
    // latecomer, who sorts first and so is on neither page after the cursor.
    assert_eq!(first_ids.len() + second_ids.len(), 56);
    assert!(second["nextCursor"].is_null());
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[tokio::test]
async fn the_list_filters_by_status_role_and_whether_the_address_is_confirmed() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let mo = s.moderator("mo").await;
    let banned = s.writer("banned").await;
    let quiet = s.writer("quiet").await;
    let gone = s.writer("gone").await;
    s.restrict(
        &admin,
        &banned,
        guideline(json!({ "kind": "ban", "scope": "site", "days": 7 })),
    )
    .await;
    s.restrict(
        &admin,
        &quiet,
        guideline(json!({ "kind": "suspend", "scope": "discussion", "days": 3 })),
    )
    .await;
    s.ok(
        &admin,
        "POST",
        &format!("/admin/users/{}/deactivate", gone.id),
        Some(json!({ "reason": "asked" })),
    )
    .await;
    diesel::update(users::table.find(&quiet.id))
        .set(users::email_verified_at.eq(None::<String>))
        .execute(&mut s.conn())
        .unwrap();

    let ids = |q: &str| {
        let s = &s;
        let admin = &admin;
        let q = q.to_string();
        async move {
            s.ok(admin, "GET", &format!("/admin/users?{q}"), None).await["users"]
                .as_array()
                .unwrap()
                .iter()
                .map(|u| u["id"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(ids("status=banned").await, vec![banned.id.clone()]);
    assert_eq!(ids("status=restricted").await, vec![quiet.id.clone()]);
    assert_eq!(ids("status=deactivated").await, vec![gone.id.clone()]);
    assert_eq!(ids("role=moderator").await, vec![mo.id.clone()]);
    assert_eq!(ids("role=admin").await, vec![admin.id.clone()]);
    assert_eq!(ids("verified=false").await, vec![quiet.id.clone()]);
    assert_eq!(
        s.status(Some(&admin), "GET", "/admin/users?status=weird", None)
            .await,
        400
    );
    let sorted = s.ok(&admin, "GET", "/admin/users?sort=reports", None).await;
    assert!(sorted["users"].as_array().unwrap().len() >= 5);
}

// --- creating, editing, resetting ---------------------------------------------------

#[tokio::test]
async fn a_made_account_is_set_up_by_its_owner_from_an_email() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let created = s
        .ok(
            &admin,
            "POST",
            "/admin/users",
            Some(json!({ "email": "Newcomer@Example.com", "displayName": "Newcomer" })),
        )
        .await;
    let body = created.to_string();
    let mail = s.mail_to("newcomer@example.com");
    assert_eq!(mail.len(), 1);
    let token = Server::token_in(&mail[0].1);
    assert!(!body.contains(&token), "the token is not in any response");

    // Can't sign in until the password is set.
    assert_eq!(
        s.status(
            None,
            "POST",
            "/auth/login",
            Some(json!({ "email": "newcomer@example.com", "password": "anything-at-all" }))
        )
        .await,
        401
    );
    // The terms have to be agreed to.
    assert_eq!(
        s.status(
            None,
            "POST",
            "/auth/setup",
            Some(json!({ "token": token, "password": "a fine password" }))
        )
        .await,
        400
    );
    let (status, session) = s
        .call(
            None,
            "POST",
            "/auth/setup",
            Some(json!({ "token": token, "password": "a fine password", "acceptTerms": true })),
        )
        .await;
    assert_eq!(status, 200, "{session}");
    assert!(session["user"]["emailVerifiedAt"].is_string());
    assert_eq!(
        session["user"]["termsVersion"],
        super::privacy::TERMS_VERSION
    );
    // Once.
    assert_eq!(
        s.status(
            None,
            "POST",
            "/auth/setup",
            Some(json!({ "token": token, "password": "another password", "acceptTerms": true }))
        )
        .await,
        400
    );
    // And now they can sign in with it.
    assert_eq!(
        s.status(
            None,
            "POST",
            "/auth/login",
            Some(json!({ "email": "newcomer@example.com", "password": "a fine password" }))
        )
        .await,
        200
    );
    // The log has one line for it.
    let lines: i64 = audit_events::table
        .filter(audit_events::action.eq("account_created_by_staff"))
        .count()
        .get_result(&mut s.conn())
        .unwrap();
    assert_eq!(lines, 1);
    // A second account at the same address is a 409.
    assert_eq!(
        s.status(
            Some(&admin),
            "POST",
            "/admin/users",
            Some(json!({ "email": "newcomer@example.com", "displayName": "Again" }))
        )
        .await,
        409
    );
}

#[tokio::test]
async fn an_expired_setup_link_says_to_ask_for_a_new_one() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let created = s
        .ok(
            &admin,
            "POST",
            "/admin/users",
            Some(json!({ "email": "late@example.com", "displayName": "Late" })),
        )
        .await;
    let token = Server::token_in(&s.mail_to("late@example.com")[0].1);
    diesel::update(email_tokens::table.filter(email_tokens::purpose.eq("setup")))
        .set(email_tokens::expires_at.eq("2020-01-01T00:00:00.000Z"))
        .execute(&mut s.conn())
        .unwrap();
    let (status, body) = s
        .call(
            None,
            "POST",
            "/auth/setup",
            Some(json!({ "token": token, "password": "a fine password", "acceptTerms": true })),
        )
        .await;
    assert_eq!(status, 400);
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("ask for a new one"));
    // Support sends a new one while the account is still pending.
    s.ok(
        &admin,
        "POST",
        &format!(
            "/admin/users/{}/setup-email",
            created["id"].as_str().unwrap()
        ),
        Some(json!({})),
    )
    .await;
    assert_eq!(s.mail_to("late@example.com").len(), 2);
}

#[tokio::test]
async fn editing_an_account_records_old_and_new_and_a_moderator_may_only_fix_text() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let mo = s.moderator("mo").await;
    let target = s.writer("tess").await;
    s.ok(
        &target,
        "PATCH",
        "/me/profile",
        Some(json!({ "bio": "I write sad poems" })),
    )
    .await;
    let path = format!("/admin/users/{}", target.id);

    // A reason is required.
    assert_eq!(
        s.status(
            Some(&mo),
            "PATCH",
            &path,
            Some(json!({ "displayName": "Tess Q" }))
        )
        .await,
        400
    );
    // A moderator can fix a name or bio with a reason…
    s.ok(
        &mo,
        "PATCH",
        &path,
        Some(json!({ "displayName": "Tess Q", "bio": "Writes", "reason": "bio broke 3.2" })),
    )
    .await;
    // …and nothing else.
    for body in [
        json!({ "email": "new@example.com", "reason": "x" }),
        json!({ "markVerified": true, "reason": "x" }),
        json!({ "resetTwoFactor": true, "reason": "x" }),
    ] {
        assert_eq!(s.status(Some(&mo), "PATCH", &path, Some(body)).await, 403);
    }
    let edits: Vec<String> = audit_events::table
        .filter(audit_events::action.eq("account_edited"))
        .select(audit_events::detail)
        .load(&mut s.conn())
        .unwrap();
    assert!(
        edits.iter().any(|d| d.contains("“tess” → “Tess Q”")),
        "{edits:?}"
    );
    assert!(
        edits
            .iter()
            .any(|d| d.contains("“I write sad poems” → “Writes”")),
        "{edits:?}"
    );

    // An address change tells both addresses and un-confirms the account.
    s.ok(
        &admin,
        "PATCH",
        &path,
        Some(json!({ "email": "tessa@example.com", "reason": "lost old inbox" })),
    )
    .await;
    assert_eq!(s.mail_to("tess@example.com").len(), 1);
    let verify = &s.mail_to("tessa@example.com")[0];
    assert!(verify.1.contains("/verify?token="));
    let detail = s.ok(&admin, "GET", &path, None).await;
    assert_eq!(detail["summary"]["emailVerified"], false);
    // The new address can't be one that already has an account.
    s.writer("other").await;
    assert_eq!(
        s.status(
            Some(&admin),
            "PATCH",
            &path,
            Some(json!({ "email": "other@example.com", "reason": "x" }))
        )
        .await,
        409
    );
    // Marking it confirmed needs a reason too.
    s.ok(
        &admin,
        "PATCH",
        &path,
        Some(json!({ "markVerified": true, "reason": "confirmed by phone" })),
    )
    .await;
    assert_eq!(
        s.ok(&admin, "GET", &path, None).await["summary"]["emailVerified"],
        true
    );
}

#[tokio::test]
async fn resetting_two_factor_signs_everyone_out_and_emails_the_account() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let target = s.writer("tess").await;
    diesel::update(users::table.find(&target.id))
        .set((
            users::totp_secret.eq(Some("SECRET")),
            users::totp_enabled_at.eq(Some(clock::now_iso())),
        ))
        .execute(&mut s.conn())
        .unwrap();
    let path = format!("/admin/users/{}", target.id);
    assert_eq!(
        s.status(
            Some(&admin),
            "PATCH",
            &path,
            Some(json!({ "resetTwoFactor": true }))
        )
        .await,
        400
    );
    s.ok(
        &admin,
        "PATCH",
        &path,
        Some(json!({ "resetTwoFactor": true, "reason": "lost phone and codes" })),
    )
    .await;
    assert_eq!(s.status(Some(&target), "GET", "/auth/me", None).await, 401);
    let row: (Option<String>, Option<String>) = users::table
        .find(&target.id)
        .select((users::totp_secret, users::totp_enabled_at))
        .first(&mut s.conn())
        .unwrap();
    assert_eq!(row, (None, None));
    assert!(s
        .mail_to("tess@example.com")
        .iter()
        .any(|m| m.0.contains("Two-factor")));
    let line: (String, Option<String>) = audit_events::table
        .filter(audit_events::action.eq("two_factor_disabled"))
        .filter(audit_events::subject_id.eq(&target.id))
        .select((audit_events::detail, audit_events::reason))
        .first(&mut s.conn())
        .unwrap();
    assert!(line.0.starts_with("by staff:"));
}

#[tokio::test]
async fn a_staff_reset_works_like_a_self_service_one_and_is_limited() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let target = s.writer("tess").await;
    let path = format!("/admin/users/{}/password-reset", target.id);
    let response = s
        .ok(&admin, "POST", &path, Some(json!({ "signOut": true })))
        .await;
    let mail = s.mail_to("tess@example.com");
    let reset = mail.iter().find(|m| m.0.contains("Resetting")).unwrap();
    let token = Server::token_in(&reset.1);
    assert!(reset.1.contains("Support sent this"));
    assert!(reset.1.contains("60 minutes"));
    assert!(!response.to_string().contains(&token));
    // Sign-out with the reset: the old token is now a 401.
    assert_eq!(s.status(Some(&target), "GET", "/auth/me", None).await, 401);
    // The link is the ordinary one: it works once.
    assert_eq!(
        s.status(
            None,
            "POST",
            "/auth/password/reset",
            Some(json!({ "token": token, "password": "brand new pass" }))
        )
        .await,
        200
    );
    assert_eq!(
        s.status(
            None,
            "POST",
            "/auth/password/reset",
            Some(json!({ "token": token, "password": "brand new pass 2" }))
        )
        .await,
        400
    );
    // Three an hour for one account.
    s.ok(&admin, "POST", &path, Some(json!({}))).await;
    s.ok(&admin, "POST", &path, Some(json!({}))).await;
    assert_eq!(
        s.status(Some(&admin), "POST", &path, Some(json!({}))).await,
        429
    );
}

#[tokio::test]
async fn revoking_one_session_or_all_of_them_takes_effect_on_the_next_request() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let target = s.writer("tess").await;
    let session_id: String = sessions::table
        .filter(sessions::user_id.eq(&target.id))
        .select(sessions::id)
        .first(&mut s.conn())
        .unwrap();
    assert_eq!(s.status(Some(&target), "GET", "/auth/me", None).await, 200);
    s.ok(
        &admin,
        "DELETE",
        &format!("/admin/users/{}/sessions/{session_id}", target.id),
        None,
    )
    .await;
    assert_eq!(s.status(Some(&target), "GET", "/auth/me", None).await, 401);
    let again = s
        .ok(
            &admin,
            "POST",
            &format!("/admin/users/{}/sessions/revoke-all", target.id),
            None,
        )
        .await;
    assert_eq!(again["ok"], true);
}

#[tokio::test]
async fn deactivating_hides_everything_and_reactivating_brings_it_back() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let author = s.writer("author").await;
    let reader = s.writer("reader").await;
    let (_, publication) = s.publication(&author, "Hidden Soon").await;
    s.comment(&reader, &publication, "I am a reader").await;
    s.ok(
        &author,
        "PATCH",
        "/me/profile",
        Some(json!({ "bio": "hello" })),
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
    assert_eq!(
        s.status(
            Some(&reader),
            "GET",
            &format!("/users/{}/profile", author.id),
            None
        )
        .await,
        200
    );
    s.ok(
        &admin,
        "POST",
        &format!("/admin/users/{}/deactivate", author.id),
        Some(json!({ "reason": "asked us to" })),
    )
    .await;
    // Can't sign in; their things are gone for everyone else.
    let (status, body) = s
        .call(
            None,
            "POST",
            "/auth/login",
            Some(json!({ "email": "author@example.com", "password": "correcthorse" })),
        )
        .await;
    assert_eq!(status, 403);
    assert_eq!(body["error"]["code"], "account_deactivated");
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
    let feed = s.ok(&reader, "GET", "/publications", None).await;
    assert!(feed.as_array().unwrap().is_empty(), "{feed}");
    s.ok(
        &admin,
        "POST",
        &format!("/admin/users/{}/reactivate", author.id),
        Some(json!({ "reason": "back" })),
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
    assert_eq!(
        s.status(
            Some(&reader),
            "GET",
            &format!("/users/{}/profile", author.id),
            None
        )
        .await,
        200
    );
}

#[tokio::test]
async fn deleting_by_staff_leaves_the_same_state_as_deleting_yourself() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let by_self = s.writer("selfie").await;
    let by_staff = s.writer("staffed").await;
    for w in [&by_self, &by_staff] {
        let (_, publication) = s.publication(w, &format!("Book of {}", w.id)).await;
        let other = s.writer(&format!("friend-{}", &w.id[..6])).await;
        s.comment(&other, &publication, "nice").await;
        s.comment(w, &publication, "thanks").await;
    }
    let before = tables(&s);

    // Self-service.
    s.ok(
        &by_self,
        "DELETE",
        "/me",
        Some(json!({ "password": "correcthorse" })),
    )
    .await;
    let after_self = tables(&s);

    // By staff: the name has to be typed, and an export is emailed first.
    let path = format!("/admin/users/{}", by_staff.id);
    assert_eq!(
        s.status(
            Some(&admin),
            "DELETE",
            &path,
            Some(json!({ "confirmName": "nope", "reason": "spam" }))
        )
        .await,
        400
    );
    s.ok(
        &admin,
        "DELETE",
        &path,
        Some(json!({ "confirmName": "staffed", "reason": "requested by email" })),
    )
    .await;
    let after_both = tables(&s);
    let sent = s.mail_to("staffed@example.com");
    assert!(
        sent.iter()
            .any(|m| m.0.contains("A copy of your Just Type Studio work")),
        "{sent:?}"
    );
    let exports: i64 = emailed_exports::table
        .count()
        .get_result(&mut s.conn())
        .unwrap();
    assert_eq!(exports, 1);

    // Each deletion removed exactly the same number of rows from each table.
    let removed_by_self: Vec<i64> = before.iter().zip(&after_self).map(|(b, a)| b - a).collect();
    let removed_by_both: Vec<i64> = after_self
        .iter()
        .zip(&after_both)
        .map(|(b, a)| b - a)
        .collect();
    assert_eq!(
        removed_by_self, removed_by_both,
        "{before:?} {after_self:?} {after_both:?}"
    );
    let line: i64 = audit_events::table
        .filter(audit_events::action.eq("account_deleted_by_staff"))
        .count()
        .get_result(&mut s.conn())
        .unwrap();
    assert_eq!(line, 1);
}

fn tables(s: &Server) -> Vec<i64> {
    let mut c = s.conn();
    vec![
        users::table.count().get_result(&mut c).unwrap(),
        works::table.count().get_result(&mut c).unwrap(),
        publications::table.count().get_result(&mut c).unwrap(),
        comments::table
            .filter(comments::state.ne("deleted"))
            .count()
            .get_result(&mut c)
            .unwrap(),
        sessions::table.count().get_result(&mut c).unwrap(),
        versions::table.count().get_result(&mut c).unwrap(),
    ]
}

#[tokio::test]
async fn staff_do_not_delete_a_co_written_book_without_the_flag() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let owner = s.writer("owner").await;
    let partner = s.writer("partner").await;
    let (work_id, _) = s.publication(&owner, "Shared").await;
    s.ok(
        &owner,
        "POST",
        &format!("/works/{work_id}/collaborators"),
        Some(json!({ "email": "partner@example.com", "role": "writer" })),
    )
    .await;
    let _ = partner;
    let path = format!("/admin/users/{}", owner.id);
    let (status, body) = s
        .call(
            Some(&admin),
            "DELETE",
            &path,
            Some(json!({ "confirmName": "owner", "reason": "r", "skipExport": true })),
        )
        .await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"]["details"]["sharedWorks"], json!(["Shared"]));
    s.ok(
        &admin,
        "DELETE",
        &path,
        Some(json!({ "confirmName": "owner", "reason": "r", "skipExport": true, "deleteSharedWorks": true })),
    )
    .await;
    assert_eq!(
        s.mail_to("owner@example.com").len(),
        0,
        "no export when skipped"
    );
}

#[tokio::test]
async fn notes_can_be_added_and_never_edited_or_removed() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let mo = s.moderator("mo").await;
    let target = s.writer("tess").await;
    let note = s
        .ok(
            &mo,
            "POST",
            &format!("/admin/users/{}/notes", target.id),
            Some(json!({ "body": "Warned in chat" })),
        )
        .await;
    let id = note["id"].as_str().unwrap();
    let detail = s
        .ok(&admin, "GET", &format!("/admin/users/{}", target.id), None)
        .await;
    assert_eq!(detail["notes"][0]["body"], "Warned in chat");
    for method in ["PATCH", "PUT", "DELETE"] {
        let status = s
            .status(
                Some(&admin),
                method,
                &format!("/admin/users/{}/notes/{id}", target.id),
                Some(json!({ "body": "x" })),
            )
            .await;
        assert!(status == 404 || status == 405, "{method} gave {status}");
    }
    let lines: i64 = audit_events::table
        .filter(audit_events::action.eq("staff_note_added"))
        .count()
        .get_result(&mut s.conn())
        .unwrap();
    assert_eq!(lines, 1);
}
