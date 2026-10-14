//! What the outline's scene menu does: leave a scene out of the book,
//! duplicate it, and move it into another chapter.
use serde_json::{json, Value};

use super::prompts_api_tests::Server;

/// A chapter of the outline by id: a new book starts with a chapter of its own.
fn chapter_in<'a>(outline: &'a Value, id: &str) -> &'a Value {
    // An outline calls it `id`; a version's content, `chapterId`.
    outline["chapters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id || c["chapterId"] == id)
        .unwrap()
}

fn doc(text: &str) -> Value {
    json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":text}]}]})
}

#[tokio::test]
async fn a_scene_left_out_is_kept_but_not_counted_printed_or_versioned() {
    let server = Server::start().await;
    let eve = server.writer("eve").await;
    let book = server
        .ok(
            &eve,
            "POST",
            "/works",
            Some(json!({ "title": "Gull Rock", "kind": "novel" })),
        )
        .await;
    let work_id = book["id"].as_str().unwrap().to_string();
    let chapter = server
        .ok(
            &eve,
            "POST",
            &format!("/works/{work_id}/chapters"),
            Some(json!({ "title": "One" })),
        )
        .await;
    let chapter_id = chapter["id"].as_str().unwrap().to_string();
    let mut ids = Vec::new();
    for (title, text) in [("Kept", "Two words."), ("Cut", "Three more words.")] {
        let scene = server
            .ok(
                &eve,
                "POST",
                &format!("/chapters/{chapter_id}/scenes"),
                Some(json!({ "title": title, "content": doc(text) })),
            )
            .await;
        ids.push(scene["id"].as_str().unwrap().to_string());
    }
    let words = |outline: &Value| outline["wordCount"].as_i64().unwrap();
    let outline = server
        .ok(&eve, "GET", &format!("/works/{work_id}/outline"), None)
        .await;
    assert_eq!(words(&outline), 5);

    let cut = server
        .ok(
            &eve,
            "PATCH",
            &format!("/scenes/{}", ids[1]),
            Some(json!({ "excluded": true })),
        )
        .await;
    assert_eq!(cut["excluded"], true);
    let outline = server
        .ok(&eve, "GET", &format!("/works/{work_id}/outline"), None)
        .await;
    assert_eq!(words(&outline), 2, "left out of the count");
    assert_eq!(
        chapter_in(&outline, &chapter_id)["scenes"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "still in the outline"
    );

    let (_, text) = server
        .call(
            Some(&eve),
            "GET",
            &format!("/works/{work_id}/export?format=txt"),
            None,
        )
        .await;
    let text = text.as_str().unwrap();
    assert!(
        text.contains("Two words.") && !text.contains("Three more words."),
        "{text}"
    );

    let version = server
        .ok(
            &eve,
            "POST",
            &format!("/works/{work_id}/versions"),
            Some(json!({})),
        )
        .await;
    let content = server
        .ok(
            &eve,
            "GET",
            &format!("/versions/{}/content", version["id"].as_str().unwrap()),
            None,
        )
        .await;
    let scenes: Vec<&str> = chapter_in(&content, &chapter_id)["scenes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["title"].as_str().unwrap())
        .collect();
    assert_eq!(scenes, ["Kept"]);

    // Duplicated, it lands just after the original, left out like it.
    let copy = server
        .ok(&eve, "POST", &format!("/scenes/{}/duplicate", ids[1]), None)
        .await;
    assert_eq!(copy["title"], "Cut (copy)");
    assert_eq!(copy["excluded"], true);
    let copy_id = copy["id"].as_str().unwrap().to_string();
    let read = server
        .ok(&eve, "GET", &format!("/scenes/{copy_id}"), None)
        .await;
    assert_eq!(read["content"], doc("Three more words."));
    server
        .ok(&eve, "POST", &format!("/scenes/{}/duplicate", ids[0]), None)
        .await;
    let outline = server
        .ok(&eve, "GET", &format!("/works/{work_id}/outline"), None)
        .await;
    let order: Vec<&str> = chapter_in(&outline, &chapter_id)["scenes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["title"].as_str().unwrap())
        .collect();
    assert_eq!(order, ["Kept", "Kept (copy)", "Cut", "Cut (copy)"]);
    assert_eq!(
        words(&outline),
        4,
        "the kept copy counts, the cut one does not"
    );

    // Put back, it counts again; and the flag rides along in a project.
    server
        .ok(
            &eve,
            "PATCH",
            &format!("/scenes/{}", ids[1]),
            Some(json!({ "excluded": false })),
        )
        .await;
    let outline = server
        .ok(&eve, "GET", &format!("/works/{work_id}/outline"), None)
        .await;
    assert_eq!(words(&outline), 7);
    let res = server
        .http
        .get(format!(
            "{}/works/{work_id}/export?format=project",
            server.base
        ))
        .bearer_auth(&eve.token)
        .send()
        .await
        .unwrap();
    let (status, copy_book) = server
        .upload(
            &eve,
            "/works/import?name=Gull.zip",
            res.bytes().await.unwrap().to_vec(),
        )
        .await;
    assert_eq!(status, 200, "{copy_book}");
    let flags: Vec<bool> = copy_book["chapters"][1]["scenes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["excluded"].as_bool().unwrap())
        .collect();
    assert_eq!(flags, [false, false, false, true]);
}

#[tokio::test]
async fn a_scene_moves_into_another_chapter_where_it_is_put() {
    let server = Server::start().await;
    let eve = server.writer("eve").await;
    let book = server
        .ok(
            &eve,
            "POST",
            "/works",
            Some(json!({ "title": "Gull Rock", "kind": "novel" })),
        )
        .await;
    let work_id = book["id"].as_str().unwrap().to_string();
    let mut chapters = Vec::new();
    for title in ["One", "Two"] {
        let c = server
            .ok(
                &eve,
                "POST",
                &format!("/works/{work_id}/chapters"),
                Some(json!({ "title": title })),
            )
            .await;
        chapters.push(c["id"].as_str().unwrap().to_string());
    }
    let mut ids = std::collections::HashMap::new();
    for (c, title) in [(0, "a"), (0, "b"), (1, "c"), (1, "d")] {
        let s = server
            .ok(
                &eve,
                "POST",
                &format!("/chapters/{}/scenes", chapters[c]),
                Some(json!({ "title": title })),
            )
            .await;
        ids.insert(title, s["id"].as_str().unwrap().to_string());
    }
    // As the outline does it: into the chapter, then into place.
    server
        .ok(
            &eve,
            "PATCH",
            &format!("/scenes/{}", ids["a"]),
            Some(json!({ "chapterId": chapters[1] })),
        )
        .await;
    server
        .ok(
            &eve,
            "POST",
            &format!("/chapters/{}/scenes/reorder", chapters[1]),
            Some(json!({ "ids": [ids["c"], ids["a"], ids["d"]] })),
        )
        .await;
    let outline = server
        .ok(&eve, "GET", &format!("/works/{work_id}/outline"), None)
        .await;
    let shape: Vec<Vec<&str>> = chapters
        .iter()
        .map(|id| {
            chapter_in(&outline, id)["scenes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["title"].as_str().unwrap())
                .collect()
        })
        .collect();
    assert_eq!(shape, [vec!["b"], vec!["c", "a", "d"]]);
}
