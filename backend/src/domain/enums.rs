//! The closed sets of strings the API accepts.
//!
//! Declared here so the generated clients get unions rather than bare `string`,
//! and so validation cannot drift from the contract: the lists the handlers
//! check against are these same enums, not a second copy.
//!
//! The columns stay `TEXT` and the Rust fields stay `String`. Mapping Diesel to
//! a custom SQL type would buy nothing here — SQLite has no enum type, and the
//! values are validated at the edge either way.

macro_rules! api_enum {
    ($name:ident { $($variant:ident => $wire:literal),* $(,)? }) => {
        // `schema(rename)` as well as `serde(rename)`: utoipa does not read
        // serde's per-variant renames, and without it the generated clients get
        // the Rust identifiers instead of the values that go over the wire.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
        pub enum $name {
            $(
                #[serde(rename = $wire)]
                #[schema(rename = $wire)]
                $variant
            ),*
        }

        // Some of these exist only to shape the generated schema — the value
        // is validated by whichever handler owns it, and several are
        // server-assigned and never parsed from a request at all.
        #[allow(dead_code)]
        impl $name {
            pub const ALL: &'static [&'static str] = &[$($wire),*];

            pub fn accepts(value: &str) -> bool {
                Self::ALL.contains(&value)
            }

            /// For the "must be one of …" half of a rejection message.
            pub fn options() -> String {
                Self::ALL.join(", ")
            }
        }
    };
}

api_enum!(WorkKind {
    Novel => "novel",
    Novella => "novella",
    ShortStory => "short_story",
    Serial => "serial",
});

api_enum!(WorkStatus {
    Draft => "draft",
    Published => "published",
    Final => "final",
    Archived => "archived",
});

api_enum!(SceneStatus {
    Outline => "outline",
    Drafting => "drafting",
    Revised => "revised",
    Done => "done",
});

api_enum!(VersionState {
    Draft => "draft",
    Published => "published",
    Final => "final",
});

api_enum!(Visibility {
    Unlisted => "unlisted",
    Platform => "platform",
    Public => "public",
});

api_enum!(PublicationScope {
    Work => "work",
    Chapter => "chapter",
    Scene => "scene",
});

api_enum!(CommentCategory {
    General => "general",
    Praise => "praise",
    Plot => "plot",
    Pacing => "pacing",
    Prose => "prose",
    Character => "character",
    Typo => "typo",
});

api_enum!(CommentState {
    Open => "open",
    Resolved => "resolved",
    Deleted => "deleted",
});

api_enum!(ChangeKind {
    Insert => "insert",
    Delete => "delete",
    Replace => "replace",
    Format => "format",
});

api_enum!(ChangeState {
    Pending => "pending",
    Accepted => "accepted",
    Rejected => "rejected",
});
