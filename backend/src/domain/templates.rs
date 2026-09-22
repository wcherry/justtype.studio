//! Built-in structural templates. A template is purely a starting skeleton of
//! chapters and scenes; once a work is created it owns its own structure.
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::db::models::NewTemplate;
use crate::db::schema::templates;
use crate::db::Conn;
use crate::error::ApiResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateStructure {
    #[serde(default)]
    pub chapters: Vec<TemplateChapter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateChapter {
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub scenes: Vec<TemplateScene>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateScene {
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    /// Prefilled prose, if the template wants to seed the scene with a prompt.
    #[serde(default)]
    pub body: Option<String>,
}

struct BuiltIn {
    id: &'static str,
    name: &'static str,
    kind: &'static str,
    description: &'static str,
    structure: fn() -> serde_json::Value,
}

fn chapter(title: &str, summary: &str, scenes: &[(&str, &str)]) -> serde_json::Value {
    json!({
        "title": title,
        "summary": summary,
        "scenes": scenes.iter().map(|(t, s)| json!({ "title": t, "summary": s })).collect::<Vec<_>>(),
    })
}

const BUILT_INS: &[BuiltIn] = &[
    BuiltIn {
        id: "tpl-blank",
        name: "Blank",
        kind: "any",
        description: "One empty chapter with one empty scene. Bring your own structure.",
        structure: || json!({ "chapters": [chapter("Chapter One", "", &[("Scene 1", "")])] }),
    },
    BuiltIn {
        id: "tpl-short-story",
        name: "Short Story",
        kind: "short_story",
        description: "A single-chapter arc: hook, turn, and landing.",
        structure: || {
            json!({ "chapters": [chapter(
                "Story",
                "A complete arc in one sitting.",
                &[
                    ("Hook", "Open in motion. Establish who wants what."),
                    ("Complication", "The want meets its obstacle."),
                    ("Turn", "The cost becomes clear and a choice is made."),
                    ("Landing", "Show what the choice changed."),
                ],
            )] })
        },
    },
    BuiltIn {
        id: "tpl-three-act",
        name: "Three-Act Novel",
        kind: "novel",
        description: "Classic three-act structure with beat-level scene stubs.",
        structure: || {
            json!({ "chapters": [
                chapter("Act I — Setup", "Ordinary world through the first door.", &[
                    ("Opening Image", "The world before the story touches it."),
                    ("Inciting Incident", "The event that makes the ordinary impossible."),
                    ("Debate", "The protagonist resists the call."),
                    ("Break Into Two", "They commit, and the world changes shape."),
                ]),
                chapter("Act II — Confrontation", "Rising cost, falling certainty.", &[
                    ("Fun and Games", "The promise of the premise delivered."),
                    ("Midpoint", "A false victory or false defeat resets the stakes."),
                    ("Bad Guys Close In", "Pressure from outside and inside."),
                    ("All Is Lost", "The lowest point; something is truly lost."),
                    ("Dark Night of the Soul", "Sit in the loss before the answer."),
                ]),
                chapter("Act III — Resolution", "The answer earned and paid for.", &[
                    ("Break Into Three", "The insight that makes action possible."),
                    ("Finale", "Confrontation on the protagonist's terms."),
                    ("Final Image", "The mirror of the opening image."),
                ]),
            ] })
        },
    },
    BuiltIn {
        id: "tpl-heros-journey",
        name: "Hero's Journey",
        kind: "novel",
        description: "Twelve-stage monomyth, one chapter per movement.",
        structure: || {
            json!({ "chapters": [
                chapter("Departure", "Leaving the known world.", &[
                    ("The Ordinary World", ""),
                    ("The Call to Adventure", ""),
                    ("Refusal of the Call", ""),
                    ("Meeting the Mentor", ""),
                    ("Crossing the Threshold", ""),
                ]),
                chapter("Initiation", "Trials in the unknown.", &[
                    ("Tests, Allies, Enemies", ""),
                    ("Approach to the Inmost Cave", ""),
                    ("The Ordeal", ""),
                    ("The Reward", ""),
                ]),
                chapter("Return", "Bringing the elixir home.", &[
                    ("The Road Back", ""),
                    ("Resurrection", ""),
                    ("Return with the Elixir", ""),
                ]),
            ] })
        },
    },
    BuiltIn {
        id: "tpl-serial-episode",
        name: "Serial Episode",
        kind: "serial",
        description: "Episode skeleton for serialised fiction, ending on a hook.",
        structure: || {
            json!({ "chapters": [chapter(
                "Episode",
                "Self-contained beat that advances the season arc.",
                &[
                    ("Cold Open", "Land the reader mid-scene."),
                    ("Episode Problem", "The question this episode answers."),
                    ("Escalation", "Two turns of the screw."),
                    ("Resolution", "Answer the episode question."),
                    ("Hook", "Open the next question."),
                ],
            )] })
        },
    },
];

/// Idempotent: built-ins are keyed by a stable id so restarts do not duplicate
/// them, and edits to this file propagate on the next boot.
pub fn seed(conn: &mut Conn, now: &str) -> ApiResult<()> {
    for built_in in BUILT_INS {
        let row = NewTemplate {
            id: built_in.id.to_string(),
            owner_id: None,
            name: built_in.name.to_string(),
            kind: built_in.kind.to_string(),
            description: built_in.description.to_string(),
            structure_json: (built_in.structure)().to_string(),
            created_at: now.to_string(),
        };
        diesel::insert_into(templates::table)
            .values(&row)
            .on_conflict(templates::id)
            .do_update()
            .set((
                templates::name.eq(row.name.clone()),
                templates::kind.eq(row.kind.clone()),
                templates::description.eq(row.description.clone()),
                templates::structure_json.eq(row.structure_json.clone()),
            ))
            .execute(conn)?;
    }
    Ok(())
}
