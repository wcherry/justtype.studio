//! A Scrivener project, zipped, imported through the API: it becomes a book of
//! the writer's own, with its chapters, scenes and notebook.
use std::io::Write;

use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use super::prompts_api_tests::Server;

const BINDER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ScrivenerProject Version="2.0">
  <Binder>
    <BinderItem UUID="D" Type="DraftFolder"><Title>Manuscript</Title><Children>
      <BinderItem UUID="C1" Type="Folder"><Title>Arrival</Title><Children>
        <BinderItem UUID="S1" Type="Text"><Title>The Door</Title></BinderItem>
        <BinderItem UUID="S2" Type="Text"><Title>The Hall</Title></BinderItem>
      </Children></BinderItem>
    </Children></BinderItem>
    <BinderItem UUID="CH" Type="Folder"><Title>Characters</Title><Children>
      <BinderItem UUID="M" Type="Text"><Title>Mara</Title></BinderItem>
    </Children></BinderItem>
  </Binder>
</ScrivenerProject>"#;

fn scriv_zip() -> Vec<u8> {
    let rtf = |t: &str| format!("{{\\rtf1\\ansi {t}}}");
    let mut zip = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in [
        ("Home.scriv/Home.scrivx", BINDER.to_string()),
        (
            "Home.scriv/Files/Data/S1/content.rtf",
            rtf("She opened the door."),
        ),
        (
            "Home.scriv/Files/Data/S2/content.rtf",
            rtf("The hall was cold and dark."),
        ),
        (
            "Home.scriv/Files/Data/M/content.rtf",
            rtf("Thirty-four, a sailor."),
        ),
    ] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[tokio::test]
async fn a_scrivener_project_becomes_a_book() {
    let server = Server::start().await;
    let eve = server.writer("eve").await;

    let (status, book) = server
        .upload(&eve, "/works/import?name=Home.scriv.zip", scriv_zip())
        .await;
    assert_eq!(status, 200, "{book}");
    assert_eq!(book["title"], "Home");
    assert_eq!(book["wordCount"], 10);
    let chapter = &book["chapters"][0];
    assert_eq!(chapter["title"], "Arrival");
    assert_eq!(chapter["scenes"][0]["title"], "The Door");
    assert_eq!(chapter["scenes"][1]["title"], "The Hall");

    let scene_id = chapter["scenes"][1]["id"].as_str().unwrap();
    let scene = server
        .ok(&eve, "GET", &format!("/scenes/{scene_id}"), None)
        .await;
    assert_eq!(
        scene["content"]["content"][0]["content"][0]["text"],
        "The hall was cold and dark."
    );

    let work_id = book["id"].as_str().unwrap();
    let notebook = server
        .ok(&eve, "GET", &format!("/works/{work_id}/notebook"), None)
        .await;
    assert_eq!(notebook[0]["kind"], "character");
    assert_eq!(notebook[0]["name"], "Mara");

    // Inside a book, where only a document makes sense, it is turned away.
    let (status, _) = server
        .upload(
            &eve,
            &format!("/works/{work_id}/import?name=Home.scriv.zip"),
            scriv_zip(),
        )
        .await;
    assert_eq!(status, 400);
}
