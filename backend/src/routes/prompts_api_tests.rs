//! Daily Prompts through the API, end to end, against a real server on a
//! local port: who sees what at each step of an answer's life, the inbox items
//! critiques make, the admin wall, the email job and turning an answer into a
//! project.
//!
//! Each test starts its own server with its own database, so they run in
//! parallel without sharing anything but the process.
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use diesel::prelude::*;
use serde_json::{json, Value};

use crate::clock;
use crate::db::schema::*;
use crate::state::AppState;

struct Server {
    base: String,
    state: AppState,
    root: std::path::PathBuf,
    http: reqwest::Client,
}

struct Writer {
    id: String,
    token: String,
}

impl Server {
    async fn start() -> Self {
        crate::auth::password::configure(crate::auth::password::Cost {
            memory_kib: 8_192,
            iterations: 1,
            lanes: 1,
        });
        let root = std::env::temp_dir().join(format!("justtype-prompts-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("works")).unwrap();
        let pool = crate::db::build_pool(root.join("t.db").to_str().unwrap()).unwrap();
        crate::db::run_migrations(&pool).unwrap();
        {
            let mut conn = pool.get().unwrap();
            super::privacy::seed_former_member(&mut conn, &clock::now_iso()).unwrap();
        }
        std::env::set_var("JUSTTYPE_JWT_SECRET", "prompt-api-test-secret-1234567890");
        let mut config = crate::config::Config::from_env().unwrap();
        config.data_dir = root.join("works");
        config.public_url = "http://justtype.test".into();
        config.anthropic_api_key = None;
        config.comment_rate = 0;
        let state = AppState {
            mailer: crate::mail::Mailer::Outbox(root.join("outbox.jsonl")),
            started_at: std::time::Instant::now(),
            config: Arc::new(config),
            pool,
            store: crate::storage::FileStore::new(root.join("works")),
            limiter: Arc::new(crate::limit::RateLimiter::new()),
            scene_cache: Arc::new(crate::cache::SceneCache::new(1 << 20)),
            hub: Arc::new(crate::live::Hub::default()),
            login_guard: Arc::new(crate::limit::LoginGuard::default()),
        };
        state.store.init().unwrap();
        let app = super::router(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap()
        });
        Server {
            base: format!("http://{addr}/api"),
            state,
            root,
            http: reqwest::Client::new(),
        }
    }

    fn conn(&self) -> crate::db::Conn {
        self.state.pool.get().unwrap()
    }

    async fn writer(&self, name: &str) -> Writer {
        let r: Value = self
            .http
            .post(format!("{}/auth/register", self.base))
            .json(&json!({
                "email": format!("{name}@example.com"),
                "password": "correcthorse",
                "displayName": name,
                "acceptTerms": true,
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let token = r["accessToken"]
            .as_str()
            .unwrap_or_else(|| panic!("{r}"))
            .to_string();
        let id = r["user"]["id"].as_str().unwrap().to_string();
        diesel::update(users::table.find(&id))
            .set(users::email_verified_at.eq(Some(clock::now_iso())))
            .execute(&mut self.conn())
            .unwrap();
        Writer { id, token }
    }

    async fn call(
        &self,
        who: Option<&Writer>,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let url = format!("{}{path}", self.base);
        let mut req = self.http.request(method.parse().unwrap(), url);
        if let Some(w) = who {
            req = req.bearer_auth(&w.token);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let res = req.send().await.unwrap();
        let status = res.status().as_u16();
        let text = res.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    async fn ok(&self, who: &Writer, method: &str, path: &str, body: Option<Value>) -> Value {
        let (status, value) = self.call(Some(who), method, path, body).await;
        assert!(
            (200..300).contains(&status),
            "{method} {path}: {status} {value}"
        );
        value
    }

    async fn status(
        &self,
        who: Option<&Writer>,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> u16 {
        self.call(who, method, path, body).await.0
    }

    /// Gives `day` a known prompt.
    fn prompt(&self, day: &str, body: &str) {
        diesel::insert_into(daily_prompts::table)
            .values(&crate::db::models::NewDailyPrompt {
                id: uuid::Uuid::new_v4().to_string(),
                day: Some(day.into()),
                position: None,
                body: body.into(),
                source: "curated".into(),
                created_by: None,
                created_at: clock::now_iso(),
                updated_at: clock::now_iso(),
            })
            .execute(&mut self.conn())
            .unwrap();
    }

    fn inbox(&self, user: &Writer) -> Vec<(String, String)> {
        inbox_items::table
            .filter(inbox_items::user_id.eq(&user.id))
            .order(inbox_items::created_at.asc())
            .select((inbox_items::kind, inbox_items::detail.assume_not_null()))
            .load::<(String, String)>(&mut self.conn())
            .unwrap()
            .into_iter()
            .map(|(k, d)| {
                let d: Value = serde_json::from_str(&d).unwrap();
                (k, d["role"].as_str().unwrap_or("").to_string())
            })
            .collect()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn text_doc(text: &str) -> Value {
    let paragraphs: Vec<Value> = text
        .split('\n')
        .map(|p| {
            if p.is_empty() {
                json!({ "type": "paragraph" })
            } else {
                json!({ "type": "paragraph", "content": [{ "type": "text", "text": p }] })
            }
        })
        .collect();
    json!({ "type": "doc", "content": paragraphs })
}

fn today() -> String {
    clock::day_string(super::prompts::today())
}

#[tokio::test]
async fn who_sees_an_answer_at_each_step() {
    let s = Server::start().await;
    let day = today();
    s.prompt(&day, "Write about a door.");
    let ada = s.writer("ada").await;
    let bo = s.writer("bo").await;
    let cy = s.writer("cy").await; // never joins
    let di = s.writer("di").await; // joins, never critiques

    // Not enrolled: the prompt, yes; writing and the feed, no.
    let today_view = s.ok(&cy, "GET", "/prompts/today", None).await;
    assert_eq!(today_view["prompt"]["body"], "Write about a door.");
    assert_eq!(today_view["enrolled"], false);
    assert_eq!(
        s.status(
            Some(&cy),
            "PUT",
            &format!("/prompts/{day}/entry"),
            Some(json!({ "content": text_doc("x") }))
        )
        .await,
        403
    );
    assert_eq!(
        s.status(Some(&cy), "GET", &format!("/prompts/{day}/entries"), None)
            .await,
        403
    );

    for w in [&ada, &bo, &di] {
        s.ok(
            w,
            "POST",
            "/prompts/enrollment",
            Some(json!({ "timezone": "Europe/London" })),
        )
        .await;
    }
    let local = clock::day_string(chrono::Utc::now().date_naive());
    let saved = s
        .ok(
            &ada,
            "PUT",
            &format!("/prompts/{day}/entry"),
            Some(json!({ "content": text_doc("The door was blue."), "localDay": local })),
        )
        .await;
    let entry_id = saved["id"].as_str().unwrap().to_string();
    assert_eq!(saved["state"], "draft");
    // Words written count toward the day.
    let words: i32 = writing_days::table
        .filter(writing_days::user_id.eq(&ada.id))
        .select(writing_days::net_words)
        .first(&mut s.conn())
        .unwrap();
    assert_eq!(words, 4);

    // A draft is its writer's alone.
    assert_eq!(
        s.status(
            Some(&bo),
            "GET",
            &format!("/prompt-entries/{entry_id}"),
            None
        )
        .await,
        404
    );
    let feed = s
        .ok(&bo, "GET", &format!("/prompts/{day}/entries"), None)
        .await;
    assert_eq!(feed["entries"].as_array().unwrap().len(), 0);
    assert_eq!(
        s.status(
            Some(&bo),
            "GET",
            &format!("/prompt-entries/{entry_id}/critiques"),
            None
        )
        .await,
        404
    );

    // Submitted: every enrolled writer reads it, nobody else.
    let submitted = s
        .ok(&ada, "POST", &format!("/prompts/{day}/entry/submit"), None)
        .await;
    let first_submitted = submitted["submittedAt"].as_str().unwrap().to_string();
    let feed = s
        .ok(&bo, "GET", &format!("/prompts/{day}/entries"), None)
        .await;
    assert_eq!(feed["entries"][0]["id"], entry_id.as_str());
    assert_eq!(feed["entries"][0]["late"], false);
    assert_eq!(
        s.ok(&bo, "GET", &format!("/prompt-entries/{entry_id}"), None)
            .await["prompt"],
        "Write about a door."
    );
    assert_eq!(
        s.status(
            Some(&cy),
            "GET",
            &format!("/prompt-entries/{entry_id}"),
            None
        )
        .await,
        403
    );
    assert_eq!(
        s.status(
            Some(&cy),
            "GET",
            &format!("/prompt-entries/{entry_id}/critiques"),
            None
        )
        .await,
        403
    );

    let critique = s
        .ok(
            &bo,
            "POST",
            &format!("/prompt-entries/{entry_id}/critiques"),
            Some(json!({ "body": "Why blue?" })),
        )
        .await;
    let critique_id = critique["id"].as_str().unwrap().to_string();
    assert_eq!(critique["author"]["displayName"], "bo");

    // Edited after submitting: in place, marked, and the first submission stands.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let edited = s
        .ok(
            &ada,
            "PUT",
            &format!("/prompts/{day}/entry"),
            Some(json!({ "content": text_doc("The door was red.") })),
        )
        .await;
    assert!(edited["editedAt"].is_string());
    assert_eq!(edited["submittedAt"], first_submitted.as_str());

    // Withdrawn: out of the feed; the writer sees every critique, the critic
    // their own, and nobody else any.
    s.ok(
        &ada,
        "POST",
        &format!("/prompts/{day}/entry/withdraw"),
        None,
    )
    .await;
    assert_eq!(
        s.ok(&di, "GET", &format!("/prompts/{day}/entries"), None)
            .await["entries"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        s.status(
            Some(&di),
            "GET",
            &format!("/prompt-entries/{entry_id}"),
            None
        )
        .await,
        404
    );
    let theirs = s
        .ok(
            &di,
            "GET",
            &format!("/prompt-entries/{entry_id}/critiques"),
            None,
        )
        .await;
    assert_eq!(theirs.as_array().unwrap().len(), 0);
    let critics = s
        .ok(
            &bo,
            "GET",
            &format!("/prompt-entries/{entry_id}/critiques"),
            None,
        )
        .await;
    assert_eq!(critics.as_array().unwrap().len(), 1);
    let writers = s
        .ok(
            &ada,
            "GET",
            &format!("/prompt-entries/{entry_id}/critiques"),
            None,
        )
        .await;
    assert_eq!(writers.as_array().unwrap().len(), 1);
    // The critic may still change their own; nobody can add one.
    s.ok(
        &bo,
        "PATCH",
        &format!("/prompt-critiques/{critique_id}"),
        Some(json!({ "body": "Why red?" })),
    )
    .await;
    assert_eq!(
        s.status(
            Some(&di),
            "POST",
            &format!("/prompt-entries/{entry_id}/critiques"),
            Some(json!({ "body": "hi" }))
        )
        .await,
        404
    );

    // Submitted again: everything shows again, and still the first time counts.
    let again = s
        .ok(&ada, "POST", &format!("/prompts/{day}/entry/submit"), None)
        .await;
    assert_eq!(again["submittedAt"], first_submitted.as_str());
    let all = s
        .ok(
            &di,
            "GET",
            &format!("/prompt-entries/{entry_id}/critiques"),
            None,
        )
        .await;
    assert_eq!(all[0]["body"], "Why red?");
    assert!(all[0]["editedAt"].is_string());

    // Leaving: the feed goes for the one who left; their answer stays for others.
    s.ok(&ada, "DELETE", "/prompts/enrollment", None).await;
    assert_eq!(
        s.status(Some(&ada), "GET", &format!("/prompts/{day}/entries"), None)
            .await,
        403
    );
    assert_eq!(
        s.ok(&ada, "GET", &format!("/prompt-entries/{entry_id}"), None)
            .await["mine"],
        true
    );
    assert_eq!(
        s.ok(&bo, "GET", &format!("/prompts/{day}/entries"), None)
            .await["entries"][0]["id"],
        entry_id.as_str()
    );
    let back = s.ok(&ada, "POST", "/prompts/enrollment", None).await;
    assert_eq!(back["enrolled"], true);
    assert_eq!(back["emailTimezone"], "Europe/London");
}

#[tokio::test]
async fn critiques_tell_the_right_people_once() {
    let s = Server::start().await;
    let day = today();
    s.prompt(&day, "A key.");
    let ada = s.writer("ada").await;
    let bo = s.writer("bo").await;
    let cy = s.writer("cy").await;
    for w in [&ada, &bo, &cy] {
        s.ok(w, "POST", "/prompts/enrollment", None).await;
    }
    let entry = s
        .ok(
            &ada,
            "PUT",
            &format!("/prompts/{day}/entry"),
            Some(json!({ "content": text_doc("A key.") })),
        )
        .await;
    let entry_id = entry["id"].as_str().unwrap().to_string();
    s.ok(&ada, "POST", &format!("/prompts/{day}/entry/submit"), None)
        .await;
    let path = format!("/prompt-entries/{entry_id}/critiques");

    let b1 = s
        .ok(&bo, "POST", &path, Some(json!({ "body": "Bo's" })))
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        s.inbox(&ada),
        [("prompt.critique".to_string(), String::new())]
    );
    // The writer replying to a critic: the critic hears, the writer does not.
    s.ok(
        &ada,
        "POST",
        &path,
        Some(json!({ "body": "Thanks", "parentId": b1 })),
    )
    .await;
    assert_eq!(
        s.inbox(&bo),
        [("prompt.reply".to_string(), "parent".to_string())]
    );
    assert_eq!(s.inbox(&ada).len(), 1);
    // A third writer in the thread: the critic as the parent's author, the
    // writer as the answer's, once each.
    let c1 = s
        .ok(
            &cy,
            "POST",
            &path,
            Some(json!({ "body": "Agreed", "parentId": b1 })),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(s.inbox(&bo).len(), 2);
    assert_eq!(
        s.inbox(&ada).last().unwrap(),
        &("prompt.reply".to_string(), "entry".to_string())
    );
    // Replying to yourself tells nobody.
    s.ok(
        &cy,
        "POST",
        &path,
        Some(json!({ "body": "And more", "parentId": c1 })),
    )
    .await;
    assert_eq!(s.inbox(&cy).len(), 0);
    assert_eq!(s.inbox(&ada).len(), 3);

    // The inbox route describes and links them.
    let page = s.ok(&ada, "GET", "/inbox", None).await;
    assert_eq!(page["items"].as_array().unwrap().len(), 3);
    let items = page["items"].as_array().unwrap();
    assert!(items.iter().any(|i| i["text"]
        .as_str()
        .unwrap()
        .contains("bo critiqued your answer")));
    assert!(items.iter().all(|i| i["link"]
        .as_str()
        .unwrap()
        .starts_with(&format!("/prompts/{day}/entries/{entry_id}#critique-"))));
    assert_eq!(
        s.ok(&ada, "GET", "/inbox/unread-count", None).await["count"],
        3
    );
    s.ok(&ada, "POST", "/inbox/read-all", None).await;
    assert_eq!(
        s.ok(&ada, "GET", "/inbox/unread-count", None).await["count"],
        0
    );

    // A critique with replies stays as a placeholder; one without goes, and
    // so do the items about it.
    s.ok(&bo, "DELETE", &format!("/prompt-critiques/{b1}"), None)
        .await;
    let list = s.ok(&bo, "GET", &path, None).await;
    let removed = list
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == b1.as_str())
        .unwrap();
    assert_eq!(removed["removed"], true);
    assert_eq!(removed["body"], "");
    assert!(removed["author"].is_null());
    let lone = s
        .ok(&cy, "POST", &path, Some(json!({ "body": "Lone" })))
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(s.inbox(&ada).len(), 4);
    s.ok(&cy, "DELETE", &format!("/prompt-critiques/{lone}"), None)
        .await;
    assert_eq!(s.inbox(&ada).len(), 3);
    assert!(!s
        .ok(&cy, "GET", &path, None)
        .await
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["id"] == lone.as_str()));

    // Reports go to the writer of the answer, who can remove the critique.
    s.ok(
        &bo,
        "POST",
        &format!("/prompt-critiques/{c1}/report"),
        Some(json!({ "reason": "abuse" })),
    )
    .await;
    assert!(s.inbox(&ada).iter().any(|(k, _)| k == "prompt.report"));
    let reports = s
        .ok(
            &ada,
            "GET",
            &format!("/prompt-entries/{entry_id}/reports"),
            None,
        )
        .await;
    assert_eq!(reports[0]["critique"]["body"], "Agreed");
    assert_eq!(
        s.status(
            Some(&bo),
            "GET",
            &format!("/prompt-entries/{entry_id}/reports"),
            None
        )
        .await,
        403
    );
    assert_eq!(
        s.status(
            Some(&bo),
            "DELETE",
            &format!("/prompt-critiques/{c1}"),
            None
        )
        .await,
        403
    );
    s.ok(&ada, "DELETE", &format!("/prompt-critiques/{c1}"), None)
        .await;

    // Deleting the answer takes every critique and every item about them.
    s.ok(&ada, "DELETE", &format!("/prompts/{day}/entry"), None)
        .await;
    assert!(s.inbox(&ada).is_empty() && s.inbox(&bo).is_empty() && s.inbox(&cy).is_empty());
    let left: i64 = prompt_critiques::table
        .count()
        .get_result(&mut s.conn())
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn a_thread_goes_twenty_deep() {
    let s = Server::start().await;
    let day = today();
    s.prompt(&day, "Deep.");
    let ada = s.writer("ada").await;
    let bo = s.writer("bo").await;
    for w in [&ada, &bo] {
        s.ok(w, "POST", "/prompts/enrollment", None).await;
    }
    let entry_id = s
        .ok(
            &ada,
            "PUT",
            &format!("/prompts/{day}/entry"),
            Some(json!({ "content": text_doc("Deep.") })),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    s.ok(&ada, "POST", &format!("/prompts/{day}/entry/submit"), None)
        .await;
    let path = format!("/prompt-entries/{entry_id}/critiques");
    let mut parent: Option<String> = None;
    for i in 0..20 {
        let who = if i % 2 == 0 { &bo } else { &ada };
        let made = s
            .ok(
                who,
                "POST",
                &path,
                Some(json!({ "body": format!("level {i}"), "parentId": parent })),
            )
            .await;
        parent = made["id"].as_str().map(str::to_string);
    }
    let list = s.ok(&bo, "GET", &path, None).await;
    assert_eq!(list.as_array().unwrap().len(), 20);
    assert_eq!(list[19]["body"], "level 19");
    assert_eq!(list[19]["parentId"], list[18]["id"]);
}

#[tokio::test]
async fn a_day_that_has_not_opened_is_never_shown() {
    let s = Server::start().await;
    let ada = s.writer("ada").await;
    let today = super::prompts::today();
    let tomorrow = clock::day_string(today + Duration::days(1));
    s.prompt(&clock::day_string(today), "Today's.");
    s.prompt(
        &clock::day_string(today - Duration::days(1)),
        "Yesterday's.",
    );
    s.prompt(
        &clock::day_string(today - Duration::days(2)),
        "The day before's.",
    );
    s.prompt(&tomorrow, "Tomorrow's secret.");
    assert_eq!(
        s.status(Some(&ada), "GET", &format!("/prompts/{tomorrow}"), None)
            .await,
        404
    );
    assert_eq!(
        s.status(
            Some(&ada),
            "GET",
            &format!("/prompts/{tomorrow}/entry"),
            None
        )
        .await,
        404
    );
    let far = clock::day_string(today + Duration::days(30));
    let archive = s
        .ok(&ada, "GET", &format!("/prompts?before={far}&limit=1"), None)
        .await;
    let bodies: Vec<&str> = archive["prompts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["body"].as_str().unwrap())
        .collect();
    assert_eq!(bodies, ["Yesterday's."]);
    // Paging back reaches every past day.
    let next = archive["nextBefore"].as_str().unwrap();
    let older = s
        .ok(
            &ada,
            "GET",
            &format!("/prompts?before={next}&limit=1"),
            None,
        )
        .await;
    assert_eq!(older["prompts"][0]["body"], "The day before's.");
    assert!(older["nextBefore"].is_null());
}

#[tokio::test]
async fn only_admins_get_past_the_admin_door() {
    let s = Server::start().await;
    let ada = s.writer("ada").await;
    let routes = [
        ("GET", "/admin/prompts", None),
        ("POST", "/admin/prompts", Some(json!({ "body": "x" }))),
        ("PATCH", "/admin/prompts/p", Some(json!({ "body": "x" }))),
        ("DELETE", "/admin/prompts/p", None),
        ("POST", "/admin/prompts/reorder", Some(json!({ "ids": [] }))),
        ("GET", "/admin/jobs", None),
    ];
    for (method, path, body) in &routes {
        assert_eq!(
            s.status(None, method, path, body.clone()).await,
            403,
            "{method} {path} signed out"
        );
        assert_eq!(
            s.status(Some(&ada), method, path, body.clone()).await,
            403,
            "{method} {path}"
        );
    }
    // No body makes anyone an admin.
    s.ok(
        &ada,
        "PATCH",
        "/me/profile",
        Some(json!({ "isAdmin": true, "displayName": "Ada" })),
    )
    .await;
    s.ok(
        &ada,
        "PATCH",
        "/me/settings",
        Some(json!({ "isAdmin": true })),
    )
    .await;
    assert_eq!(s.ok(&ada, "GET", "/auth/me", None).await["isAdmin"], false);
    assert_eq!(
        s.status(Some(&ada), "GET", "/admin/prompts", None).await,
        403
    );

    // The command line does, and it is on the record.
    super::admin::grant(&mut s.conn(), "ADA@example.com", true).unwrap();
    let page = s.ok(&ada, "GET", "/admin/prompts", None).await;
    assert!(page["warning"]
        .as_str()
        .unwrap()
        .contains("Only 0 prompts are queued"));
    let tomorrow = clock::day_string(super::prompts::today() + Duration::days(1));
    s.ok(
        &ada,
        "POST",
        "/admin/prompts",
        Some(json!({ "body": "Queued one." })),
    )
    .await;
    s.ok(
        &ada,
        "POST",
        "/admin/prompts",
        Some(json!({ "body": "Pinned one.", "day": tomorrow })),
    )
    .await;
    assert_eq!(
        s.status(
            Some(&ada),
            "POST",
            "/admin/prompts",
            Some(json!({ "body": "Again.", "day": tomorrow }))
        )
        .await,
        409
    );
    assert_eq!(
        s.status(
            Some(&ada),
            "POST",
            "/admin/prompts",
            Some(json!({ "body": "Past.", "day": today() }))
        )
        .await,
        400
    );
    // The pin wins over the queue when the day is prepared.
    crate::jobs::ensure_prompt(&s.state, super::prompts::today() + Duration::days(1), false)
        .await
        .unwrap();
    let page = s.ok(&ada, "GET", "/admin/prompts", None).await;
    let day = page["days"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["day"] == tomorrow.as_str())
        .unwrap();
    assert_eq!(day["prompt"]["body"], "Pinned one.");
    assert_eq!(page["queue"][0]["body"], "Queued one.");
    super::admin::grant(&mut s.conn(), "ada@example.com", false).unwrap();
    assert_eq!(
        s.status(Some(&ada), "GET", "/admin/prompts", None).await,
        403
    );
    let actions: Vec<String> = audit_events::table
        .filter(audit_events::actor_id.eq(&ada.id))
        .select(audit_events::action)
        .load(&mut s.conn())
        .unwrap();
    assert!(actions.contains(&"admin_granted".into()) && actions.contains(&"admin_revoked".into()));
}

#[tokio::test]
async fn an_empty_queue_still_opens_the_day() {
    let s = Server::start().await;
    // No queue, and no model configured: the built-in list.
    let today = super::prompts::today();
    crate::jobs::ensure_prompt(&s.state, today, true)
        .await
        .unwrap();
    let (body, source): (String, String) = daily_prompts::table
        .filter(daily_prompts::day.eq(clock::day_string(today)))
        .select((daily_prompts::body, daily_prompts::source))
        .first(&mut s.conn())
        .unwrap();
    assert_eq!(source, "evergreen");
    assert_eq!(body, crate::domain::prompts::evergreen_for(today));
    // Without the fallback (the prepare run), a missing model is a failure to
    // retry, not a made-up day.
    assert!(
        crate::jobs::ensure_prompt(&s.state, today + Duration::days(1), false)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_model_writes_the_prompt_when_nothing_is_queued() {
    use axum::routing::post;
    let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let log = seen.clone();
    let app = axum::Router::new().route(
        "/v1/messages",
        post(move |axum::Json(body): axum::Json<Value>| {
            let log = log.clone();
            async move {
                log.lock().unwrap().push(body);
                axum::Json(json!({
                    "model": "m", "stop_reason": "end_turn",
                    "content": [{ "type": "text", "text": "\"A lighthouse keeper's last night.\"" }],
                    "usage": { "input_tokens": 1, "output_tokens": 1 }
                }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let mut s = Server::start().await;
    let mut config = (*s.state.config).clone();
    config.anthropic_api_key = Some("k".into());
    config.anthropic_url = format!("http://{addr}");
    s.state.config = Arc::new(config);
    let yesterday = super::prompts::today() - Duration::days(1);
    s.prompt(&clock::day_string(yesterday), "An old one.");
    let tomorrow = super::prompts::today() + Duration::days(1);
    crate::jobs::ensure_prompt(&s.state, tomorrow, false)
        .await
        .unwrap();
    let (body, source): (String, String) = daily_prompts::table
        .filter(daily_prompts::day.eq(clock::day_string(tomorrow)))
        .select((daily_prompts::body, daily_prompts::source))
        .first(&mut s.conn())
        .unwrap();
    assert_eq!(
        (body.as_str(), source.as_str()),
        ("A lighthouse keeper's last night.", "ai")
    );
    // It was told the earlier prompts, and nothing a writer wrote.
    let sent = seen.lock().unwrap()[0]["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(sent.contains("<prompt>An old one.</prompt>"));
}

#[tokio::test]
async fn the_email_goes_once_at_each_writers_own_time() {
    let s = Server::start().await;
    let day = clock::parse_day("2026-07-13").unwrap();
    s.prompt("2026-07-13", "A door.");
    let ny = s.writer("ny").await;
    let ldn = s.writer("ldn").await;
    let tyo = s.writer("tyo").await;
    let unverified = s.writer("unv").await;
    diesel::update(users::table.find(&unverified.id))
        .set(users::email_verified_at.eq(None::<String>))
        .execute(&mut s.conn())
        .unwrap();
    for (w, tz, time) in [
        (&ny, "America/New_York", "08:00"),
        (&ldn, "Europe/London", "07:00"),
        (&tyo, "Asia/Tokyo", "17:00"),
        (&unverified, "Europe/London", "09:00"),
    ] {
        s.ok(
            w,
            "POST",
            "/prompts/enrollment",
            Some(json!({ "timezone": tz })),
        )
        .await;
        diesel::update(prompt_enrollments::table.find(&w.id))
            .set((
                prompt_enrollments::email_enabled.eq(true),
                prompt_enrollments::email_local_time.eq(time),
            ))
            .execute(&mut s.conn())
            .unwrap();
    }
    // Turning email on needs a confirmed address.
    assert_eq!(
        s.status(
            Some(&unverified),
            "PATCH",
            "/prompts/enrollment",
            Some(json!({ "emailEnabled": true }))
        )
        .await,
        403
    );

    let at = |t: &str| DateTime::parse_from_rfc3339(t).unwrap().with_timezone(&Utc);
    let sent = |s: &Server| -> Vec<String> {
        prompt_email_sends::table
            .inner_join(users::table)
            .order(users::display_name.asc())
            .select(users::display_name)
            .load(&mut s.conn())
            .unwrap()
    };
    // Just after it opens (07:00Z): Tokyo's 17:00 is 08:00Z, New York's 08:00
    // is 12:00Z, and London's 07:00 is tomorrow's 06:00Z.
    assert_eq!(
        crate::jobs::send_prompt_emails(&s.state, at("2026-07-13T07:30:00Z"))
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        crate::jobs::send_prompt_emails(&s.state, at("2026-07-13T08:00:00Z"))
            .await
            .unwrap(),
        1
    );
    assert_eq!(sent(&s), ["tyo"]);
    // Down for hours: New York's goes on the first tick back, and Tokyo's
    // is not sent again.
    assert_eq!(
        crate::jobs::send_prompt_emails(&s.state, at("2026-07-13T20:00:00Z"))
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        crate::jobs::send_prompt_emails(&s.state, at("2026-07-14T06:00:00Z"))
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        crate::jobs::send_prompt_emails(&s.state, at("2026-07-14T06:59:00Z"))
            .await
            .unwrap(),
        0
    );
    assert_eq!(sent(&s), ["ldn", "ny", "tyo"]);
    assert_eq!(clock::prompt_day(at("2026-07-14T06:59:00Z")), day);

    // Delivered through the outbox: both parts and the unsubscribe headers.
    assert_eq!(
        crate::mail::deliver_due(&s.state.pool, &s.state.mailer)
            .await
            .unwrap(),
        3
    );
    let outbox = std::fs::read_to_string(s.root.join("outbox.jsonl")).unwrap();
    let prompt_mail: Vec<Value> = outbox
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|m| m["subject"].as_str().unwrap().starts_with("Today's prompt"))
        .collect();
    assert_eq!(prompt_mail.len(), 3);
    assert!(prompt_mail.iter().all(|m| m["to"] != "unv@example.com"));
    let first = &prompt_mail[0];
    assert!(first["body"]
        .as_str()
        .unwrap()
        .contains("http://justtype.test/prompts/2026-07-13/write"));
    assert!(first["html"]
        .as_str()
        .unwrap()
        .contains("Write your answer"));
    let unsubscribe = first["headers"]["List-Unsubscribe"].as_str().unwrap();
    assert_eq!(
        first["headers"]["List-Unsubscribe-Post"],
        "List-Unsubscribe=One-Click"
    );
    assert_eq!(
        crate::mail::deliver_due(&s.state.pool, &s.state.mailer)
            .await
            .unwrap(),
        0
    );

    // One click, signed out, turns it off.
    let token = unsubscribe
        .split("token=")
        .nth(1)
        .unwrap()
        .trim_end_matches('>');
    let res = s
        .http
        .post(format!("{}/prompts/unsubscribe?token={token}", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let off: i64 = prompt_enrollments::table
        .filter(prompt_enrollments::email_enabled.eq(false))
        .count()
        .get_result(&mut s.conn())
        .unwrap();
    assert_eq!(off, 1);
    assert_eq!(
        s.http
            .post(format!("{}/prompts/unsubscribe?token=nope", s.base))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
}

#[tokio::test]
async fn an_answer_becomes_a_project_and_stays_as_it_was() {
    let s = Server::start().await;
    let day = today();
    s.prompt(
        &day,
        "Write about a door that someone was told never to open, and why.",
    );
    let ada = s.writer("ada").await;
    s.ok(&ada, "POST", "/prompts/enrollment", None).await;
    let local = clock::day_string(chrono::Utc::now().date_naive());
    let entry = s
        .ok(
            &ada,
            "PUT",
            &format!("/prompts/{day}/entry"),
            Some(json!({
                "content": text_doc("One.\n--\nTwo.\n\nStill two."),
                "localDay": local,
            })),
        )
        .await;
    let entry_id = entry["id"].as_str().unwrap().to_string();
    let words_before: i32 = writing_days::table
        .filter(writing_days::user_id.eq(&ada.id))
        .select(writing_days::net_words)
        .first(&mut s.conn())
        .unwrap();

    let preview = s
        .ok(
            &ada,
            "POST",
            &format!("/prompt-entries/{entry_id}/to-work/preview"),
            None,
        )
        .await;
    assert_eq!(preview["scenes"].as_array().unwrap().len(), 2);
    assert_eq!(preview["blankParagraphSplits"], false);
    // With the setting on, the blank paragraph splits too.
    s.ok(
        &ada,
        "PATCH",
        "/me/settings",
        Some(json!({ "doubleEnterSplitsScenes": true })),
    )
    .await;
    assert_eq!(
        s.ok(&ada, "GET", "/auth/me", None).await["doubleEnterSplitsScenes"],
        true
    );
    let preview = s
        .ok(
            &ada,
            "POST",
            &format!("/prompt-entries/{entry_id}/to-work/preview"),
            None,
        )
        .await;
    let lines: Vec<&str> = preview["scenes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["firstLine"].as_str().unwrap())
        .collect();
    assert_eq!(lines, ["One.", "Two.", "Still two."]);

    let work = s
        .ok(
            &ada,
            "POST",
            &format!("/prompt-entries/{entry_id}/to-work"),
            Some(json!({})),
        )
        .await;
    assert_eq!(
        work["title"],
        "Write about a door that someone was told never to open…"
    );
    let scenes = work["chapters"][0]["scenes"].as_array().unwrap();
    assert_eq!(scenes.len(), 3, "the preview matches what is made");
    let work_id = work["id"].as_str().unwrap();
    let notes: Vec<(String, String)> = notebook_entries::table
        .filter(notebook_entries::work_id.eq(work_id))
        .select((notebook_entries::kind, notebook_entries::name))
        .load(&mut s.conn())
        .unwrap();
    assert_eq!(
        notes,
        [("note".to_string(), format!("Daily prompt — {day}"))]
    );

    // The answer is as it was, says what it became, and no words were counted twice.
    let after = s
        .ok(&ada, "GET", &format!("/prompt-entries/{entry_id}"), None)
        .await;
    assert_eq!(after["contentHash"], entry["contentHash"]);
    assert_eq!(after["convertedWork"]["id"], work_id);
    let words_after: i32 = writing_days::table
        .filter(writing_days::user_id.eq(&ada.id))
        .select(writing_days::net_words)
        .first(&mut s.conn())
        .unwrap();
    assert_eq!(words_before, words_after);
    // Someone else's answer cannot be taken.
    let bo = s.writer("bo").await;
    assert_eq!(
        s.status(
            Some(&bo),
            "POST",
            &format!("/prompt-entries/{entry_id}/to-work"),
            Some(json!({}))
        )
        .await,
        404
    );
}

#[tokio::test]
async fn the_daily_summary_is_built_once_per_writer() {
    let s = Server::start().await;
    let day = today();
    s.prompt(&day, "A summary.");
    let ada = s.writer("ada").await;
    let bo = s.writer("bo").await;
    for w in [&ada, &bo] {
        s.ok(w, "POST", "/prompts/enrollment", None).await;
    }
    let entry_id = s
        .ok(
            &ada,
            "PUT",
            &format!("/prompts/{day}/entry"),
            Some(json!({ "content": text_doc("x y") })),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    s.ok(&ada, "POST", &format!("/prompts/{day}/entry/submit"), None)
        .await;
    for i in 0..3 {
        s.ok(
            &bo,
            "POST",
            &format!("/prompt-entries/{entry_id}/critiques"),
            Some(json!({ "body": format!("c{i}") })),
        )
        .await;
    }
    // The items were made today; sum today up as "yesterday", twice.
    let today = super::prompts::today();
    assert_eq!(
        crate::jobs::build_digests_for(&mut s.conn(), today).unwrap(),
        1
    );
    assert_eq!(
        crate::jobs::build_digests_for(&mut s.conn(), today).unwrap(),
        0
    );
    let page = s.ok(&ada, "GET", "/inbox", None).await;
    assert_eq!(
        page["digest"]["text"],
        "Yesterday: 3 critiques on your answer to “A summary.”. Today's prompt is open."
    );
    assert_eq!(
        s.ok(&ada, "GET", "/inbox/unread-count", None).await["count"],
        4
    );
    s.ok(&ada, "POST", &format!("/inbox/digests/{day}/read"), None)
        .await;
    assert!(s.ok(&ada, "GET", "/inbox", None).await["digest"].is_null());
    assert!(s.ok(&bo, "GET", "/inbox", None).await["digest"].is_null());
}

#[tokio::test]
async fn a_departing_writer_takes_their_answers_and_leaves_their_critiques_blank() {
    let s = Server::start().await;
    let day = today();
    s.prompt(&day, "Leaving.");
    let ada = s.writer("ada").await;
    let bo = s.writer("bo").await;
    for w in [&ada, &bo] {
        s.ok(w, "POST", "/prompts/enrollment", None).await;
    }
    let ada_entry = s
        .ok(
            &ada,
            "PUT",
            &format!("/prompts/{day}/entry"),
            Some(json!({ "content": text_doc("Ada's") })),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    s.ok(&ada, "POST", &format!("/prompts/{day}/entry/submit"), None)
        .await;
    let bo_entry = s
        .ok(
            &bo,
            "PUT",
            &format!("/prompts/{day}/entry"),
            Some(json!({ "content": text_doc("Bo's") })),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    s.ok(&bo, "POST", &format!("/prompts/{day}/entry/submit"), None)
        .await;
    // Bo critiques Ada's; Ada critiques Bo's, and Bo replies.
    s.ok(
        &bo,
        "POST",
        &format!("/prompt-entries/{ada_entry}/critiques"),
        Some(json!({ "body": "Bo on Ada" })),
    )
    .await;
    let a_on_b = s
        .ok(
            &ada,
            "POST",
            &format!("/prompt-entries/{bo_entry}/critiques"),
            Some(json!({ "body": "Ada on Bo" })),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    s.ok(
        &bo,
        "POST",
        &format!("/prompt-entries/{bo_entry}/critiques"),
        Some(json!({ "body": "Bo replies", "parentId": a_on_b })),
    )
    .await;

    let export = s
        .http
        .get(format!("{}/me/export", s.base))
        .bearer_auth(&ada.token)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(export.to_vec())).unwrap();
    let mut prompts = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("prompts.json").unwrap(), &mut prompts).unwrap();
    let prompts: Value = serde_json::from_str(&prompts).unwrap();
    assert_eq!(
        prompts["entries"][0]["content"]["content"][0]["content"][0]["text"],
        "Ada's"
    );
    assert_eq!(prompts["critiques"][0]["body"], "Ada on Bo");
    assert_eq!(prompts["enrollment"]["emailEnabled"], false);

    s.ok(
        &ada,
        "DELETE",
        "/me",
        Some(json!({ "password": "correcthorse" })),
    )
    .await;
    assert_eq!(
        s.status(
            Some(&bo),
            "GET",
            &format!("/prompt-entries/{ada_entry}"),
            None
        )
        .await,
        404
    );
    // Bo's thread keeps its shape: Ada's critique is a nameless placeholder
    // above Bo's reply.
    let thread = s
        .ok(
            &bo,
            "GET",
            &format!("/prompt-entries/{bo_entry}/critiques"),
            None,
        )
        .await;
    assert_eq!(thread.as_array().unwrap().len(), 2);
    assert_eq!(thread[0]["removed"], true);
    assert!(thread[0]["author"].is_null());
    assert_eq!(thread[1]["body"], "Bo replies");
    // Nothing of Ada's is left anywhere.
    let theirs: i64 = prompt_enrollments::table
        .filter(prompt_enrollments::user_id.eq(&ada.id))
        .count()
        .get_result(&mut s.conn())
        .unwrap();
    assert_eq!(theirs, 0);
    assert!(s.inbox(&bo).is_empty(), "{:?}", s.inbox(&bo));
}
