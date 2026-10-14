//! Reviews put up from the outline: a snapshot reused while the draft stands
//! still and frozen anew once it moves, brought up to date with the comments
//! carried along, revoked without losing them, and shared with everyone or
//! with people picked by name or by address.
use diesel::prelude::*;
use serde_json::{json, Value};

use super::prompts_api_tests::{Server, Writer};
use crate::db::schema::{mail_queue, versions};

fn doc(text: &str) -> Value {
    json!({ "type": "doc", "content": [
        { "type": "paragraph", "content": [{ "type": "text", "text": text }] }
    ] })
}

async fn write(server: &Server, who: &Writer, scene: &str, text: &str) {
    server
        .ok(
            who,
            "PUT",
            &format!("/scenes/{scene}/content"),
            Some(json!({ "content": doc(text) })),
        )
        .await;
}

fn versions_of(server: &Server, work_id: &str) -> i64 {
    versions::table
        .filter(versions::work_id.eq(work_id))
        .count()
        .get_result(&mut server.conn())
        .unwrap()
}

fn kinds(inbox: &Value) -> Vec<String> {
    inbox["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["kind"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn a_review_follows_the_draft_and_keeps_its_comments() {
    let server = Server::start().await;
    let ann = server.writer("ann").await;
    let bo = server.writer("bo").await;
    let outline = server
        .ok(
            &ann,
            "POST",
            "/works",
            Some(json!({ "title": "Tide", "kind": "novel" })),
        )
        .await;
    let work_id = outline["id"].as_str().unwrap().to_string();
    let scene = outline["chapters"][0]["scenes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    write(&server, &ann, &scene, "The door was open.").await;

    // Marked for review: frozen once, and the same snapshot used again while
    // the scene stands still.
    let reviews = format!("/works/{work_id}/reviews");
    let first = server
        .ok(
            &ann,
            "POST",
            &reviews,
            Some(
                json!({ "scope": "scene", "scopeRefId": scene, "audience": "everyone",
                         "prompt": "Does the opening pull?" }),
            ),
        )
        .await;
    assert_eq!(first["state"], "open");
    assert_eq!(first["visibility"], "platform");
    assert_eq!(first["outdated"], false);
    assert_eq!(versions_of(&server, &work_id), 1);
    let second = server
        .ok(
            &ann,
            "POST",
            &reviews,
            Some(json!({ "scope": "scene", "scopeRefId": scene, "audience": "everyone" })),
        )
        .await;
    assert_eq!(
        second["versionId"], first["versionId"],
        "nothing changed: reused"
    );
    assert_eq!(versions_of(&server, &work_id), 1);
    let review = first["id"].as_str().unwrap().to_string();

    // Anyone signed in reads it and leaves a note.
    let content = server
        .ok(&bo, "GET", &format!("/publications/{review}/content"), None)
        .await;
    let scenes: usize = content["chapters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["scenes"].as_array().unwrap().len())
        .sum();
    assert_eq!(scenes, 1, "{content}");
    server
        .ok(
            &bo,
            "POST",
            &format!("/publications/{review}/comments"),
            Some(json!({ "body": "Lovely start.", "sceneId": scene,
                         "anchor": { "from": 1, "to": 9 }, "anchorQuote": "The door",
                         "category": "praise" })),
        )
        .await;

    // The draft moves on: the outline's badge and the review say so.
    write(&server, &ann, &scene, "The door was open. Rain came in.").await;
    let outline = server
        .ok(&ann, "GET", &format!("/works/{work_id}/outline"), None)
        .await;
    let badge = outline["reviews"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == review.as_str())
        .unwrap()
        .clone();
    assert_eq!(badge["outdated"], true);

    // Brought up to date: a new snapshot, the note carried along and marked
    // with the version it was written on, and its writer told.
    let updated = server
        .ok(&ann, "POST", &format!("/reviews/{review}/update"), None)
        .await;
    assert_ne!(updated["versionId"], first["versionId"]);
    assert_eq!(updated["outdated"], false);
    assert_eq!(versions_of(&server, &work_id), 2);
    let notes = server
        .ok(
            &ann,
            "GET",
            &format!("/publications/{review}/comments"),
            None,
        )
        .await;
    assert_eq!(notes[0]["body"], "Lovely start.");
    assert_eq!(notes[0]["earlierVersion"], true);
    assert_eq!(notes[0]["versionLabel"], first["versionLabel"]);
    let inbox = server.ok(&bo, "GET", "/inbox", None).await;
    assert!(
        kinds(&inbox).contains(&"review.updated".to_string()),
        "{inbox}"
    );
    // Brought up to date again with nothing new: nothing changes.
    let again = server
        .ok(&ann, "POST", &format!("/reviews/{review}/update"), None)
        .await;
    assert_eq!(again["versionId"], updated["versionId"]);

    // Revoked: closed to readers, kept for the writer.
    let revoked = server
        .ok(&ann, "POST", &format!("/reviews/{review}/revoke"), None)
        .await;
    assert_eq!(revoked["state"], "revoked");
    let status = server
        .status(Some(&bo), "GET", &format!("/publications/{review}"), None)
        .await;
    assert_eq!(status, 403);
    let feed = server.ok(&bo, "GET", "/publications", None).await;
    assert!(!feed
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["id"] == review.as_str()));
    let notes = server
        .ok(
            &ann,
            "GET",
            &format!("/publications/{review}/comments"),
            None,
        )
        .await;
    assert_eq!(notes.as_array().unwrap().len(), 1, "the writer keeps them");
    let outline = server
        .ok(&ann, "GET", &format!("/works/{work_id}/outline"), None)
        .await;
    assert!(!outline["reviews"]
        .as_array()
        .unwrap()
        .iter()
        .any(|b| b["id"] == review.as_str()));

    // Opened again, to the same readers.
    let reopened = server
        .ok(&ann, "POST", &format!("/reviews/{review}/reopen"), None)
        .await;
    assert_eq!(reopened["state"], "open");
    server
        .ok(&bo, "GET", &format!("/publications/{review}"), None)
        .await;

    // Nobody else manages it.
    let status = server
        .status(
            Some(&bo),
            "POST",
            &format!("/reviews/{review}/revoke"),
            None,
        )
        .await;
    assert!(status == 403 || status == 404, "{status}");
    let list = server.ok(&ann, "GET", &reviews, None).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn a_part_shared_with_people_picked_by_name_and_by_address() {
    let server = Server::start().await;
    let ann = server.writer("ann").await;
    let bo = server.writer("bo").await;
    let cy = server.writer("cyrille").await;
    let outline = server
        .ok(
            &ann,
            "POST",
            "/works",
            Some(json!({ "title": "Tide", "kind": "novel" })),
        )
        .await;
    let work_id = outline["id"].as_str().unwrap().to_string();
    let opening = outline["chapters"][0]["id"].as_str().unwrap().to_string();
    // Part One, holding a chapter of its own; the book's first chapter
    // stands before it.
    let part = server
        .ok(
            &ann,
            "POST",
            &format!("/works/{work_id}/chapters"),
            Some(json!({ "title": "Part One" })),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    server
        .ok(
            &ann,
            "PATCH",
            &format!("/chapters/{part}"),
            Some(json!({ "role": "part" })),
        )
        .await;
    let held = server
        .ok(
            &ann,
            "POST",
            &format!("/works/{work_id}/chapters"),
            Some(json!({ "title": "The Crossing" })),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    server
        .ok(
            &ann,
            "POST",
            &format!("/chapters/{held}/scenes"),
            Some(json!({ "title": "Out" })),
        )
        .await;

    // Found by name to pick; names only.
    let found = server.ok(&ann, "GET", "/people?q=cyr", None).await;
    assert_eq!(found[0]["displayName"], "cyrille");
    assert!(found[0].get("email").is_none());

    let review = server
        .ok(
            &ann,
            "POST",
            &format!("/works/{work_id}/reviews"),
            Some(
                json!({ "scope": "part", "scopeRefId": part, "audience": "selected",
                         "userIds": [cy.id], "emails": ["Dee@Example.com"] }),
            ),
        )
        .await;
    assert_eq!(review["title"], "“Part One”");
    assert_eq!(review["visibility"], "invited");
    let people = review["people"].as_array().unwrap();
    assert_eq!(people.len(), 2);
    let picked = people.iter().find(|p| p.get("user").is_some()).unwrap();
    assert_eq!(picked["user"]["displayName"], "cyrille");
    assert!(
        picked.get("email").is_none(),
        "an account's address is not shown"
    );
    assert!(people.iter().any(|p| p["email"] == "dee@example.com"));
    let id = review["id"].as_str().unwrap().to_string();

    // Told: an inbox item for the account, a mail for the address.
    let inbox = server.ok(&cy, "GET", "/inbox", None).await;
    assert!(
        kinds(&inbox).contains(&"review.invited".to_string()),
        "{inbox}"
    );
    let mailed: Vec<String> = mail_queue::table
        .select(mail_queue::to_address)
        .load(&mut server.conn())
        .unwrap();
    assert!(
        mailed.contains(&"dee@example.com".to_string()),
        "{mailed:?}"
    );

    // The part and the chapter it holds, not the chapter before it.
    let content = server
        .ok(&cy, "GET", &format!("/publications/{id}/content"), None)
        .await;
    let chapters: Vec<&str> = content["chapters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["chapterId"].as_str().unwrap())
        .collect();
    assert!(chapters.contains(&held.as_str()), "{content}");
    assert!(!chapters.contains(&opening.as_str()), "{content}");

    // Someone not picked is turned away.
    let status = server
        .status(Some(&bo), "GET", &format!("/publications/{id}"), None)
        .await;
    assert_eq!(status, 403);

    // Bo added, Dee taken off: Bo is told, the list is what was sent.
    let review = server
        .ok(
            &ann,
            "PATCH",
            &format!("/reviews/{id}"),
            Some(json!({ "userIds": [cy.id, bo.id], "emails": [] })),
        )
        .await;
    assert_eq!(review["people"].as_array().unwrap().len(), 2);
    assert!(!review["people"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["email"] == "dee@example.com"));
    let inbox = server.ok(&bo, "GET", "/inbox", None).await;
    assert!(kinds(&inbox).contains(&"review.invited".to_string()));
    server
        .ok(&bo, "GET", &format!("/publications/{id}"), None)
        .await;

    // Nobody picked is refused; and a book that is not yours is not yours to
    // put up for review.
    let status = server
        .status(
            Some(&ann),
            "PATCH",
            &format!("/reviews/{id}"),
            Some(json!({ "userIds": [], "emails": [] })),
        )
        .await;
    assert_eq!(status, 400);
    let status = server
        .status(
            Some(&bo),
            "POST",
            &format!("/works/{work_id}/reviews"),
            Some(json!({ "scope": "work", "audience": "everyone" })),
        )
        .await;
    assert!(status == 403 || status == 404, "{status}");
    // A chapter is not a part.
    let status = server
        .status(
            Some(&ann),
            "POST",
            &format!("/works/{work_id}/reviews"),
            Some(json!({ "scope": "part", "scopeRefId": held, "audience": "everyone" })),
        )
        .await;
    assert_eq!(status, 400);
}
