//! The sign-up guard through the API: the address checks, the limits and
//! what does and does not count against them, the CAPTCHA's two buckets, and
//! the admin summary and bulk deactivation for cleaning up afterwards.
//!
//! Every request here comes from 127.0.0.1, so "one address" is the test.
use diesel::prelude::*;
use serde_json::{json, Value};

use super::prompts_api_tests::{Server, Writer};
use crate::db::schema::*;
use crate::signup_guard::{CaptchaOutcome, SignupConfig, SignupGuard};

fn limits(f: impl FnOnce(&mut SignupConfig)) -> SignupConfig {
    let mut c = SignupConfig::off();
    f(&mut c);
    c
}

impl Server {
    async fn sign_up(&self, email: &str, token: Option<&str>) -> (u16, Value) {
        let mut body = json!({
            "email": email,
            "password": "correcthorse",
            "displayName": email.split('@').next().unwrap(),
            "acceptTerms": true,
        });
        if let Some(t) = token {
            body["turnstileToken"] = json!(t);
        }
        self.call(None, "POST", "/auth/register", Some(body)).await
    }

    fn signup_rows(&self) -> Vec<(Option<String>, String, String, bool)> {
        signup_events::table
            .order(signup_events::created_at.asc())
            .select((
                signup_events::user_id,
                signup_events::ip,
                signup_events::email_domain,
                signup_events::human_verified,
            ))
            .load(&mut self.conn())
            .unwrap()
    }
}

#[tokio::test]
async fn the_per_address_limit_refuses_with_a_wait_and_a_failed_sign_up_costs_nothing() {
    let s = Server::start_with_signup_guard(SignupGuard::new(limits(|c| c.ip_hourly = 2))).await;
    assert_eq!(s.sign_up("ann@example.com", None).await.0, 200);
    // Taken, refused and malformed addresses don't use up the allowance.
    assert_eq!(s.sign_up("ann@example.com", None).await.0, 409);
    assert_eq!(s.sign_up("bot@mailinator.com", None).await.0, 409);
    assert_eq!(s.sign_up("bot@eu.mailinator.com", None).await.0, 409);
    assert_eq!(s.sign_up("bot@nowhere", None).await.0, 400);
    assert_eq!(s.sign_up("bea@example.com", None).await.0, 200);

    let (status, refused) = s.sign_up("cy@example.com", None).await;
    assert_eq!(status, 429, "{refused}");
    assert_eq!(refused["error"]["code"], "signup_rate_limited");
    let wait = refused["error"]["details"]["retryAfter"].as_i64().unwrap();
    assert!((3000..=3600).contains(&wait), "about an hour, got {wait}");

    // Only the two accounts are recorded, each tied to its account.
    let rows = s.signup_rows();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(rows.iter().all(|(user, ip, domain, verified)| {
        user.is_some() && ip == "127.0.0.1" && domain == "example.com" && !verified
    }));
}

#[tokio::test]
async fn the_daily_and_site_wide_limits_hold_and_old_sign_ups_age_out() {
    let s = Server::start_with_signup_guard(SignupGuard::new(limits(|c| {
        c.ip_daily = 1;
        c.global_hourly = 3;
    })))
    .await;
    let age = |s: &Server| {
        let yesterday = (chrono::Utc::now() - chrono::Duration::hours(25))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        diesel::update(signup_events::table.filter(signup_events::ip.eq("127.0.0.1")))
            .set(signup_events::created_at.eq(yesterday))
            .execute(&mut s.conn())
            .unwrap();
    };
    assert_eq!(s.sign_up("ann@example.com", None).await.0, 200);
    assert_eq!(s.sign_up("bea@example.com", None).await.0, 429);
    // A day later, the first no longer counts.
    age(&s);
    assert_eq!(s.sign_up("bea@example.com", None).await.0, 200);
    age(&s);

    // Two from elsewhere in the last hour, and this one, fill the site.
    let now = crate::clock::now_iso();
    for i in 0..2 {
        diesel::insert_into(signup_events::table)
            .values((
                signup_events::id.eq(format!("other-{i}")),
                signup_events::ip.eq(format!("198.51.100.{i}")),
                signup_events::email_domain.eq("example.com"),
                signup_events::human_verified.eq(true),
                signup_events::created_at.eq(&now),
            ))
            .execute(&mut s.conn())
            .unwrap();
    }
    assert_eq!(s.sign_up("cy@example.com", None).await.0, 200);
    // As if that one came from elsewhere too, so only the site limit is full.
    diesel::update(
        signup_events::table
            .filter(signup_events::ip.eq("127.0.0.1"))
            .filter(signup_events::created_at.ge(&now)),
    )
    .set(signup_events::ip.eq("198.51.100.9"))
    .execute(&mut s.conn())
    .unwrap();
    let (status, refused) = s.sign_up("dee@example.com", None).await;
    assert_eq!(status, 429);
    assert_eq!(refused["error"]["code"], "signup_rate_limited");
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("busy"),
        "{refused}"
    );
}

