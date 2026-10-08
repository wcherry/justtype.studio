//! One self-contained HTML page: the whole book, styles inline, nothing to
//! fetch. For reading in a browser, pasting into a blog, or printing.
use super::epub::spans_to_html;
use super::{xml_escape, Block, Manuscript};

const STYLE: &str = "body{max-width:36em;margin:3em auto;padding:0 1em;font:1.1em/1.6 Georgia,serif;color:#222}\
h1,h2{font-weight:normal}.title-page{text-align:center;margin:6em 0}.title-page h1{font-size:2.4em}\
.byline,.subtitle{font-style:italic}.dedication,.epigraph{margin:2em auto;max-width:24em}\
section.chapter{margin-top:4em}p{margin:0;text-indent:1.5em}p.first{text-indent:0}\
hr.scene{border:0;text-align:center;margin:1.5em 0}hr.scene::after{content:\"* * *\"}\
blockquote{margin:1em 2em;font-style:italic}";

pub fn write(manuscript: &Manuscript) -> String {
    let mut body = format!(
        "<header class=\"title-page\">\n<h1>{}</h1>\n",
        xml_escape(&manuscript.title)
    );
    if let Some(subtitle) = &manuscript.subtitle {
        body.push_str(&format!("<p class=\"subtitle\">{}</p>\n", xml_escape(subtitle)));
    }
    body.push_str(&format!("<p class=\"byline\">by {}</p>\n", xml_escape(&manuscript.byline)));
    if let Some(dedication) = &manuscript.dedication {
        body.push_str(&format!("<p class=\"dedication\">{}</p>\n", xml_escape(dedication)));
    }
    if let Some(epigraph) = &manuscript.epigraph {
        body.push_str(&format!("<blockquote class=\"epigraph\"><p>{}</p>", xml_escape(epigraph)));
        if let Some(source) = &manuscript.epigraph_source {
            body.push_str(&format!("<p class=\"source\">— {}</p>", xml_escape(source)));
        }
        body.push_str("</blockquote>\n");
    }
    body.push_str("</header>\n");

    for chapter in &manuscript.chapters {
        body.push_str(&format!(
            "<section class=\"chapter\">\n<h2>{}</h2>\n",
            xml_escape(&chapter.title)
        ));
        for (index, scene) in chapter.scenes.iter().enumerate() {
            if index > 0 {
                body.push_str("<hr class=\"scene\">\n");
            }
            let mut first = true;
            for block in &scene.blocks {
                match block {
                    Block::Paragraph(spans) => {
                        let class = if first { " class=\"first\"" } else { "" };
                        first = false;
                        body.push_str(&format!("<p{class}>{}</p>\n", spans_to_html(spans)));
                    }
                    Block::Heading { level, spans } => {
                        first = true;
                        let level = (*level).clamp(3, 6);
                        body.push_str(&format!("<h{level}>{}</h{level}>\n", spans_to_html(spans)));
                    }
                    Block::Quote(spans) => {
                        first = true;
                        body.push_str(&format!("<blockquote><p>{}</p></blockquote>\n", spans_to_html(spans)));
                    }
                    Block::SceneBreak => {
                        first = true;
                        body.push_str("<hr class=\"scene\">\n");
                    }
                }
            }
        }
        body.push_str("</section>\n");
    }

    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{}</title>\n\
<style>{STYLE}</style>\n</head>\n<body>\n{body}</body>\n</html>\n",
        xml_escape(&manuscript.title)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{ChapterContent, Contact, SceneContent, Span};

    #[test]
    fn a_book_comes_out_as_one_escaped_page() {
        let manuscript = Manuscript {
            title: "Fish & Chips".into(),
            subtitle: None,
            byline: "A".into(),
            surname: "A".into(),
            short_title: "F".into(),
            contact: Contact::default(),
            copyright_line: None,
            dedication: None,
            epigraph: None,
            epigraph_source: None,
            word_count: 2,
            chapters: vec![ChapterContent {
                title: "One".into(),
                scenes: vec![SceneContent {
                    title: String::new(),
                    blocks: vec![Block::Paragraph(vec![Span {
                        text: "<hi>".into(),
                        ..Span::default()
                    }])],
                }],
            }],
        };
        let html = write(&manuscript);
        assert!(html.contains("<title>Fish &amp; Chips</title>"));
        assert!(html.contains("&lt;hi&gt;"));
        assert!(html.contains("<h2>One</h2>"));
    }
}
