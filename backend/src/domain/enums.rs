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
    // Variants may carry doc comments; they reach the generated schema, where
    // they are the only place a client learns what a value means.
    ($name:ident { $($(#[$variant_doc:meta])* $variant:ident => $wire:literal),* $(,)? }) => {
        // `schema(rename)` as well as `serde(rename)`: utoipa does not read
        // serde's per-variant renames, and without it the generated clients get
        // the Rust identifiers instead of the values that go over the wire.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
        pub enum $name {
            $(
                $(#[$variant_doc])*
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

            /// The value as it goes over the wire and into the column.
            pub fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => $wire),*
                }
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
    /// Only people the author invited by email.
    Invited => "invited",
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

api_enum!(NotebookKind {
    Character => "character",
    Place => "place",
    /// An object the plot turns on.
    Thing => "thing",
    /// A family, a crew, an organisation.
    Group => "group",
    Note => "note",
});

api_enum!(ChangeState {
    Pending => "pending",
    Accepted => "accepted",
    Rejected => "rejected",
});

api_enum!(ReactionKind {
    /// This landed.
    Loved => "loved",
    /// I did not follow what happened here.
    Confusing => "confusing",
    /// I was skimming by this point.
    Dragging => "dragging",
});

api_enum!(QuestionKind {
    /// Answered one to five.
    Rating => "rating",
    /// Answered in the reader's own words.
    Text => "text",
});

api_enum!(QuestionScope {
    /// Asked once about the whole publication.
    Work => "work",
    /// Asked again for each chapter in the published slice.
    Chapter => "chapter",
});

api_enum!(TaskState {
    /// Still to do.
    Open => "open",
    /// Dealt with.
    Done => "done",
});

api_enum!(ReportReason {
    /// Abusive or harassing.
    Abuse => "abuse",
    /// Advertising, or not feedback at all.
    Spam => "spam",
    /// Gives away something later in the book.
    Spoiler => "spoiler",
    /// Not about the writing.
    OffTopic => "off_topic",
    Other => "other",
});

api_enum!(Genre {
    Literary => "literary",
    Fantasy => "fantasy",
    ScienceFiction => "science_fiction",
    Mystery => "mystery",
    Thriller => "thriller",
    Romance => "romance",
    Horror => "horror",
    Historical => "historical",
    YoungAdult => "young_adult",
    Other => "other",
});

api_enum!(LengthBand {
    /// Under 7,500 words.
    Short => "short",
    /// 7,500 to 17,499.
    Novelette => "novelette",
    /// 17,500 to 39,999.
    Novella => "novella",
    /// 40,000 and up.
    Novel => "novel",
});

api_enum!(WindowFilter {
    /// Closes within the next three days.
    ClosingSoon => "closing_soon",
    /// Has no closing date at all.
    OpenEnded => "open_ended",
});

api_enum!(FeedSort {
    /// Most recently offered first.
    Newest => "newest",
    /// Least-read first, raised by how much the author has read for others.
    NeedsReaders => "needs_readers",
});

api_enum!(CollaboratorRole {
    /// Everything the creator can do but delete the work.
    Owner => "owner",
    /// Changes the text and the structure.
    Writer => "writer",
    /// Proposes changes and keeps the to-do list; does not change the text.
    Editor => "editor",
    /// Reads the working draft.
    Reader => "reader",
});

api_enum!(ActivityKind {
    SceneEdited => "scene_edited",
    SceneAdded => "scene_added",
    SceneRemoved => "scene_removed",
    ChapterAdded => "chapter_added",
    ChapterRemoved => "chapter_removed",
    ChangeProposed => "change_proposed",
    ChangeAccepted => "change_accepted",
    ChangeRejected => "change_rejected",
    TaskAdded => "task_added",
    TaskDone => "task_done",
    VersionSaved => "version_saved",
    VersionPublished => "version_published",
    PublicationOpened => "publication_opened",
    CollaboratorAdded => "collaborator_added",
    CollaboratorChanged => "collaborator_changed",
    CollaboratorRemoved => "collaborator_removed",
});

api_enum!(AuditAction {
    PasswordReset => "password_reset",
    PasswordChanged => "password_changed",
    TwoFactorEnabled => "two_factor_enabled",
    TwoFactorDisabled => "two_factor_disabled",
    SessionRevoked => "session_revoked",
    SessionsRevoked => "sessions_revoked",
    AccountExported => "account_exported",
    AccountDeleted => "account_deleted",
    WorkDeleted => "work_deleted",
    PublicationCreated => "publication_created",
    PublicationChanged => "publication_changed",
    PublicationWithdrawn => "publication_withdrawn",
    CollaboratorAdded => "collaborator_added",
    CollaboratorChanged => "collaborator_changed",
    CollaboratorRemoved => "collaborator_removed",
    ReaderInvited => "reader_invited",
    ReaderUninvited => "reader_uninvited",
    ReaderBlocked => "reader_blocked",
    ReaderUnblocked => "reader_unblocked",
    TermsAccepted => "terms_accepted",
    ShareLinkMade => "share_link_made",
    ShareLinksRevoked => "share_links_revoked",
    SummariesAllowed => "summaries_allowed",
    SummariesWithdrawn => "summaries_withdrawn",
    AssistantAllowed => "assistant_allowed",
    AssistantWithdrawn => "assistant_withdrawn",
});

api_enum!(AnnotationKind {
    /// A passage marked to find again or think about.
    Highlight => "highlight",
    /// A named place to come back to.
    Bookmark => "bookmark",
    /// A note written against a passage; unlike a to-do, it belongs to the
    /// place in the text it is about.
    Comment => "comment",
});
