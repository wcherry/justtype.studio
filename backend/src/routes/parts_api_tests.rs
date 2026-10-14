//! Parts through the API: a Scrivener part arrives as one, a chapter can be
//! made one, and either way it keeps its part page, is named on its own count,
//! and goes into a version and an export.
use std::io::Write;

use serde_json::{json, Value};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use super::prompts_api_tests::Server;

/// Part "Arrival" → an untitled chapter → one scene.
fn scriv_zip() -> Vec<u8> {
    let binder = r#"<ScrivenerProject Version="2.0"><Binder>
      <BinderItem UUID="D" Type="DraftFolder"><Title>Manuscript</Title><Children>
        <BinderItem UUID="P" Type="Folder"><Title>Arrival</Title><Children>
          <BinderItem UUID="C" Type="Folder"><Title></Title><Children>
            <BinderItem UUID="S" Type="Text"><Title>Door</Title></BinderItem>
          </Children></BinderItem>
        </Children></BinderItem>
      </Children></BinderItem>
    </Binder></ScrivenerProject>"#;
    let mut zip = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in [
        ("Home.scriv/Home.scrivx", binder.to_string()),
        (
            "Home.scriv/Files/Data/S/content.rtf",
            r"{\rtf1\ansi She opened the door.}".to_string(),
        ),
    ] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn titles(chapters: &Value) -> Vec<(String, String, usize)> {
    chapters
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["role"].as_str().unwrap().to_string(),
                c["title"].as_str().unwrap().to_string(),
                c["scenes"].as_array().map_or(0, Vec::len),
            )
        })
        .collect()
}

#[tokio::test]
async fn parts_are_kept_named_and_exported() {
    let server = Server::start().await;
    let eve = server.writer("eve").await;

    let (status, book) = server
        .upload(&eve, "/works/import?name=Home.scriv.zip", scriv_zip())
        .await;
    assert_eq!(status, 200, "{book}");
    let work_id = book["id"].as_str().unwrap().to_string();
    assert_eq!(
        titles(&book["chapters"]),
        [
            ("part".into(), "Arrival".into(), 1),
            ("chapter".into(), "".into(), 1),
        ],
        "a part arrives with its (empty) part page"
    );

    // A new, empty chapter made a part is given its part page.
    let divider = server
        .ok(
            &eve,
            "POST",
            &format!("/works/{work_id}/chapters"),
            Some(json!({})),
        )
        .await;
    let divider_id = divider["id"].as_str().unwrap();
    let made = server
        .ok(
            &eve,
            "PATCH",
            &format!("/chapters/{divider_id}"),
            Some(json!({ "role": "part" })),
        )
        .await;
    assert_eq!(made["role"], "part");
    // Making it a part again does not add a second page.
    server
        .ok(
            &eve,
            "PATCH",
            &format!("/chapters/{divider_id}"),
            Some(json!({ "role": "part" })),
        )
        .await;

    let last = server
        .ok(
            &eve,
            "POST",
            &format!("/works/{work_id}/chapters"),
            Some(json!({})),
        )
        .await;
    server
        .ok(
            &eve,
            "POST",
            &format!("/chapters/{}/scenes", last["id"].as_str().unwrap()),
            Some(json!({
                "title": "",
                "content": {"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"The end came quietly."}]}]},
            })),
        )
        .await;

    let outline = server
        .ok(&eve, "GET", &format!("/works/{work_id}/outline"), None)
        .await;
    assert_eq!(
        titles(&outline["chapters"]),
        [
            ("part".into(), "Arrival".into(), 1),
            ("chapter".into(), "".into(), 1),
            ("part".into(), "".into(), 1),
            ("chapter".into(), "".into(), 1),
        ]
    );

    // A version keeps both parts, and names the untitled ones by their own
    // count: the second chapter is Chapter 2, not Chapter 3.
    let version = server
        .ok(
            &eve,
            "POST",
            &format!("/works/{work_id}/versions"),
            Some(json!({})),
        )
        .await;
    let version_id = version["id"].as_str().unwrap();
    let content = server
        .ok(
            &eve,
            "GET",
            &format!("/versions/{version_id}/content"),
            None,
        )
        .await;
    assert_eq!(
        titles(&content["chapters"]),
        [
            ("part".into(), "Arrival".into(), 1),
            ("chapter".into(), "Chapter 1".into(), 1),
            ("part".into(), "Part 2".into(), 1),
            ("chapter".into(), "Chapter 2".into(), 1),
        ]
    );

    // And an export prints the parts' pages, empty as they are.
    let (status, text) = server
        .call(
            Some(&eve),
            "GET",
            &format!("/versions/{version_id}/export?format=txt"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    let text = text.as_str().unwrap();
    let order: Vec<usize> = [
        "Arrival",
        "Chapter 1",
        "She opened",
        "Part 2",
        "Chapter 2",
        "The end",
    ]
    .iter()
    .map(|t| {
        text.find(t)
            .unwrap_or_else(|| panic!("{t} missing from:\n{text}"))
    })
    .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{text}");

    // Any other role is still refused.
    let refused = server
        .status(
            Some(&eve),
            "PATCH",
            &format!("/chapters/{divider_id}"),
            Some(json!({ "role": "interlude" })),
        )
        .await;
    assert_eq!(refused, 400);
}
