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
});

row!(RefreshToken, refresh_tokens, {
    id: String,
    user_id: String,
    token_hash: String,
    expires_at: String,
    revoked_at: Option<String>,
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
});

row!(Chapter, chapters, {
    id: String,
    work_id: String,
    title: String,
    summary: Option<String>,
    position: i32,
    created_at: String,
    updated_at: String,
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
});

row!(BibleEntry, bible_entries, {
    id: String,
    work_id: String,
    #[schema(value_type = crate::domain::enums::BibleKind)]
    kind: String,
    name: String,
    summary: Option<String>,
    body: Option<String>,
    created_at: String,
    updated_at: String,
});

row!(SceneBibleLink, scene_bible_links, {
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
});

new_row!(NewWritingDay, writing_days, {
    id: String, user_id: String, day: String, net_words: i32, saves: i32,
    created_at: String, updated_at: String,
});

new_row!(NewRefreshToken, refresh_tokens, {
    id: String, user_id: String, token_hash: String, expires_at: String,
    revoked_at: Option<String>, created_at: String,
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
});

new_row!(NewChapter, chapters, {
    id: String, work_id: String, title: String, summary: Option<String>,
    position: i32, created_at: String, updated_at: String,
});

new_row!(NewScene, scenes, {
    id: String, work_id: String, chapter_id: String, title: String,
    summary: Option<String>, pov: Option<String>, status: String, position: i32,
    word_count: i32, content_hash: String, created_at: String, updated_at: String,
    target_words: Option<i32>, location: Option<String>, story_date: Option<String>,
});

new_row!(NewBibleEntry, bible_entries, {
    id: String, work_id: String, kind: String, name: String,
    summary: Option<String>, body: Option<String>,
    created_at: String, updated_at: String,
});

new_row!(NewSceneBibleLink, scene_bible_links, {
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
});

new_row!(NewPublication, publications, {
    id: String, work_id: String, version_id: String, visibility: String,
    scope: String, scope_ref_id: Option<String>, review_prompt: Option<String>,
    opens_at: Option<String>, closes_at: Option<String>, created_at: String,
    updated_at: String, max_reviewers: Option<i32>, spoiler_safe: i32,
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
    done_at: Option<String>,
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
