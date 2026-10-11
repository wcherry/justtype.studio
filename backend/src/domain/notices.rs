//! Telling a writer what staff did, and why: an email through the mail queue
//! and an item in the inbox, both written in the transaction of the action, so
//! neither can exist without the other.
//!
//! The words are fixed, and the only free text is the message the moderator
//! wrote for the writer. A notice never names the moderator.
use chrono::{DateTime, Duration, Utc};
use diesel::prelude::*;
use serde_json::json;

use crate::db::models::Restriction;
use crate::db::schema::users;
use crate::db::Conn;
use crate::domain::restrictions as rules;
use crate::error::ApiResult;
use crate::mail::Message;

/// How long the queue keeps trying to deliver a notice.
const GIVE_UP_DAYS: i64 = 3;

/// "14 October 2026".
pub fn human_date(iso: &str) -> String {
    DateTime::parse_from_rfc3339(iso)
        .map(|t| t.with_timezone(&Utc).format("%-d %B %Y").to_string())
        .unwrap_or_else(|_| iso.to_string())
}

/// When a closing restriction's account is deleted.
pub fn purge_date(r: &Restriction) -> Option<String> {
    rules::is_closing(r).then(|| {
        let start = DateTime::parse_from_rfc3339(&r.starts_at)
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        (start + Duration::days(rules::CLOSING_DAYS))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    })
}

fn what(r: &Restriction) -> &'static str {
    match (r.kind.as_str(), r.scope.as_str()) {
        ("warning", _) => "This is a warning. Nothing about your account has changed.",
        ("ban", "site") => "Your account has been banned, so you can't sign in.",
        ("suspend", "site") => {
            "Your account has been suspended. You can sign in and read, but you can't publish or take part in discussions."
        }
        ("read_only", "site") => {
            "Your account is now read-only. You can sign in, read and export your work, but you can't write, edit, publish or post anything."
        }
        (_, "discussion") => "You can't take part in discussions: comments, reader questions and answers, Daily Prompts answers and critiques.",
        _ => "Your account has been restricted.",
    }
}

pub struct Mailed<'a> {
    pub subject: String,
    pub paragraphs: Vec<String>,
    pub link: Option<(&'a str, String)>,
}

fn html(paragraphs: &[String], link: &Option<(&str, String)>) -> String {
    let button = "display:inline-block;padding:10px 16px;background:#7a4f2a;color:#fff;text-decoration:none;border-radius:4px";
    let mut body = String::from(
        "<!doctype html><html><body style=\"font-family:Georgia,serif;color:#222;max-width:560px;margin:0 auto;padding:24px;line-height:1.5\">",
    );
    for p in paragraphs {
        body.push_str(&format!("<p>{}</p>", crate::jobs::escape(p)));
    }
    if let Some((label, url)) = link {
        body.push_str(&format!(
            "<p><a href=\"{}\" style=\"{button}\">{}</a></p>",
            crate::jobs::escape(url),
            crate::jobs::escape(label)
        ));
    }
    body.push_str("</body></html>");
    body
}

/// Queues the email and writes the inbox item for `user_id`.
pub fn tell(
    conn: &mut Conn,
    user_id: &str,
    inbox_kind: &str,
    subject: (&str, &str),
    mailed: Mailed,
) -> ApiResult<()> {
    let (email, name): (String, String) = users::table
        .find(user_id)
        .select((users::email, users::display_name))
        .first(conn)?;
    let mut plain = format!("Hello {name},\n\n");
    for p in &mailed.paragraphs {
        plain.push_str(p);
        plain.push_str("\n\n");
    }
    if let Some((_, url)) = &mailed.link {
        plain.push_str(url);
        plain.push('\n');
    }
    let mut paragraphs = vec![format!("Hello {name},")];
    paragraphs.extend(mailed.paragraphs.iter().cloned());
    crate::mail::enqueue(
        conn,
        &Message {
            to: email,
            subject: mailed.subject.clone(),
            body: plain,
            html: Some(html(&paragraphs, &mailed.link)),
            headers: Vec::new(),
        },
        crate::clock::now() + Duration::days(GIVE_UP_DAYS),
    )?;
    crate::routes::inbox::notify(
        conn,
        user_id,
        inbox_kind,
        subject,
        None,
        None,
        &json!({ "text": mailed.paragraphs.first().cloned().unwrap_or_default() }).to_string(),
    )
}

/// A restriction was set. `appeal_url` is where the writer appeals: the page
/// in the app, or for a banned account a link carrying a single-use token.
pub fn restriction_set(
    conn: &mut Conn,
    public_url: &str,
    r: &Restriction,
    appeal_url: &str,
    export_url: Option<&str>,
) -> ApiResult<()> {
    let mut paragraphs = vec![what(r).to_string()];
    paragraphs.push(format!(
        "Community guidelines, section {}: {}",
        r.reason_code, r.reason_text
    ));
    if r.kind != "warning" {
        paragraphs.push(match &r.ends_at {
            Some(end) => format!("This lasts until {}.", human_date(end)),
            None => "This doesn't have an end date.".to_string(),
        });
    }
    if let Some(purge) = purge_date(r) {
        paragraphs.push(format!(
            "On {}, this account, all of its writing and all of its comments will be deleted.",
            human_date(&purge)
        ));
        if r.kind == "read_only" {
            paragraphs.push(
                "Until then you can export your work from the banner at the top of any page."
                    .to_string(),
            );
        } else if let Some(url) = export_url {
            paragraphs.push(format!(
                "Until then you can have a copy of your work emailed to you: {url}"
            ));
        }
    }
    if r.kind != "warning" {
        paragraphs.push(
            "If you think this is a mistake you can appeal once. A different member of staff will read it.".to_string(),
        );
    }
    let subject = if r.kind == "warning" {
        "A warning about your Just Type Studio account"
    } else {
        "A restriction on your Just Type Studio account"
    };
    let _ = public_url;
    tell(
        conn,
        &r.user_id,
        "moderation.restriction",
        ("restriction", &r.id),
        Mailed {
            subject: subject.into(),
            paragraphs,
            link: (r.kind != "warning").then(|| ("Read more and appeal", appeal_url.to_string())),
        },
    )
}

