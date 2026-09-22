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
    created_at: String, updated_at: String,
});

new_row!(NewChapter, chapters, {
    id: String, work_id: String, title: String, summary: Option<String>,
    position: i32, created_at: String, updated_at: String,
});

new_row!(NewScene, scenes, {
    id: String, work_id: String, chapter_id: String, title: String,
    summary: Option<String>, pov: Option<String>, status: String, position: i32,
    word_count: i32, content_hash: String, created_at: String, updated_at: String,
});

new_row!(NewSceneRevision, scene_revisions, {
    id: String, scene_id: String, work_id: String, author_id: String,
    content_hash: String, word_count: i32, created_at: String,
});

new_row!(NewTrackedChange, tracked_changes, {
    id: String, scene_id: String, work_id: String, author_id: String, kind: String,
    anchor_from: i32, anchor_to: i32, quote: Option<String>, payload: String,
    note: Option<String>, state: String, resolved_by: Option<String>,
    resolved_at: Option<String>, created_at: String,
});

new_row!(NewVersion, versions, {
    id: String, work_id: String, number: i32, label: String, state: String,
    parent_version_id: Option<String>, created_by: String, notes: Option<String>,
    word_count: i32, created_at: String, published_at: Option<String>,
    finalized_at: Option<String>,
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
    updated_at: String,
});

new_row!(NewComment, comments, {
    id: String, publication_id: String, version_id: String, scene_id: Option<String>,
    parent_id: Option<String>, author_id: String, body: String, anchor: Option<String>,
    anchor_quote: Option<String>, category: String, state: String, score: i32,
    created_at: String, updated_at: String,
});

new_row!(NewCommentVote, comment_votes, {
    id: String, comment_id: String, user_id: String, value: i32, created_at: String,
});
