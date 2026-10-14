//! The notebook in the writer's order: new entries go to the end of their
//! kind, a drag's new order is kept, and a project carries it to a new book.
use serde_json::{json, Value};

use super::prompts_api_tests::Server;

fn names(entries: &Value, kind: &str) -> Vec<String> {
    entries
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == kind)
        .map(|e| e["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn the_notebook_keeps_the_writers_order() {
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
    let notebook = format!("/works/{work_id}/notebook");

    let mut ids = std::collections::HashMap::new();
    for (kind, name) in [
        ("character", "Zed"),
        ("character", "Agnes"),
        ("place", "The Lamp Room"),
        ("character", "Mara"),
    ] {
        let entry = server
            .ok(
                &eve,
                "POST",
                &notebook,
                Some(json!({ "kind": kind, "name": name })),
            )
            .await;
        ids.insert(name, entry["id"].as_str().unwrap().to_string());
    }
    let list = server.ok(&eve, "GET", &notebook, None).await;
    assert_eq!(
        names(&list, "character"),
        ["Zed", "Agnes", "Mara"],
        "added at the end"
    );

    let reordered = server
        .ok(
            &eve,
            "POST",
            &format!("{notebook}/reorder"),
            Some(json!({ "kind": "character", "ids": [ids["Mara"], ids["Zed"], ids["Agnes"]] })),
        )
        .await;
    assert_eq!(names(&reordered, "character"), ["Mara", "Zed", "Agnes"]);
    let list = server.ok(&eve, "GET", &notebook, None).await;
    assert_eq!(names(&list, "character"), ["Mara", "Zed", "Agnes"], "kept");
    assert_eq!(
        names(&list, "place"),
        ["The Lamp Room"],
        "other kinds untouched"
    );

    // Only some listed: they go first, the rest keep their order after them.
    server
        .ok(
            &eve,
            "POST",
            &format!("{notebook}/reorder"),
            Some(json!({ "kind": "character", "ids": [ids["Agnes"]] })),
        )
        .await;
    let list = server.ok(&eve, "GET", &notebook, None).await;
    assert_eq!(names(&list, "character"), ["Agnes", "Mara", "Zed"]);

    // A new one still goes to the end.
    server
        .ok(
            &eve,
            "POST",
            &notebook,
            Some(json!({ "kind": "character", "name": "Bea" })),
        )
        .await;
    let list = server.ok(&eve, "GET", &notebook, None).await;
    assert_eq!(names(&list, "character"), ["Agnes", "Mara", "Zed", "Bea"]);

    // An entry of another kind, or another book, is refused.
    for (kind, id) in [
        ("character", ids["The Lamp Room"].clone()),
        ("character", "nope".into()),
    ] {
        let status = server
            .status(
                Some(&eve),
                "POST",
                &format!("{notebook}/reorder"),
                Some(json!({ "kind": kind, "ids": [id] })),
            )
            .await;
        assert_eq!(status, 400);
    }
    // Someone else's book is not theirs to arrange.
    let mallory = server.writer("mallory").await;
    let status = server
        .status(
            Some(&mallory),
            "POST",
            &format!("{notebook}/reorder"),
            Some(json!({ "kind": "character", "ids": [] })),
        )
        .await;
    assert!(status == 403 || status == 404, "{status}");

    // A project carries the order to the book it makes.
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
    assert_eq!(res.status().as_u16(), 200);
    let zip = res.bytes().await.unwrap().to_vec();
    let (status, copy) = server
        .upload(&eve, "/works/import?name=Gull%20Rock.zip", zip)
        .await;
    assert_eq!(status, 200, "{copy}");
    let copied = server
        .ok(
            &eve,
            "GET",
            &format!("/works/{}/notebook", copy["id"].as_str().unwrap()),
            None,
        )
        .await;
    assert_eq!(names(&copied, "character"), ["Agnes", "Mara", "Zed", "Bea"]);
}