#[tokio::test]
async fn with_the_captcha_on_sign_ups_without_it_get_a_tighter_limit() {
    let s = Server::start_with_signup_guard(SignupGuard::with_stub_captcha(
        limits(|c| c.unverified_ip_daily = 1),
        CaptchaOutcome::Passed,
    ))
    .await;
    let (_, policy) = s.call(None, "GET", "/auth/signup-policy", None).await;
    assert_eq!(policy["turnstileSiteKey"], "test-site-key");

    let (_, ann) = s.sign_up("ann@example.com", None).await;
    let (status, refused) = s.sign_up("bea@example.com", None).await;
    assert_eq!(status, 429);
    assert_eq!(refused["error"]["code"], "signup_captcha_required");
    // Passing the check is a different bucket.
    assert_eq!(s.sign_up("bea@example.com", Some("good")).await.0, 200);
    // So is an address a writer invited, token or not.
    diesel::insert_into(site_invites::table)
        .values((
            site_invites::id.eq("invite-1"),
            site_invites::inviter_id.eq(ann["user"]["id"].as_str().unwrap()),
            site_invites::email.eq("cy@example.com"),
            site_invites::mailed.eq(1),
            site_invites::created_at.eq(crate::clock::now_iso()),
        ))
        .execute(&mut s.conn())
        .unwrap();
    assert_eq!(s.sign_up("cy@example.com", None).await.0, 200);
    let verified: Vec<bool> = s.signup_rows().into_iter().map(|r| r.3).collect();
    assert_eq!(verified, vec![false, true, true]);
}

#[tokio::test]
async fn a_failed_captcha_is_refused_and_an_unreachable_one_counts_as_no_token() {
    let s = Server::start_with_signup_guard(SignupGuard::with_stub_captcha(
        SignupConfig::off(),
        CaptchaOutcome::Failed,
    ))
    .await;
    let (status, refused) = s.sign_up("ann@example.com", Some("bad")).await;
    assert_eq!(status, 400);
    assert_eq!(refused["error"]["code"], "captcha_failed");
    assert!(s.signup_rows().is_empty());

    let s = Server::start_with_signup_guard(SignupGuard::with_stub_captcha(
        limits(|c| c.unverified_ip_daily = 1),
        CaptchaOutcome::Unavailable,
    ))
    .await;
    assert_eq!(s.sign_up("ann@example.com", Some("any")).await.0, 200);
    assert_eq!(
        s.sign_up("bea@example.com", Some("any")).await.1["error"]["code"],
        "signup_captcha_required"
    );
}

#[tokio::test]
async fn without_a_captcha_the_policy_has_no_key() {
    let s = Server::start().await;
    let (status, policy) = s.call(None, "GET", "/auth/signup-policy", None).await;
    assert_eq!(status, 200);
    assert!(policy["turnstileSiteKey"].is_null(), "{policy}");
}

/// Accounts as a script would leave them: signed up, never confirmed.
async fn bots(s: &Server, emails: &[&str]) -> Vec<String> {
    let mut ids = Vec::new();
    for email in emails {
        let (status, made) = s.sign_up(email, None).await;
        assert_eq!(status, 200, "{made}");
        ids.push(made["user"]["id"].as_str().unwrap().to_string());
    }
    ids
}

