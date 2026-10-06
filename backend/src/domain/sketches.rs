//! Templates for story bible sketches.
//!
//! Built in, like the structural templates: the questions a writer is glad to
//! have been asked about a character or a place. Each field has a prompt as
//! well as a label, because "Wound" on its own is a word and "What happened
//! that they are still answering?" is a question someone can answer.

#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SketchTemplate {
    pub id: &'static str,
    #[schema(value_type = crate::domain::enums::BibleKind)]
    pub kind: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub fields: Vec<TemplateField>,
}

#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TemplateField {
    pub key: &'static str,
    pub label: &'static str,
    /// The question the field is asking.
    pub prompt: &'static str,
}

const fn f(key: &'static str, label: &'static str, prompt: &'static str) -> TemplateField {
    TemplateField { key, label, prompt }
}

pub fn all() -> Vec<SketchTemplate> {
    vec![
        SketchTemplate {
            id: "character-main",
            kind: "character",
            name: "Main character",
            description: "Someone the story happens to: what drives them, and what it costs.",
            fields: vec![
                f("role", "Role", "What are they to this story?"),
                f(
                    "want",
                    "Want",
                    "What are they after, that they would say out loud?",
                ),
                f(
                    "need",
                    "Need",
                    "What do they actually need, that they would not?",
                ),
                f(
                    "wound",
                    "Wound",
                    "What happened that they are still answering?",
                ),
                f(
                    "fear",
                    "Fear",
                    "What would they do almost anything to avoid?",
                ),
                f(
                    "secret",
                    "Secret",
                    "What do they know that nobody else does?",
                ),
                f("look", "Look", "What would a stranger notice first?"),
                f(
                    "voice",
                    "Voice",
                    "How do they talk — words they use, words they never would?",
                ),
                f(
                    "arc",
                    "Arc",
                    "Who are they at the start, and who at the end?",
                ),
                f("ties", "Ties", "Who matters to them, and how?"),
            ],
        },
        SketchTemplate {
            id: "character-supporting",
            kind: "character",
            name: "Supporting character",
            description: "Someone who stays in their lane — enough to keep them consistent.",
            fields: vec![
                f("role", "Role", "What are they to the protagonist?"),
                f("want", "Want", "What do they want from the protagonist?"),
                f("look", "Look", "What would a stranger notice first?"),
                f("voice", "Voice", "How do they talk?"),
                f("first", "First appears", "Where do we meet them?"),
            ],
        },
        SketchTemplate {
            id: "place-location",
            kind: "place",
            name: "Location",
            description: "Somewhere scenes happen, through every sense.",
            fields: vec![
                f(
                    "where",
                    "Where",
                    "Where is it, and how do people get there?",
                ),
                f(
                    "see",
                    "Sight",
                    "What do you see first, and what only later?",
                ),
                f("sound", "Sound", "What do you hear?"),
                f("smell", "Smell", "What does it smell of?"),
                f(
                    "who",
                    "Who's there",
                    "Who is usually here, and who never is?",
                ),
                f("history", "History", "What happened here before the story?"),
                f(
                    "meaning",
                    "Meaning",
                    "What does this place mean to the people in it?",
                ),
            ],
        },
        SketchTemplate {
            id: "place-room",
            kind: "place",
            name: "Room",
            description: "An interior: enough to block a scene in it.",
            fields: vec![
                f(
                    "layout",
                    "Layout",
                    "Where are the doors, the windows, the furniture?",
                ),
                f(
                    "light",
                    "Light",
                    "Where does the light come from, at what hour?",
                ),
                f(
                    "objects",
                    "Objects",
                    "What is on the table, the wall, the floor?",
                ),
                f("mood", "Mood", "How does it feel to walk into?"),
            ],
        },
        SketchTemplate {
            id: "thing-object",
            kind: "thing",
            name: "Object",
            description: "A thing the plot turns on: the letter, the lantern, the key.",
            fields: vec![
                f(
                    "what",
                    "What it is",
                    "Describe it as someone holding it would.",
                ),
                f("origin", "Origin", "Where did it come from?"),
                f("holder", "Who has it", "Who has it now, and who wants it?"),
                f("purpose", "In the plot", "What does it make happen?"),
            ],
        },
        SketchTemplate {
            id: "group-organisation",
            kind: "group",
            name: "Family or organisation",
            description: "A household, a crew, a firm, a congregation.",
            fields: vec![
                f("members", "Members", "Who belongs, and who leads?"),
                f("purpose", "Purpose", "What holds them together?"),
                f(
                    "rules",
                    "Rules",
                    "What is never done — and what is done but never said?",
                ),
                f("conflict", "Fault lines", "Where does it crack?"),
            ],
        },
        SketchTemplate {
            id: "note-timeline",
            kind: "note",
            name: "Timeline event",
            description: "Something that happened, for keeping the order straight.",
            fields: vec![
                f("when", "When", "When did it happen, in story time?"),
                f("what", "What", "What happened?"),
                f(
                    "who",
                    "Who knows",
                    "Who was there, and who found out later?",
                ),
            ],
        },
    ]
}

pub fn find(id: &str) -> Option<SketchTemplate> {
    all().into_iter().find(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_are_well_formed() {
        let all = all();
        let mut ids = std::collections::HashSet::new();
        for template in &all {
            assert!(
                ids.insert(template.id),
                "duplicate template {}",
                template.id
            );
            assert!(
                crate::domain::enums::BibleKind::accepts(template.kind),
                "{} has an unknown kind",
                template.id
            );
            let mut keys = std::collections::HashSet::new();
            for field in &template.fields {
                assert!(
                    keys.insert(field.key),
                    "{} repeats {}",
                    template.id,
                    field.key
                );
                assert!(field.prompt.ends_with('?') || field.prompt.ends_with('.'));
            }
        }
        // Every kind has at least one template to start from.
        for kind in crate::domain::enums::BibleKind::ALL {
            assert!(
                all.iter().any(|t| t.kind == *kind),
                "no template for {kind}"
            );
        }
    }
}
