// @generated-by-hand: kept in sync with migrations/*/up.sql.
// Regenerate with `diesel print-schema` once diesel_cli is installed.
diesel::table! {
    users (id) {
        id -> Text,
        email -> Text,
        display_name -> Text,
        password_hash -> Text,
        bio -> Nullable<Text>,
        created_at -> Text,
        updated_at -> Text,
    }
}

diesel::table! {
    refresh_tokens (id) {
        id -> Text,
        user_id -> Text,
        token_hash -> Text,
        expires_at -> Text,
        revoked_at -> Nullable<Text>,
        created_at -> Text,
    }
}

diesel::table! {
    templates (id) {
        id -> Text,
        owner_id -> Nullable<Text>,
        name -> Text,
        kind -> Text,
        description -> Text,
        structure_json -> Text,
        created_at -> Text,
    }
}

diesel::table! {
    works (id) {
        id -> Text,
        owner_id -> Text,
        title -> Text,
        subtitle -> Nullable<Text>,
        kind -> Text,
        synopsis -> Nullable<Text>,
        template_id -> Nullable<Text>,
        status -> Text,
        current_version_id -> Nullable<Text>,
        word_count -> Integer,
        created_at -> Text,
        updated_at -> Text,
    }
}

diesel::table! {
    chapters (id) {
        id -> Text,
        work_id -> Text,
        title -> Text,
        summary -> Nullable<Text>,
        position -> Integer,
        created_at -> Text,
        updated_at -> Text,
    }
}

diesel::table! {
    scenes (id) {
        id -> Text,
        work_id -> Text,
        chapter_id -> Text,
        title -> Text,
        summary -> Nullable<Text>,
        pov -> Nullable<Text>,
        status -> Text,
        position -> Integer,
        word_count -> Integer,
        content_hash -> Text,
        created_at -> Text,
        updated_at -> Text,
    }
}

diesel::table! {
    scene_revisions (id) {
        id -> Text,
        scene_id -> Text,
        work_id -> Text,
        author_id -> Text,
        content_hash -> Text,
        word_count -> Integer,
        created_at -> Text,
    }
}

diesel::table! {
    tracked_changes (id) {
        id -> Text,
        scene_id -> Text,
        work_id -> Text,
        author_id -> Text,
        kind -> Text,
        anchor_from -> Integer,
        anchor_to -> Integer,
        quote -> Nullable<Text>,
        payload -> Text,
        note -> Nullable<Text>,
        state -> Text,
        resolved_by -> Nullable<Text>,
        resolved_at -> Nullable<Text>,
        created_at -> Text,
    }
}

diesel::table! {
    versions (id) {
        id -> Text,
        work_id -> Text,
        number -> Integer,
        label -> Text,
        state -> Text,
        parent_version_id -> Nullable<Text>,
        created_by -> Text,
        notes -> Nullable<Text>,
        word_count -> Integer,
        created_at -> Text,
        published_at -> Nullable<Text>,
        finalized_at -> Nullable<Text>,
    }
}

diesel::table! {
    version_entries (id) {
        id -> Text,
        version_id -> Text,
        chapter_id -> Text,
        chapter_title -> Text,
        chapter_position -> Integer,
        scene_id -> Text,
        scene_title -> Text,
        scene_position -> Integer,
        content_hash -> Text,
        word_count -> Integer,
    }
}

diesel::table! {
    publications (id) {
        id -> Text,
        work_id -> Text,
        version_id -> Text,
        visibility -> Text,
        scope -> Text,
        scope_ref_id -> Nullable<Text>,
        review_prompt -> Nullable<Text>,
        opens_at -> Nullable<Text>,
        closes_at -> Nullable<Text>,
        created_at -> Text,
        updated_at -> Text,
    }
}

diesel::table! {
    comments (id) {
        id -> Text,
        publication_id -> Text,
        version_id -> Text,
        scene_id -> Nullable<Text>,
        parent_id -> Nullable<Text>,
        author_id -> Text,
        body -> Text,
        anchor -> Nullable<Text>,
        anchor_quote -> Nullable<Text>,
        category -> Text,
        state -> Text,
        score -> Integer,
        created_at -> Text,
        updated_at -> Text,
    }
}

diesel::table! {
    comment_votes (id) {
        id -> Text,
        comment_id -> Text,
        user_id -> Text,
        value -> Integer,
        created_at -> Text,
    }
}

diesel::joinable!(refresh_tokens -> users (user_id));
diesel::joinable!(works -> users (owner_id));
diesel::joinable!(chapters -> works (work_id));
diesel::joinable!(scenes -> chapters (chapter_id));
diesel::joinable!(scene_revisions -> scenes (scene_id));
diesel::joinable!(tracked_changes -> scenes (scene_id));
diesel::joinable!(versions -> works (work_id));
diesel::joinable!(version_entries -> versions (version_id));
diesel::joinable!(publications -> works (work_id));
diesel::joinable!(comments -> publications (publication_id));
diesel::joinable!(comment_votes -> comments (comment_id));
diesel::joinable!(comment_votes -> users (user_id));

diesel::allow_tables_to_appear_in_same_query!(
    chapters,
    comment_votes,
    comments,
    publications,
    refresh_tokens,
    scene_revisions,
    scenes,
    templates,
    tracked_changes,
    users,
    version_entries,
    versions,
    works,
);
