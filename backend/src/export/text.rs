//! Plain text output.
//!
//! Not on the roadmap, but it costs almost nothing and it is the only format a
//! human can diff, grep or paste into a submission form — and the one the tests
//! can assert against without unzipping anything.
use super::options::ExportOptions;
use super::{Block, Manuscript, NoteLayout, Span, Style};

fn plain(spans: &[Span], out: &mut String) {
    for span in spans {
        match span.note {
            Some(n) => out.push_str(&format!("[{n}]")),
            None => out.push_str(&span.text),
        }
    }
}

pub fn write(manuscript: &Manuscript, options: &ExportOptions) -> String {
    let style = options.style_kind();
    let mut out = String::new();

    // The only typography plain text has. Standard manuscript format keeps `#`.
    let scene_break = match (style, &options.style.scene_break) {
        (Style::Book, Some(glyph)) => glyph.clone(),
        _ => "#".to_string(),
    };
    let break_block = if scene_break.is_empty() {
        "\n".to_string()
    } else {
        format!("\n{scene_break}\n\n")
    };

    if !manuscript.omit_front_matter {
        if style == Style::Manuscript {
            let contact = &manuscript.contact;
            for line in [
                contact
                    .legal_name
                    .clone()
                    .or_else(|| Some(manuscript.byline.clone())),
                contact.address.clone(),
                contact.phone.clone(),
                contact.email.clone(),
            ]
            .into_iter()
            .flatten()
            {
                out.push_str(&line);
                out.push('\n');
            }
            out.push_str(&format!(
                "\nabout {} words\n",
                manuscript.rounded_word_count()
            ));
        }

        out.push_str(&format!("\n\n{}\n", manuscript.title));
        if let Some(subtitle) = &manuscript.subtitle {
            out.push_str(&format!("{subtitle}\n"));
        }
        out.push_str(&format!("by {}\n", manuscript.byline));
    }

    if manuscript.toc {
        out.push_str("\n\nContents\n\n");
        for chapter in &manuscript.chapters {
            out.push_str(&format!("  {}\n", chapter.title));
        }
    }

    for chapter in &manuscript.chapters {
        out.push_str(&format!("\n\n{}\n\n", chapter.title));
        for (index, scene) in chapter.scenes.iter().enumerate() {
            if index > 0 {
                out.push_str(&break_block);
            }
            for block in &scene.blocks {
                match block {
                    Block::Paragraph(spans)
                    | Block::Quote(spans)
                    | Block::Heading { spans, .. } => {
                        plain(spans, &mut out);
                        out.push_str("\n\n");
                    }
                    Block::SceneBreak => {
                        if scene_break.is_empty() {
                            out.push('\n');
                        } else {
                            out.push_str(&format!("{scene_break}\n\n"));
                        }
                    }
                }
            }
        }
        if manuscript.note_layout != NoteLayout::EndnotesBook && !chapter.notes.is_empty() {
            out.push_str("\nNotes\n\n");
            for note in &chapter.notes {
                out.push_str(&format!(
                    "[{}] {}\n",
                    note.number,
                    note.plain(manuscript.note_label)
                ));
            }
        }
    }

    if manuscript.note_layout == NoteLayout::EndnotesBook {
        let notes = manuscript.all_notes();
        if !notes.is_empty() {
            out.push_str("\n\nNotes\n\n");
            for note in notes {
                out.push_str(&format!(
                    "[{}] {}\n",
                    note.number,
                    note.plain(manuscript.note_label)
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::options::Format;
    use crate::export::{ChapterContent, Note, SceneContent};

    fn sample() -> Manuscript {
        let para = |t: &str| Block::Paragraph(vec![Span { text: t.into(), ..Default::default() }]);
        Manuscript {
            title: "T".into(),
            byline: "A B".into(),
            chapters: vec![
                ChapterContent {
                    title: "One".into(),
                    scenes: vec![SceneContent {
                        blocks: vec![Block::Paragraph(vec![
                            Span { text: "Hello".into(), ..Default::default() },
                            Span { note: Some(1), ..Default::default() },
                            Span { text: " world.".into(), ..Default::default() },
                        ])],
                        ..Default::default()
                    }],
                    notes: vec![Note { number: 1, text: "fix this".into(), quote: Some("Hello".into()) }],
                    ..Default::default()
                },
                ChapterContent {
                    title: "Two".into(),
                    scenes: vec![SceneContent { blocks: vec![para("x")], ..Default::default() }],
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn notes_are_marked_and_listed_per_chapter() {
        let mut m = sample();
        m.note_layout = NoteLayout::EndnotesChapter;
        let text = write(&m, &ExportOptions::shorthand(Format::Txt, Style::Book));
        assert!(text.contains("Hello[1] world."));
        assert!(text.contains("[1] “Hello” — fix this"));
        assert!(text.find("Notes").unwrap() < text.find("Two").unwrap());
    }

    #[test]
    fn book_notes_gather_at_the_end() {
        let mut m = sample();
        m.note_layout = NoteLayout::EndnotesBook;
        m.note_label = true;
        let text = write(&m, &ExportOptions::default());
        assert!(text.find("Two").unwrap() < text.find("Notes").unwrap());
        assert!(text.contains("Comment: “Hello” — fix this"));
    }

    #[test]
    fn front_matter_and_toc_toggles() {
        let mut m = sample();
        m.omit_front_matter = true;
        m.toc = true;
        let text = write(&m, &ExportOptions::default());
        assert!(!text.contains("by A B"));
        assert!(text.contains("Contents"));
    }

    #[test]
    fn the_scene_break_glyph_follows_the_book_style_only() {
        let mut m = sample();
        m.chapters[0].scenes.push(SceneContent {
            blocks: vec![Block::Paragraph(vec![Span { text: "more".into(), ..Default::default() }])],
            ..Default::default()
        });
        let mut options = ExportOptions::shorthand(Format::Txt, Style::Book);
        options.style.scene_break = Some("* * *".into());
        assert!(write(&m, &options).contains("\n* * *\n"));
        options.style.preset = crate::export::options::StylePreset::Manuscript;
        assert!(write(&m, &options).contains("\n#\n"));
    }
}
