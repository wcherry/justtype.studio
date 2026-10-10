//! Where each writer was in a book, kept on the server so it follows them to
//! another device — and kept apart, so nobody's place shows anyone else's.
use serde_json::{json, Value};

use super::prompts_api_tests::Server;

fn first_scene(outline: &Value) -> String {
    outline["chapters"][0]["scenes"][0]["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn a_place_is_kept_per_writer_and_only_in_their_own_books() {
    let server = Server::start().await;
    let ann = server.writer("ann").await;
    let bo = server.writer("bo").await;

    let outline = server
        .ok(&ann, "POST", "/works", Some(json!({ "title": "Tide", "kind": "novel" })))
        .await;
    let work = outline["id"].as_str().unwrap().to_string();
    let scene = first_scene(&outline);
    assert!(outline.get("place").is_none(), "no place before one is kept");

    let place = json!({ "sceneId": scene, "from": 3, "to": 7 });
    server.ok(&ann, "PUT", &format!("/works/{work}/place"), Some(place.clone())).await;
    let outline = server.ok(&ann, "GET", &format!("/works/{work}/outline"), None).await;
    assert_eq!(outline["place"], place);

    // Someone else's book: neither its outline nor a place in it.
    assert_eq!(server.status(Some(&bo), "GET", &format!("/works/{work}/outline"), None).await, 403);
    assert_eq!(
        server.status(Some(&bo), "PUT", &format!("/works/{work}/place"), Some(place.clone())).await,
        403
    );

    // A collaborator has a place of their own, and does not see Ann's.
    server
        .ok(
            &ann,
            "POST",
            &format!("/works/{work}/collaborators"),
            Some(json!({ "email": "bo@example.com", "role": "reader" })),
        )
        .await;
    let theirs = server.ok(&bo, "GET", &format!("/works/{work}/outline"), None).await;
    assert!(theirs.get("place").is_none(), "{theirs}");
    server
        .ok(&bo, "PUT", &format!("/works/{work}/place"), Some(json!({ "sceneId": scene, "from": 1, "to": 1 })))
        .await;
    let outline = server.ok(&ann, "GET", &format!("/works/{work}/outline"), None).await;
    assert_eq!(outline["place"], place, "Bo's place left Ann's alone");

    // A scene from another book, or a backwards selection, is refused.
    let other = server
        .ok(&ann, "POST", "/works", Some(json!({ "title": "Ebb", "kind": "novel" })))
        .await;
    let elsewhere = json!({ "sceneId": first_scene(&other), "from": 0, "to": 0 });
    assert_eq!(server.status(Some(&ann), "PUT", &format!("/works/{work}/place"), Some(elsewhere)).await, 404);
    let backwards = json!({ "sceneId": scene, "from": 5, "to": 2 });
    assert_eq!(server.status(Some(&ann), "PUT", &format!("/works/{work}/place"), Some(backwards)).await, 400);

    // Deleting the scene forgets the place, rather than pointing at nothing.
    server.ok(&ann, "DELETE", &format!("/scenes/{scene}"), None).await;
    let outline = server.ok(&ann, "GET", &format!("/works/{work}/outline"), None).await;
    assert!(outline.get("place").is_none(), "{outline}");
}