#[tokio::test]
async fn staff_see_where_sign_ups_came_from_and_deactivate_them_in_bulk() {
    let s = Server::start().await;
    let admin = s.admin("ada").await;
    let spam = bots(
        &s,
        &["a1@spam.example", "a2@spam.example", "a3@spam.example"],
    )
    .await;
    let real = bots(&s, &["kim@good.example"]).await;
    // One of them at spam.example proved the address: left alone unless asked.
    diesel::update(users::table.find(&spam[2]))
        .set(users::email_verified_at.eq(Some(crate::clock::now_iso())))
        .execute(&mut s.conn())
        .unwrap();
    diesel::update(signup_events::table.filter(signup_events::user_id.eq(&spam[0])))
        .set(signup_events::ip.eq("203.0.113.9"))
        .execute(&mut s.conn())
        .unwrap();

    let summary = s.ok(&admin, "GET", "/admin/signups?hours=1", None).await;
    assert_eq!(summary["total"], 5, "{summary}");
    assert_eq!(
        summary["byDomain"][0],
        json!({ "key": "spam.example", "total": 3, "unverified": 2 })
    );
    assert_eq!(summary["byIp"][0]["key"], "127.0.0.1");
    assert_eq!(summary["byIp"][0]["total"], 4);

    // A moderator can't see addresses, so can't see this.
    let moderator = s.moderator("mo").await;
    assert_eq!(
        s.status(Some(&moderator), "GET", "/admin/signups", None)
            .await,
        403
    );
    let filter = json!({ "emailDomain": "spam.example" });
    assert_eq!(
        s.status(
            Some(&moderator),
            "POST",
            "/admin/users/bulk-deactivate",
            Some(filter.clone())
        )
        .await,
        403
    );
    // Something has to narrow it.
    assert_eq!(
        s.status(
            Some(&admin),
            "POST",
            "/admin/users/bulk-deactivate",
            Some(json!({}))
        )
        .await,
        400
    );

    // A dry run unless said, and it changes nothing.
    let preview = s
        .ok(
            &admin,
            "POST",
            "/admin/users/bulk-deactivate",
            Some(filter.clone()),
        )
        .await;
    assert_eq!(preview["dryRun"], true);
    assert_eq!(preview["matched"], 2);
    assert_eq!(preview["deactivated"], 0);
    let shown: Vec<&str> = preview["sample"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["email"].as_str().unwrap())
        .collect();
    assert_eq!(shown, vec!["a2@spam.example", "a1@spam.example"]);
    assert_eq!(preview["sample"][1]["signupIp"], "203.0.113.9");

    // By address, the other way to narrow it.
    let by_ip = s
        .ok(
            &admin,
            "POST",
            "/admin/users/bulk-deactivate",
            Some(json!({ "ip": "203.0.113.9" })),
        )
        .await;
    assert_eq!(by_ip["matched"], 1);

    // Acting needs a reason.
    let mut act = filter.clone();
    act["dryRun"] = json!(false);
    assert_eq!(
        s.status(
            Some(&admin),
            "POST",
            "/admin/users/bulk-deactivate",
            Some(act.clone())
        )
        .await,
        400
    );
    act["reason"] = json!("scripted sign-ups, 15 Nov");
    let done = s
        .ok(&admin, "POST", "/admin/users/bulk-deactivate", Some(act))
        .await;
    assert_eq!(done["deactivated"], 2);

    let deactivated: Vec<String> = users::table
        .filter(users::deactivated_at.is_not_null())
        .select(users::id)
        .load(&mut s.conn())
        .unwrap();
    let mut expected = vec![spam[0].clone(), spam[1].clone()];
    expected.sort();
    let mut got = deactivated.clone();
    got.sort();
    assert_eq!(got, expected);
    assert!(!deactivated.contains(&real[0]));
    // Signed out, and logged against each account with the reason.
    let bot = Writer {
        id: spam[0].clone(),
        token: String::new(),
    };
    let live_sessions: i64 = sessions::table
        .filter(sessions::user_id.eq(&bot.id))
        .filter(sessions::revoked_at.is_null())
        .count()
        .get_result(&mut s.conn())
        .unwrap();
    assert_eq!(live_sessions, 0);
    let logged: i64 = audit_events::table
        .filter(audit_events::action.eq("account_deactivated"))
        .filter(audit_events::reason.eq("scripted sign-ups, 15 Nov"))
        .count()
        .get_result(&mut s.conn())
        .unwrap();
    assert_eq!(logged, 2);

    // Gone from the summary (which now has the moderator in it too); the
    // verified one is matched only when asked.
    let summary = s.ok(&admin, "GET", "/admin/signups?hours=1", None).await;
    assert_eq!(summary["total"], 4, "{summary}");
    assert_eq!(
        summary["byDomain"][0],
        json!({ "key": "example.com", "total": 2, "unverified": 0 })
    );
    let wider = s
        .ok(
            &admin,
            "POST",
            "/admin/users/bulk-deactivate",
            Some(json!({ "emailDomain": "spam.example", "includeVerified": true })),
        )
        .await;
    assert_eq!(wider["matched"], 1);
    // Staff and the caller are never matched, however wide the filter.
    let everyone = s
        .ok(
            &admin,
            "POST",
            "/admin/users/bulk-deactivate",
            Some(json!({ "createdAfter": "2000-01-01T00:00:00Z", "includeVerified": true })),
        )
        .await;
    let ids: Vec<&str> = everyone["sample"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&admin.id.as_str()) && !ids.contains(&moderator.id.as_str()));
    assert_eq!(everyone["matched"], 2, "{everyone}");
}
