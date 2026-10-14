//! Row structs. These mirror `schema.rs` exactly; anything the API shapes
//! differently lives in `routes::dto`.
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use super::schema::*;

macro_rules! row {
    ($name:ident, $table:ident, { $($(#[$fattr:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize, Deserialize, utoipa::ToSchema)]
        #[diesel(table_name = $table)]
        #[diesel(check_for_backend(diesel::sqlite::Sqlite))]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            $($(#[$fattr])* pub $field: $ty),*
        }
    };
}

/// `row!` for a table whose primary key is not `id`.
macro_rules! row_keyed {
    ($name:ident, $table:ident, ($($pk:ident),+), { $($(#[$fattr:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize, Deserialize, utoipa::ToSchema)]
        #[diesel(table_name = $table)]
        #[diesel(primary_key($($pk),+))]
        #[diesel(check_for_backend(diesel::sqlite::Sqlite))]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            $($(#[$fattr])* pub $field: $ty),*
        }
    };
}

row!(User, users, {
    id: String,
    email: String,
    display_name: String,
    #[serde(skip)]
    password_hash: String,
    bio: Option<String>,
    created_at: String,
    updated_at: String,
    /// The writer's daily target in words, when they have set one.
    daily_word_goal: Option<i32>,
    /// Contact details for a submission's first page. Kept on the writer rather
    /// than the work, since they are the same for every submission.
    legal_name: Option<String>,
    mailing_address: Option<String>,
    phone: Option<String>,
    /// When the address was proved. Invitations addressed to it reach this
    /// account only once it has been.
    email_verified_at: Option<String>,
    #[serde(skip)]
    totp_secret: Option<String>,
    /// When two-factor sign-in was turned on.
    totp_enabled_at: Option<String>,
    #[serde(skip)]
    totp_last_step: Option<i64>,
    /// The version of the terms they last agreed to.
    terms_version: Option<i32>,
    terms_accepted_at: Option<String>,
    /// The writer's weekly target in words, when they have set one.
    weekly_word_goal: Option<i32>,
    /// When they agreed that text they ask to summarise may be sent to the
    /// model provider. Unset, no summary is ever asked for on their behalf.
    summaries_allowed_at: Option<String>,
    /// A paragraph about the writer, reusable as back matter in any book.
    about_author: Option<String>,
    /// When they agreed that text they hand the writing assistant may be sent
    /// to the model provider. Separate from summaries: a different use.
    assistant_allowed_at: Option<String>,
    /// A site administrator: curates the daily prompts. Set from the command
    /// line only; no request body can change it.
    is_admin: bool,
    /// A blank paragraph (double Enter) is a scene break: in the web book
    /// editor it moves on to the next scene, and turning text into a project
    /// splits there as well as at a `--` line.
    double_enter_splits_scenes: bool,
    /// How the app looks for them, on every device.
    #[schema(value_type = crate::domain::enums::Theme)]
    theme: String,
    /// A site moderator: works the queue and sets short restrictions. Granted
    /// from the admin dashboard, never from a request body of the account's own.
    is_moderator: bool,
});

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize, utoipa::ToSchema)]
#[diesel(table_name = work_metadata)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[serde(rename_all = "camelCase")]
pub struct WorkMetadata {
    pub work_id: String,
    /// BCP 47.
    pub language: Option<String>,
    pub text_direction: Option<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub published: Option<String>,
    pub identifier: Option<String>,
    /// JSON array of strings.
    pub keywords: Option<String>,
    /// JSON array of `{name, role}`.
    pub contributors: Option<String>,
    pub cover_mime: Option<String>,
    pub cover_bytes: Option<i32>,
    pub cover_width: Option<i32>,
    pub cover_height: Option<i32>,
    pub updated_at: String,
}

row!(ExportProfile, export_profiles, {
    id: String,
    user_id: String,
    name: String,
    options: String,
    created_at: String,
    updated_at: String,
});

row!(ExportHistoryRow, export_history, {
    id: String,
    work_id: String,
    user_id: String,
    version_id: Option<String>,
    format: String,
    profile_name: Option<String>,
    options_hash: String,
    options: String,
    filename: String,
    byte_size: i32,
    content_hash: String,
    created_at: String,
});

row!(RefreshToken, refresh_tokens, {
    id: String,
    user_id: String,
    token_hash: String,
    expires_at: String,
    revoked_at: Option<String>,
    created_at: String,
    session_id: Option<String>,
});

row!(Session, sessions, {
    id: String,
    user_id: String,
    /// What the device said it was, for telling sessions apart in a list.
    user_agent: Option<String>,
    ip: Option<String>,
    created_at: String,
    last_seen_at: String,
    revoked_at: Option<String>,
});

row!(EmailToken, email_tokens, {
    id: String,
    user_id: String,
    purpose: String,
    email: String,
    token_hash: String,
    expires_at: String,
    used_at: Option<String>,
    created_at: String,
});

row!(AuditEvent, audit_events, {
    id: String,
    /// Who did it. Kept after their account is gone.
    actor_id: Option<String>,
    work_id: Option<String>,
    #[schema(value_type = crate::domain::enums::AuditAction)]
    action: String,
    detail: String,
    created_at: String,
});

row!(Template, templates, {
    id: String,
    owner_id: Option<String>,
    name: String,
    kind: String,
    description: String,
    structure_json: String,
    created_at: String,
});

row!(Work, works, {
    id: String,
    owner_id: String,
    title: String,
    subtitle: Option<String>,
    #[schema(value_type = crate::domain::enums::WorkKind)]
    kind: String,
    synopsis: Option<String>,
    template_id: Option<String>,
    #[schema(value_type = crate::domain::enums::WorkStatus)]
    status: String,
    current_version_id: Option<String>,
    word_count: i32,
    created_at: String,
    updated_at: String,
    /// The name on the byline, when it is not the writer's own.
    pen_name: Option<String>,
    /// Short form for a running header; a long title wraps and ruins it.
    short_title: Option<String>,
    dedication: Option<String>,
    epigraph: Option<String>,
    epigraph_source: Option<String>,
    copyright_line: Option<String>,
    /// The snapshot the writer wants to find without looking — what they last
    /// sent out.
    pinned_version_id: Option<String>,
    /// What to browse it under. Unset is allowed: plenty of writers do not
    /// know yet.
    #[schema(value_type = Option<crate::domain::enums::Genre>)]
    genre: Option<String>,
    /// How long the book is meant to be, when the writer has said.
    target_words: Option<i32>,
    /// The day they mean to get there by, `YYYY-MM-DD`.
    target_date: Option<String>,
});

row!(Chapter, chapters, {
    id: String,
    work_id: String,
    title: String,
    summary: Option<String>,
    position: i32,
    created_at: String,
    updated_at: String,
    /// `chapter`, `front`, `back` or `part`. Front and back matter are not
    /// numbered and are included or left out of an export on their own; a
    /// part is a divider before its chapters and is not numbered as one.
    role: String,
});

row!(Scene, scenes, {
    id: String,
    work_id: String,
    chapter_id: String,
    title: String,
    summary: Option<String>,
    pov: Option<String>,
    #[schema(value_type = crate::domain::enums::SceneStatus)]
    status: String,
    position: i32,
    word_count: i32,
    content_hash: String,
    created_at: String,
    updated_at: String,
    /// A target for this scene, for writers who plan at that level.
    target_words: Option<i32>,
    location: Option<String>,
    /// Free text: fiction rarely runs on the Gregorian calendar.
    story_date: Option<String>,
    /// Kept in the outline but not part of the book: not exported, not in a
    /// version, not in the book's word count. New scenes are part of it, and
    /// so is one a client cached before this existed: absent means `false`.
    #[serde(default)]
    excluded: bool,
});

row!(NotebookEntry, notebook_entries, {
    id: String,
    work_id: String,
    #[schema(value_type = crate::domain::enums::NotebookKind)]
    kind: String,
    name: String,
    summary: Option<String>,
    body: Option<String>,
    created_at: String,
    updated_at: String,
    /// JSON; read through `EntryView::fields`.
    #[serde(skip)]
    fields: Option<String>,
    /// The sketch template it was started from, if any.
    template_id: Option<String>,
    /// Where it sits among the entries of its kind, as the writer arranged
    /// them; 0 first. Absent from an entry cached before it existed.
    #[serde(default)]
    position: i32,
});

row!(SceneNotebookLink, scene_notebook_links, {
    id: String,
    scene_id: String,
    entry_id: String,
    created_at: String,
});

row!(WritingDay, writing_days, {
    id: String,
    user_id: String,
    /// The writer's local date, `YYYY-MM-DD`.
    day: String,
    net_words: i32,
    saves: i32,
    created_at: String,
    updated_at: String,
});

row!(SceneRevision, scene_revisions, {
    id: String,
    scene_id: String,
    work_id: String,
    author_id: String,
    content_hash: String,
    word_count: i32,
    created_at: String,
});

row!(TrackedChange, tracked_changes, {
    id: String,
    scene_id: String,
    work_id: String,
    author_id: String,
    #[schema(value_type = crate::domain::enums::ChangeKind)]
    kind: String,
    anchor_from: i32,
    anchor_to: i32,
    quote: Option<String>,
    payload: String,
    note: Option<String>,
    #[schema(value_type = crate::domain::enums::ChangeState)]
    state: String,
    resolved_by: Option<String>,
    resolved_at: Option<String>,
    created_at: String,
    /// The scene's `contentHash` when the proposal was written. `None` on rows
    /// predating this column; those always re-anchor by their quotation.
    base_hash: Option<String>,
});

row!(Version, versions, {
    id: String,
    work_id: String,
    number: i32,
    label: String,
    #[schema(value_type = crate::domain::enums::VersionState)]
    state: String,
    parent_version_id: Option<String>,
    created_by: String,
    notes: Option<String>,
    word_count: i32,
    created_at: String,
    published_at: Option<String>,
    finalized_at: Option<String>,
    /// `work`, `chapter` or `scene` — how much of the book this froze.
    #[schema(value_type = crate::domain::enums::PublicationScope)]
    scope: String,
    scope_ref_id: Option<String>,
});

row!(VersionTag, version_tags, {
    id: String,
    version_id: String,
    tag: String,
    created_at: String,
});

row!(VersionEntry, version_entries, {
    id: String,
    version_id: String,
    chapter_id: String,
    chapter_title: String,
    chapter_position: i32,
    scene_id: String,
    scene_title: String,
    scene_position: i32,
    content_hash: String,
    word_count: i32,
    /// The chapter's role when the snapshot was taken.
    chapter_role: String,
});

row!(Publication, publications, {
    id: String,
    work_id: String,
    version_id: String,
    #[schema(value_type = crate::domain::enums::Visibility)]
    visibility: String,
    #[schema(value_type = crate::domain::enums::PublicationScope)]
    scope: String,
    scope_ref_id: Option<String>,
    review_prompt: Option<String>,
    opens_at: Option<String>,
    closes_at: Option<String>,
    created_at: String,
    updated_at: String,
    /// How many people may comment. Beta readers are scarce, and twenty
    /// overlapping opinions are worse than five considered ones.
    max_reviewers: Option<i32>,
    /// Withhold notes on scenes a reader has not reached yet. Defaulted on for
    /// a serial, where not knowing yet is the whole point of reading one.
    spoiler_safe: i32,
    /// What share links are signed against; never sent anywhere.
    #[serde(skip)]
    link_secret: Option<String>,
});

row!(PublicationInvite, publication_invites, {
    id: String,
    publication_id: String,
    email: String,
    invited_by: String,
    accepted_at: Option<String>,
    accepted_by: Option<String>,
    created_at: String,
});

row!(ReaderCircle, reader_circles, {
    id: String,
    owner_id: String,
    name: String,
    created_at: String,
    updated_at: String,
});

row!(CircleMember, circle_members, {
    id: String,
    circle_id: String,
    email: String,
    name: Option<String>,
    created_at: String,
});

row!(ReadingProgress, reading_progress, {
    id: String,
    publication_id: String,
    user_id: String,
    scene_id: String,
    /// The furthest they ever reached, which is the interesting one: a reader
    /// who gave up in chapter three and later skimmed the ending has still
    /// stopped in chapter three.
    furthest_index: i32,
    scenes_total: i32,
    finished_at: Option<String>,
    created_at: String,
    updated_at: String,
});

row!(RevisionTask, revision_tasks, {
    id: String,
    work_id: String,
    scene_id: Option<String>,
    /// The note it came from, or `None` for one the author typed themselves —
    /// or one whose comment has since been deleted. The task is the author's
    /// either way.
    comment_id: Option<String>,
    body: String,
    state: String,
    created_at: String,
    updated_at: String,
    done_at: Option<String>,
    /// The passage it was written against, when it was.
    quote: Option<String>,
});

row!(CommentReport, comment_reports, {
    id: String,
    comment_id: String,
    reporter_id: String,
    reason: String,
    detail: Option<String>,
    created_at: String,
});

row!(BlockedReader, blocked_readers, {
    id: String,
    owner_id: String,
    user_id: String,
    created_at: String,
});

row!(WorkCollaborator, work_collaborators, {
    id: String,
    work_id: String,
    email: String,
    /// Empty until they first open the work.
    user_id: Option<String>,
    #[schema(value_type = crate::domain::enums::CollaboratorRole)]
    role: String,
    chapter_scoped: i32,
    invited_by: Option<String>,
    accepted_at: Option<String>,
    created_at: String,
    updated_at: String,
});

row!(WorkActivity, work_activity, {
    id: String,
    work_id: String,
    actor_id: String,
    #[schema(value_type = crate::domain::enums::ActivityKind)]
    kind: String,
    subject_id: Option<String>,
    /// The title as it was at the time.
    subject: String,
    /// How many acts this line stands for.
    times: i32,
    created_at: String,
    updated_at: String,
});

row!(Reaction, reactions, {
    id: String,
    publication_id: String,
    user_id: String,
    scene_id: String,
    /// Index of the block in the snapshot's `content` array. Blocks, not
    /// spans: two readers never highlight quite the same words, and a tally
    /// nobody can add up is not a signal.
    block_index: i32,
    kind: String,
    created_at: String,
});

row!(ReviewQuestion, review_questions, {
    id: String,
    publication_id: String,
    prompt: String,
    kind: String,
    scope: String,
    position: i32,
    created_at: String,
    updated_at: String,
    moderation_state: String,
});

row!(ReviewAnswer, review_answers, {
    id: String,
    question_id: String,
    user_id: String,
    /// Empty for a work-scoped question.
    chapter_id: String,
    rating: Option<i32>,
    body: Option<String>,
    created_at: String,
    updated_at: String,
    moderation_state: String,
});

row!(Comment, comments, {
    id: String,
    publication_id: String,
    version_id: String,
    scene_id: Option<String>,
    parent_id: Option<String>,
    author_id: String,
    body: String,
    anchor: Option<String>,
    anchor_quote: Option<String>,
    #[schema(value_type = crate::domain::enums::CommentCategory)]
    category: String,
    #[schema(value_type = crate::domain::enums::CommentState)]
    state: String,
    score: i32,
    created_at: String,
    updated_at: String,
    /// `visible`, `held` or `removed`: what site moderation has done with it,
    /// apart from the author's own delete.
    moderation_state: String,
    #[serde(skip)]
    original_body: Option<String>,
});

row!(CommentVote, comment_votes, {
    id: String,
    comment_id: String,
    user_id: String,
    value: i32,
    created_at: String,
});

// Insertable mirrors. Every field is owned so a handler can build one and hand
// it to the blocking pool without borrowing from the request.
macro_rules! new_row {
    ($name:ident, $table:ident, { $($field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, Insertable)]
        #[diesel(table_name = $table)]
        pub struct $name {
            $(pub $field: $ty),*
        }
    };
}

new_row!(NewUser, users, {
    id: String, email: String, display_name: String, password_hash: String,
    bio: Option<String>, created_at: String, updated_at: String,
    daily_word_goal: Option<i32>, legal_name: Option<String>,
    mailing_address: Option<String>, phone: Option<String>,
    email_verified_at: Option<String>, totp_secret: Option<String>,
    totp_enabled_at: Option<String>, totp_last_step: Option<i64>,
    terms_version: Option<i32>, terms_accepted_at: Option<String>,
    weekly_word_goal: Option<i32>, summaries_allowed_at: Option<String>,
    assistant_allowed_at: Option<String>,
});

new_row!(NewExportProfile, export_profiles, {
    id: String, user_id: String, name: String, options: String,
    created_at: String, updated_at: String,
});

new_row!(NewExportHistory, export_history, {
    id: String, work_id: String, user_id: String, version_id: Option<String>,
    format: String, profile_name: Option<String>, options_hash: String,
    options: String, filename: String, byte_size: i32, content_hash: String,
    created_at: String,
});

new_row!(NewWritingDay, writing_days, {
    id: String, user_id: String, day: String, net_words: i32, saves: i32,
    created_at: String, updated_at: String,
});

new_row!(NewRefreshToken, refresh_tokens, {
    id: String, user_id: String, token_hash: String, expires_at: String,
    revoked_at: Option<String>, created_at: String, session_id: Option<String>,
});

new_row!(NewAuditEvent, audit_events, {
    id: String, actor_id: Option<String>, work_id: Option<String>, action: String,
    detail: String, created_at: String, subject_id: Option<String>, reason: Option<String>,
});

new_row!(NewSummary, summaries, {
    id: String, work_id: String, target_kind: String, target_id: String,
    input_hash: String, summary: String, model: String, input_tokens: i32,
    output_tokens: i32, created_by: Option<String>, created_at: String,
});

row!(Annotation, annotations, {
    id: String,
    user_id: String,
    work_id: String,
    scene_id: String,
    /// `None` on the working draft.
    publication_id: Option<String>,
    #[schema(value_type = crate::domain::enums::AnnotationKind)]
    kind: String,
    from_pos: i32,
    to_pos: i32,
    quote: String,
    note: Option<String>,
    created_at: String,
    updated_at: String,
});

new_row!(NewAnnotation, annotations, {
    id: String, user_id: String, work_id: String, scene_id: String,
    publication_id: Option<String>, kind: String, from_pos: i32, to_pos: i32,
    quote: String, note: Option<String>, created_at: String, updated_at: String,
});

new_row!(NewSession, sessions, {
    id: String, user_id: String, user_agent: Option<String>, ip: Option<String>,
    created_at: String, last_seen_at: String, revoked_at: Option<String>,
});

new_row!(NewEmailToken, email_tokens, {
    id: String, user_id: String, purpose: String, email: String, token_hash: String,
    expires_at: String, used_at: Option<String>, created_at: String,
});

new_row!(NewRecoveryCode, recovery_codes, {
    id: String, user_id: String, code_hash: String, used_at: Option<String>,
});

new_row!(NewTemplate, templates, {
    id: String, owner_id: Option<String>, name: String, kind: String,
    description: String, structure_json: String, created_at: String,
});

new_row!(NewWork, works, {
    id: String, owner_id: String, title: String, subtitle: Option<String>,
    kind: String, synopsis: Option<String>, template_id: Option<String>,
    status: String, current_version_id: Option<String>, word_count: i32,
    created_at: String, updated_at: String, pen_name: Option<String>,
    short_title: Option<String>, dedication: Option<String>, epigraph: Option<String>,
    epigraph_source: Option<String>, copyright_line: Option<String>,
    pinned_version_id: Option<String>, genre: Option<String>,
    target_words: Option<i32>, target_date: Option<String>,
});

new_row!(NewChapter, chapters, {
    id: String, work_id: String, title: String, summary: Option<String>,
    position: i32, created_at: String, updated_at: String, role: String,
});

new_row!(NewScene, scenes, {
    id: String, work_id: String, chapter_id: String, title: String,
    summary: Option<String>, pov: Option<String>, status: String, position: i32,
    word_count: i32, content_hash: String, created_at: String, updated_at: String,
    target_words: Option<i32>, location: Option<String>, story_date: Option<String>,
});

new_row!(NewNotebookEntry, notebook_entries, {
    id: String, work_id: String, kind: String, name: String,
    summary: Option<String>, body: Option<String>,
    created_at: String, updated_at: String,
    fields: Option<String>, template_id: Option<String>, position: i32,
});

new_row!(NewSceneNotebookLink, scene_notebook_links, {
    id: String, scene_id: String, entry_id: String, created_at: String,
});

new_row!(NewSceneRevision, scene_revisions, {
    id: String, scene_id: String, work_id: String, author_id: String,
    content_hash: String, word_count: i32, created_at: String,
});

new_row!(NewTrackedChange, tracked_changes, {
    id: String, scene_id: String, work_id: String, author_id: String, kind: String,
    anchor_from: i32, anchor_to: i32, quote: Option<String>, payload: String,
    note: Option<String>, state: String, resolved_by: Option<String>,
    resolved_at: Option<String>, created_at: String, base_hash: Option<String>,
});

new_row!(NewVersion, versions, {
    id: String, work_id: String, number: i32, label: String, state: String,
    parent_version_id: Option<String>, created_by: String, notes: Option<String>,
    word_count: i32, created_at: String, published_at: Option<String>,
    finalized_at: Option<String>, scope: String, scope_ref_id: Option<String>,
});

new_row!(NewVersionTag, version_tags, {
    id: String, version_id: String, tag: String, created_at: String,
});

new_row!(NewVersionEntry, version_entries, {
    id: String, version_id: String, chapter_id: String, chapter_title: String,
    chapter_position: i32, scene_id: String, scene_title: String,
    scene_position: i32, content_hash: String, word_count: i32,
    chapter_role: String,
});

new_row!(NewPublication, publications, {
    id: String, work_id: String, version_id: String, visibility: String,
    scope: String, scope_ref_id: Option<String>, review_prompt: Option<String>,
    opens_at: Option<String>, closes_at: Option<String>, created_at: String,
    updated_at: String, max_reviewers: Option<i32>, spoiler_safe: i32,
    link_secret: Option<String>,
});

new_row!(NewPublicationInvite, publication_invites, {
    id: String, publication_id: String, email: String, invited_by: String,
    accepted_at: Option<String>, accepted_by: Option<String>, created_at: String,
});

new_row!(NewReaderCircle, reader_circles, {
    id: String, owner_id: String, name: String, created_at: String, updated_at: String,
});

new_row!(NewCircleMember, circle_members, {
    id: String, circle_id: String, email: String, name: Option<String>, created_at: String,
});

new_row!(NewReadingProgress, reading_progress, {
    id: String, publication_id: String, user_id: String, scene_id: String,
    furthest_index: i32, scenes_total: i32, finished_at: Option<String>,
    created_at: String, updated_at: String,
});

new_row!(NewComment, comments, {
    id: String, publication_id: String, version_id: String, scene_id: Option<String>,
    parent_id: Option<String>, author_id: String, body: String, anchor: Option<String>,
    anchor_quote: Option<String>, category: String, state: String, score: i32,
    created_at: String, updated_at: String,
});

new_row!(NewRevisionTask, revision_tasks, {
    id: String, work_id: String, scene_id: Option<String>, comment_id: Option<String>,
    body: String, state: String, created_at: String, updated_at: String,
    done_at: Option<String>, quote: Option<String>,
});

new_row!(NewCommentReport, comment_reports, {
    id: String, comment_id: String, reporter_id: String, reason: String,
    detail: Option<String>, created_at: String,
});

new_row!(NewBlockedReader, blocked_readers, {
    id: String, owner_id: String, user_id: String, created_at: String,
});

new_row!(NewFollow, follows, {
    id: String, follower_id: String, followee_id: String, created_at: String,
});

new_row!(NewWorkCollaborator, work_collaborators, {
    id: String, work_id: String, email: String, user_id: Option<String>, role: String,
    chapter_scoped: i32, invited_by: Option<String>, accepted_at: Option<String>,
    created_at: String, updated_at: String,
});

new_row!(NewCollaboratorChapter, collaborator_chapters, {
    id: String, collaborator_id: String, chapter_id: String,
});

new_row!(NewWorkActivity, work_activity, {
    id: String, work_id: String, actor_id: String, kind: String,
    subject_id: Option<String>, subject: String, times: i32,
    created_at: String, updated_at: String,
});

new_row!(NewReaction, reactions, {
    id: String, publication_id: String, user_id: String, scene_id: String,
    block_index: i32, kind: String, created_at: String,
});

new_row!(NewReviewQuestion, review_questions, {
    id: String, publication_id: String, prompt: String, kind: String,
    scope: String, position: i32, created_at: String, updated_at: String,
});

new_row!(NewReviewAnswer, review_answers, {
    id: String, question_id: String, user_id: String, chapter_id: String,
    rating: Option<i32>, body: Option<String>, created_at: String, updated_at: String,
});

new_row!(NewCommentVote, comment_votes, {
    id: String, comment_id: String, user_id: String, value: i32, created_at: String,
});

// --- Daily Prompts, the job runner, the inbox ------------------------------

row_keyed!(JobRun, job_runs, (job, key), {
    job: String,
    /// What the run is for: the day, for a daily job.
    key: String,
    started_at: String,
    finished_at: Option<String>,
    error: Option<String>,
    attempts: i32,
});

new_row!(NewJobRun, job_runs, {
    job: String, key: String, started_at: String, finished_at: Option<String>,
    error: Option<String>, attempts: i32,
});

row!(QueuedMail, mail_queue, {
    id: String,
    to_address: String,
    subject: String,
    body: String,
    html: Option<String>,
    headers: Option<String>,
    created_at: String,
    attempts: i32,
    next_attempt_at: String,
    give_up_at: String,
    sent_at: Option<String>,
    failed_at: Option<String>,
    last_error: Option<String>,
});

new_row!(NewQueuedMail, mail_queue, {
    id: String, to_address: String, subject: String, body: String,
    html: Option<String>, headers: Option<String>, created_at: String,
    attempts: i32, next_attempt_at: String, give_up_at: String,
    sent_at: Option<String>, failed_at: Option<String>, last_error: Option<String>,
});

row!(DailyPrompt, daily_prompts, {
    id: String,
    /// `YYYY-MM-DD`, the Pacific prompt day; `None` while it waits in the queue.
    day: Option<String>,
    /// Its place in the queue.
    position: Option<i32>,
    body: String,
    /// `curated` or `ai`. Only admins see it.
    source: String,
    created_by: Option<String>,
    created_at: String,
    updated_at: String,
});

new_row!(NewDailyPrompt, daily_prompts, {
    id: String, day: Option<String>, position: Option<i32>, body: String,
    source: String, created_by: Option<String>, created_at: String, updated_at: String,
});

row_keyed!(PromptEnrollment, prompt_enrollments, (user_id), {
    user_id: String,
    enrolled_at: String,
    /// Set while the writer has left; their preferences are kept for rejoining.
    left_at: Option<String>,
    email_enabled: bool,
    /// `HH:MM`, in `email_timezone`.
    email_local_time: String,
    /// An IANA name, such as `Europe/London`.
    email_timezone: String,
    #[serde(skip)]
    unsubscribe_token: String,
    updated_at: String,
});

new_row!(NewPromptEnrollment, prompt_enrollments, {
    user_id: String, enrolled_at: String, left_at: Option<String>,
    email_enabled: bool, email_local_time: String, email_timezone: String,
    unsubscribe_token: String, updated_at: String,
});

row!(PromptEntry, prompt_entries, {
    id: String,
    prompt_id: String,
    author_id: String,
    /// A ProseMirror document, as JSON.
    #[serde(skip)]
    body: String,
    content_hash: String,
    word_count: i32,
    /// `draft`, `submitted` or `withdrawn`.
    #[schema(value_type = crate::domain::enums::EntryState)]
    state: String,
    /// The first submission. Never moved by an edit or a resubmission.
    submitted_at: Option<String>,
    /// The last change made after submitting.
    edited_at: Option<String>,
    converted_work_id: Option<String>,
    created_at: String,
    updated_at: String,
    /// `visible`, `held` or `removed`, apart from the author's own state.
    moderation_state: String,
});

new_row!(NewPromptEntry, prompt_entries, {
    id: String, prompt_id: String, author_id: String, body: String,
    content_hash: String, word_count: i32, state: String,
    submitted_at: Option<String>, edited_at: Option<String>,
    converted_work_id: Option<String>, created_at: String, updated_at: String,
});

row!(PromptCritique, prompt_critiques, {
    id: String,
    entry_id: String,
    /// The critique this one replies to.
    parent_id: Option<String>,
    author_id: String,
    body: String,
    edited_at: Option<String>,
    /// Kept as a placeholder because it has replies.
    deleted_at: Option<String>,
    created_at: String,
    updated_at: String,
    moderation_state: String,
    #[serde(skip)]
    original_body: Option<String>,
});

new_row!(NewPromptCritique, prompt_critiques, {
    id: String, entry_id: String, parent_id: Option<String>, author_id: String,
    body: String, edited_at: Option<String>, deleted_at: Option<String>,
    created_at: String, updated_at: String,
});

row!(PromptCritiqueReport, prompt_critique_reports, {
    id: String,
    critique_id: String,
    reporter_id: String,
    #[schema(value_type = crate::domain::enums::ReportReason)]
    reason: String,
    detail: Option<String>,
    created_at: String,
});

new_row!(NewPromptCritiqueReport, prompt_critique_reports, {
    id: String, critique_id: String, reporter_id: String, reason: String,
    detail: Option<String>, created_at: String,
});

row!(InboxItem, inbox_items, {
    id: String,
    user_id: String,
    /// See `domain::inbox` for what each kind means.
    kind: String,
    subject_type: String,
    subject_id: String,
    context_type: Option<String>,
    context_id: Option<String>,
    actor_id: Option<String>,
    #[serde(skip)]
    detail: Option<String>,
    created_at: String,
    read_at: Option<String>,
});

new_row!(NewInboxItem, inbox_items, {
    id: String, user_id: String, kind: String, subject_type: String,
    subject_id: String, context_type: Option<String>, context_id: Option<String>,
    actor_id: Option<String>, detail: Option<String>, created_at: String,
    read_at: Option<String>,
});

row_keyed!(InboxDigest, inbox_digests, (user_id, day), {
    user_id: String,
    day: String,
    #[serde(skip)]
    summary_json: String,
    created_at: String,
    read_at: Option<String>,
});

// --- site administration and moderation ------------------------------------

// An audit line with the columns staff add: who it was about, and why.
row!(StaffAuditRow, audit_events, {
    id: String,
    actor_id: Option<String>,
    work_id: Option<String>,
    action: String,
    detail: String,
    created_at: String,
    subject_id: Option<String>,
    reason: Option<String>,
});

row!(StaffNote, staff_notes, {
    id: String,
    user_id: String,
    author_id: String,
    body: String,
    created_at: String,
});

row!(Restriction, restrictions, {
    id: String,
    user_id: String,
    /// `warning`, `ban`, `suspend` or `read_only`.
    kind: String,
    /// `site` or `discussion`.
    scope: String,
    starts_at: String,
    /// Absent for a permanent restriction.
    ends_at: Option<String>,
    /// A section of the community guidelines.
    reason_code: String,
    /// The message the writer is shown.
    reason_text: String,
    internal_note: Option<String>,
    created_by: String,
    created_at: String,
    lifted_at: Option<String>,
    lifted_by: Option<String>,
    lift_reason: Option<String>,
    ended_notified_at: Option<String>,
    /// JSON array of the queue items that prompted it.
    linked_items: Option<String>,
});

row!(Appeal, appeals, {
    id: String,
    restriction_id: String,
    body: String,
    /// `open`, `upheld` or `overturned`.
    state: String,
    decided_by: Option<String>,
    decision_text: Option<String>,
    created_at: String,
    decided_at: Option<String>,
});

row!(PermanentBan, permanent_bans, {
    id: String,
    email: String,
    email_canonical: String,
    former_user_id: Option<String>,
    display_name: String,
    reason_code: String,
    reason_text: String,
    restriction_id: Option<String>,
    banned_by: String,
    created_at: String,
    removed_at: Option<String>,
    removed_by: Option<String>,
    removed_reason: Option<String>,
});

row_keyed!(BlockedDomain, blocked_domains, (domain), {
    domain: String,
    note: String,
    created_by: String,
    created_at: String,
});

row!(ModerationItem, moderation_items, {
    id: String,
    subject_type: String,
    subject_id: String,
    subject_author_id: String,
    /// `open`, `held`, `actioned` or `dismissed`.
    state: String,
    /// JSON array: `report`, `word_filter`, `external`, `threshold`.
    sources: String,
    report_count: i32,
    top_reason: Option<String>,
    severity: i32,
    labels: Option<String>,
    first_flagged_at: String,
    last_flagged_at: String,
    claimed_by: Option<String>,
    claimed_until: Option<String>,
    decided_by: Option<String>,
    decided_at: Option<String>,
    decision: Option<String>,
});

row!(WordFilterTerm, word_filter_terms, {
    id: String,
    term: String,
    normalized: String,
    /// `word` or `substring`.
    match_kind: String,
    /// `block`, `hold`, `flag` or `mask`.
    action: String,
    /// JSON array of surfaces.
    applies_to: String,
    note: String,
    created_by: String,
    created_at: String,
    updated_at: String,
});

row!(ExternalCheck, external_checks, {
    id: String,
    subject_type: String,
    subject_id: String,
    provider: String,
    external_id: Option<String>,
    state: String,
    attempts: i32,
    recommended_action: Option<String>,
    labels: Option<String>,
    created_at: String,
    checked_at: Option<String>,
    next_attempt_at: String,
});

row!(ExternalEvent, external_events, {
    id: String,
    provider: String,
    event_id: String,
    kind: String,
    #[serde(skip)]
    payload: String,
    received_at: String,
    applied_at: Option<String>,
    error: Option<String>,
});