pub fn restriction_changed(conn: &mut Conn, r: &Restriction, appeal_url: &str) -> ApiResult<()> {
    let mut paragraphs = vec![format!(
        "A restriction on your account has been changed. {}",
        match &r.ends_at {
            Some(end) => format!("It now lasts until {}.", human_date(end)),
            None => "It now has no end date.".to_string(),
        }
    )];
    paragraphs.push(format!("Message from the moderators: {}", r.reason_text));
    tell(
        conn,
        &r.user_id,
        "moderation.restriction",
        ("restriction", &r.id),
        Mailed {
            subject: "A restriction on your Just Type Studio account has changed".into(),
            paragraphs,
            link: Some(("Read more", appeal_url.to_string())),
        },
    )
}

pub fn restriction_lifted(conn: &mut Conn, r: &Restriction, reason: &str) -> ApiResult<()> {
    tell(
        conn,
        &r.user_id,
        "moderation.restriction_ended",
        ("restriction", &r.id),
        Mailed {
            subject: "A restriction on your Just Type Studio account has been lifted".into(),
            paragraphs: vec![
                "The restriction on your account has been lifted. Everything works as it did before.".to_string(),
                format!("Why: {reason}"),
            ],
            link: None,
        },
    )
}

pub fn restriction_ended(conn: &mut Conn, r: &Restriction) -> ApiResult<()> {
    tell(
        conn,
        &r.user_id,
        "moderation.restriction_ended",
        ("restriction", &format!("{}-ended", r.id)),
        Mailed {
            subject: "Your Just Type Studio restriction has ended".into(),
            paragraphs: vec![
                "The restriction on your account has ended. Everything works as it did before."
                    .to_string(),
            ],
            link: None,
        },
    )
}

pub fn appeal_decided(
    conn: &mut Conn,
    user_id: &str,
    restriction_id: &str,
    overturned: bool,
    message: &str,
) -> ApiResult<()> {
    tell(
        conn,
        user_id,
        "moderation.appeal_decided",
        ("restriction", &format!("{restriction_id}-appeal")),
        Mailed {
            subject: "The decision on your appeal".into(),
            paragraphs: vec![
                if overturned {
                    "Your appeal was upheld, and the restriction has been lifted.".to_string()
                } else {
                    "Your appeal was read, and the restriction stands.".to_string()
                },
                format!("Message from the moderators: {message}"),
            ],
            link: None,
        },
    )
}

/// The 7-day and 1-day reminders before a closing restriction deletes
/// an account.
pub fn closing_reminder(
    conn: &mut Conn,
    r: &Restriction,
    purge_at: &str,
    days_left: i64,
) -> ApiResult<()> {
    tell(
        conn,
        &r.user_id,
        "moderation.restriction",
        ("restriction", &format!("{}-reminder-{days_left}", r.id)),
        Mailed {
            subject: format!(
                "Your Just Type Studio account will be deleted in {days_left} day{}",
                if days_left == 1 { "" } else { "s" }
            ),
            paragraphs: vec![
                format!(
                    "On {}, this account, all of its writing and all of its comments will be deleted.",
                    human_date(purge_at)
                ),
                "You can still export your work from the banner at the top of any page.".to_string(),
            ],
            link: None,
        },
    )
}

pub fn book_handed_over(
    conn: &mut Conn,
    new_owner: &str,
    work_id: &str,
    title: &str,
) -> ApiResult<()> {
    tell(
        conn,
        new_owner,
        "moderation.book_handed_over",
        ("work", work_id),
        Mailed {
            subject: format!("“{title}” is now yours"),
            paragraphs: vec![format!(
                "The previous owner of “{title}” has left, and the book is now yours. You are its owner, and everything in it stays as it was."
            )],
            link: None,
        },
    )?;
    // The inbox link needs the work id.
    diesel::update(crate::db::schema::inbox_items::table)
        .filter(crate::db::schema::inbox_items::user_id.eq(new_owner))
        .filter(crate::db::schema::inbox_items::subject_id.eq(work_id))
        .filter(crate::db::schema::inbox_items::kind.eq("moderation.book_handed_over"))
        .set(
            crate::db::schema::inbox_items::detail.eq(Some(
                json!({
                    "text": format!("“{title}” is now yours: its previous owner has left."),
                    "workId": work_id,
                })
                .to_string(),
            )),
        )
        .execute(conn)?;
    Ok(())
}
