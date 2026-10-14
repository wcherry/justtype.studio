//! A book's own scene statuses: listed before the built-in ones, each with its
//! colour, held by scenes, refused from other books, and carried by a project.
use serde_json::{json, Value};

use super::prompts_api_tests::Server;

fn labels(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|s| s["label"].as_str().unwrap().to_string())
        .collect()
}

fn key_of(list: &Value, label: &str) -> String {
    list.as_array()
        .unwrap()
        .iter()
        .find(|s| s["label"] == label)
        .unwrap()["key"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn a_book_has_statuses_of_its_own_before_the_built_in_ones() {
    let server = Server::start().await;
    let eve = server.writer("eve").await;
    let outline = server
        .ok(
            &eve,
            "POST",
            "/works",
            Some(json!({ "title": "Gull Rock", "kind": "novel" })),
        )
        .await;
    let work_id = outline["id"].as_str().unwrap().to_string();
    let scene_id = outline["chapters"][0]["scenes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        labels(&outline["statuses"]),
        ["Outline", "Drafting", "Revised", "Done"]
    );
    assert!(outline["statuses"][0]["color"]
        .as_str()
        .unwrap()
        .starts_with('#'));
    let statuses = format!("/works/{work_id}/statuses");

    server
        .ok(
            &eve,
            "POST",
            &statuses,
            Some(json!({ "label": "Needs research", "color": "#C2453D" })),
        )
        .await;
    let list = server
        .ok(&eve, "POST", &statuses, Some(json!({ "label": "Cut?" })))
        .await;
    assert_eq!(
        labels(&list),
        [
            "Needs research",
            "Cut?",
            "Outline",
            "Drafting",
            "Revised",
            "Done"
        ]
    );
    assert_eq!(list[0]["color"], "#c2453d", "kept, lowercased");
    assert_eq!(list[0]["custom"], true);
    assert_eq!(list[2]["custom"], false);
    assert!(
        list[1]["color"].as_str().unwrap().starts_with('#'),
        "given one"
    );
    let research = key_of(&list, "Needs research");
    let cut = key_of(&list, "Cut?");

    // Names are unique, built-in ones included; colours are `#rrggbb`.
    for body in [
        json!({ "label": "drafting" }),
        json!({ "label": "needs RESEARCH" }),
    ] {
        let status = server
            .status(Some(&eve), "POST", &statuses, Some(body))
            .await;
        assert_eq!(status, 409);
    }
    for body in [
        json!({ "label": "" }),
        json!({ "label": "Red", "color": "red" }),
    ] {
        let status = server
            .status(Some(&eve), "POST", &statuses, Some(body))
            .await;
        assert_eq!(status, 400);
    }

    // A scene can have one.
    let scene = server
        .ok(
            &eve,
            "PATCH",
            &format!("/scenes/{scene_id}"),
            Some(json!({ "status": research })),
        )
        .await;
    assert_eq!(scene["status"], research.as_str());

    // Renamed and recoloured, the scene still has it.
    let list = server
        .ok(
            &eve,
            "PATCH",
            &format!("/statuses/{research}"),
            Some(json!({ "label": "Research", "color": "#2f9aa8" })),
        )
        .await;
    assert_eq!(list[0]["label"], "Research");
    assert_eq!(list[0]["color"], "#2f9aa8");

    // Reordered.
    let list = server
        .ok(
            &eve,
            "POST",
            &format!("{statuses}/reorder"),
            Some(json!({ "ids": [cut] })),
        )
        .await;
    assert_eq!(labels(&list)[..2], ["Cut?", "Research"]);

    // A project carries them, and the scene's, to the book it makes.
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
    assert_eq!(labels(&copy["statuses"])[..2], ["Cut?", "Research"]);
    let copied_key = key_of(&copy["statuses"], "Research");
    assert_ne!(copied_key, research, "the copy's own");
    assert_eq!(
        copy["chapters"][0]["scenes"][0]["status"],
        copied_key.as_str()
    );

    // Another book's status is not this one's.
    let other = server
        .ok(
            &eve,
            "POST",
            "/works",
            Some(json!({ "title": "Elsewhere", "kind": "novel" })),
        )
        .await;
    let other_scene = other["chapters"][0]["scenes"][0]["id"].as_str().unwrap();
    let status = server
        .status(
            Some(&eve),
            "PATCH",
            &format!("/scenes/{other_scene}"),
            Some(json!({ "status": research, "title": "Changed" })),
        )
        .await;
    assert_eq!(status, 400);
    let other = server
        .ok(
            &eve,
            "GET",
            &format!("/works/{}/outline", other["id"].as_str().unwrap()),
            None,
        )
        .await;
    assert_ne!(
        other["chapters"][0]["scenes"][0]["title"], "Changed",
        "a refused status changes nothing"
    );

    // Nobody else changes them.
    let mallory = server.writer("mallory").await;
    let status = server
        .status(
            Some(&mallory),
            "PATCH",
            &format!("/statuses/{research}"),
            Some(json!({ "color": "#000000" })),
        )
        .await;
    assert!(status == 403 || status == 404, "{status}");

    // Removed, its scenes go back to drafting.
    let list = server
        .ok(&eve, "DELETE", &format!("/statuses/{research}"), None)
        .await;
    assert!(!labels(&list).contains(&"Research".to_string()));
    let outline = server
        .ok(&eve, "GET", &format!("/works/{work_id}/outline"), None)
        .await;
    assert_eq!(outline["chapters"][0]["scenes"][0]["status"], "drafting");
}
