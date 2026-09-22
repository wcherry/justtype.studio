//! Plain text output.
//!
//! Not on the roadmap, but it costs almost nothing and it is the only format a
//! human can diff, grep or paste into a submission form — and the one the tests
//! can assert against without unzipping anything.
use super::{Block, Manuscript, Style};

pub fn write(manuscript: &Manuscript, style: Style) -> String {
    let mut out = String::new();

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

    for chapter in &manuscript.chapters {
        out.push_str(&format!("\n\n{}\n\n", chapter.title));
        for (index, scene) in chapter.scenes.iter().enumerate() {
            if index > 0 {
                out.push_str("\n#\n\n");
            }
            for block in &scene.blocks {
                match block {
                    Block::Paragraph(spans) | Block::Quote(spans) => {
                        for span in spans {
                            out.push_str(&span.text);
                        }
                        out.push_str("\n\n");
                    }
                    Block::Heading { spans, .. } => {
                        for span in spans {
                            out.push_str(&span.text);
                        }
                        out.push_str("\n\n");
                    }
                    Block::SceneBreak => out.push_str("#\n\n"),
                }
            }
        }
    }
    out
}
