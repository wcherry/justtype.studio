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
        daily_word_goal -> Nullable<Integer>,
        legal_name -> Nullable<Text>,
        mailing_address -> Nullable<Text>,
        phone -> Nullable<Text>,
        email_verified_at -> Nullable<Text>,
        totp_secret -> Nullable<Text>,
        totp_enabled_at -> Nullable<Text>,
        totp_last_step -> Nullable<BigInt>,
        terms_version -> Nullable<Integer>,
        terms_accepted_at -> Nullable<Text>,
        weekly_word_goal -> Nullable<Integer>,
        summaries_allowed_at -> Nullable<Text>,
        about_author -> Nullable<Text>,
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
        session_id -> Nullable<Text>,
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
        pen_name -> Nullable<Text>,
        short_title -> Nullable<Text>,
        dedication -> Nullable<Text>,
        epigraph -> Nullable<Text>,
        epigraph_source -> Nullable<Text>,
        copyright_line -> Nullable<Text>,
        pinned_version_id -> Nullable<Text>,
        genre -> Nullable<Text>,
        target_words -> Nullable<Integer>,
        target_date -> Nullable<Text>,
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
        role -> Text,
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
        target_words -> Nullable<Integer>,
        location -> Nullable<Text>,
        story_date -> Nullable<Text>,
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
    notebook_entries (id) {
        id -> Text,
        work_id -> Text,
        kind -> Text,
        name -> Text,
        summary -> Nullable<Text>,
        body -> Nullable<Text>,
        created_at -> Text,
        updated_at -> Text,
        fields -> Nullable<Text>,
        template_id -> Nullable<Text>,
    }
}

diesel::table! {
    scene_notebook_links (id) {
        id -> Text,
        scene_id -> Text,
        entry_id -> Text,
        created_at -> Text,
    }
}

diesel::table! {
    writing_days (id) {
        id -> Text,
        user_id -> Text,
        day -> Text,
        net_words -> Integer,
        saves -> Integer,
        created_at -> Text,
        updated_at -> Text,
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
        base_hash -> Nullable<Text>,
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
        scope -> Text,
        scope_ref_id -> Nullable<Text>,
    }
}

diesel::table! {
    version_tags (id) {
        id -> Text,
        version_id -> Text,
        tag -> Text,
        created_at -> Text,
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
        chapter_role -> Text,
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
        max_reviewers -> Nullable<Integer>,
        spoiler_safe -> Integer,
        link_secret -> Nullable<Text>,
    }
}

diesel::table! {
    publication_invites (id) {
        id -> Text,
        publication_id -> Text,
        email -> Text,
        invited_by -> Text,
        accepted_at -> Nullable<Text>,
        accepted_by -> Nullable<Text>,
        created_at -> Text,
    }
}

diesel::table! {
    reader_circles (id) {
        id -> Text,
        owner_id -> Text,
        name -> Text,
        created_at -> Text,
        updated_at -> Text,
    }
}

diesel::table! {
    circle_members (id) {
        id -> Text,
        circle_id -> Text,
        email -> Text,
        name -> Nullable<Text>,
        created_at -> Text,
    }
}

diesel::table! {
    reading_progress (id) {
        id -> Text,
        publication_id -> Text,
        user_id -> Text,
        scene_id -> Text,
        furthest_index -> Integer,
        scenes_total -> Integer,
        finished_at -> Nullable<Text>,
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
    revision_tasks (id) {
        id -> Text,
        work_id -> Text,
        scene_id -> Nullable<Text>,
        comment_id -> Nullable<Text>,
        body -> Text,
        state -> Text,
        created_at -> Text,
        updated_at -> Text,
        done_at -> Nullable<Text>,
        quote -> Nullable<Text>,
    }
}

diesel::table! {
    comment_reports (id) {
        id -> Text,
        comment_id -> Text,
        reporter_id -> Text,
        reason -> Text,
        detail -> Nullable<Text>,
        created_at -> Text,
    }
}

diesel::table! {
    blocked_readers (id) {
        id -> Text,
        owner_id -> Text,
        user_id -> Text,
        created_at -> Text,
    }
}

diesel::table! {
    reactions (id) {
        id -> Text,
        publication_id -> Text,
        user_id -> Text,
        scene_id -> Text,
        block_index -> Integer,
        kind -> Text,
        created_at -> Text,
    }
}

diesel::table! {
    review_questions (id) {
        id -> Text,
        publication_id -> Text,
        prompt -> Text,
        kind -> Text,
        scope -> Text,
        position -> Integer,
        created_at -> Text,
        updated_at -> Text,
    }
}

diesel::table! {
    review_answers (id) {
        id -> Text,
        question_id -> Text,
        user_id -> Text,
        chapter_id -> Text,
        rating -> Nullable<Integer>,
        body -> Nullable<Text>,
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

diesel::table! {
    follows (id) {
        id -> Text,
        follower_id -> Text,
        followee_id -> Text,
        created_at -> Text,
    }
}

diesel::table! {
    work_collaborators (id) {
        id -> Text,
        work_id -> Text,
        email -> Text,
        user_id -> Nullable<Text>,
        role -> Text,
        chapter_scoped -> Integer,
        invited_by -> Nullable<Text>,
        accepted_at -> Nullable<Text>,
        created_at -> Text,
        updated_at -> Text,
    }
}

diesel::table! {
    collaborator_chapters (id) {
        id -> Text,
        collaborator_id -> Text,
        chapter_id -> Text,
    }
}

diesel::table! {
    work_activity (id) {
        id -> Text,
        work_id -> Text,
        actor_id -> Text,
        kind -> Text,
        subject_id -> Nullable<Text>,
        subject -> Text,
        times -> Integer,
        created_at -> Text,
        updated_at -> Text,
    }
}

diesel::table! {
    sessions (id) {
        id -> Text,
        user_id -> Text,
        user_agent -> Nullable<Text>,
        ip -> Nullable<Text>,
        created_at -> Text,
        last_seen_at -> Text,
        revoked_at -> Nullable<Text>,
    }
}

diesel::table! {
    email_tokens (id) {
        id -> Text,
        user_id -> Text,
        purpose -> Text,
        email -> Text,
        token_hash -> Text,
        expires_at -> Text,
        used_at -> Nullable<Text>,
        created_at -> Text,
    }
}

diesel::table! {
    recovery_codes (id) {
        id -> Text,
        user_id -> Text,
        code_hash -> Text,
        used_at -> Nullable<Text>,
    }
}

diesel::table! {
    audit_events (id) {
        id -> Text,
        actor_id -> Nullable<Text>,
        work_id -> Nullable<Text>,
        action -> Text,
        detail -> Text,
        created_at -> Text,
    }
}

diesel::table! {
    summaries (id) {
        id -> Text,
        work_id -> Text,
        target_kind -> Text,
        target_id -> Text,
        input_hash -> Text,
        summary -> Text,
        model -> Text,
        input_tokens -> Integer,
        output_tokens -> Integer,
        created_by -> Nullable<Text>,
        created_at -> Text,
    }
}

diesel::table! {
    annotations (id) {
        id -> Text,
        user_id -> Text,
        work_id -> Text,
        scene_id -> Text,
        publication_id -> Nullable<Text>,
        kind -> Text,
        from_pos -> Integer,
        to_pos -> Integer,
        quote -> Text,
        note -> Nullable<Text>,
        created_at -> Text,
        updated_at -> Text,
    }
}

diesel::table! {
    work_metadata (work_id) {
        work_id -> Text,
        language -> Nullable<Text>,
        text_direction -> Nullable<Text>,
        description -> Nullable<Text>,
        publisher -> Nullable<Text>,
        published -> Nullable<Text>,
        identifier -> Nullable<Text>,
        keywords -> Nullable<Text>,
        contributors -> Nullable<Text>,
        cover_mime -> Nullable<Text>,
        cover_bytes -> Nullable<Integer>,
        updated_at -> Text,
    }
}

diesel::table! {
    export_profiles (id) {
        id -> Text,
        user_id -> Text,
        name -> Text,
        options -> Text,
        created_at -> Text,
        updated_at -> Text,
    }
}

diesel::table! {
    export_last_options (work_id, format) {
        work_id -> Text,
        format -> Text,
        options -> Text,
        used_at -> Text,
    }
}

diesel::table! {
    export_history (id) {
        id -> Text,
        work_id -> Text,
        user_id -> Text,
        version_id -> Nullable<Text>,
        format -> Text,
        profile_name -> Nullable<Text>,
        options_hash -> Text,
        options -> Text,
        filename -> Text,
        byte_size -> Integer,
        created_at -> Text,
    }
}

diesel::joinable!(refresh_tokens -> users (user_id));
diesel::joinable!(sessions -> users (user_id));
diesel::joinable!(work_collaborators -> works (work_id));
diesel::joinable!(collaborator_chapters -> work_collaborators (collaborator_id));
diesel::joinable!(work_activity -> works (work_id));
diesel::joinable!(writing_days -> users (user_id));
diesel::joinable!(notebook_entries -> works (work_id));
diesel::joinable!(scene_notebook_links -> notebook_entries (entry_id));
diesel::joinable!(works -> users (owner_id));
diesel::joinable!(chapters -> works (work_id));
diesel::joinable!(scenes -> chapters (chapter_id));
diesel::joinable!(scene_revisions -> scenes (scene_id));
diesel::joinable!(tracked_changes -> scenes (scene_id));
diesel::joinable!(versions -> works (work_id));
diesel::joinable!(version_entries -> versions (version_id));
diesel::joinable!(version_tags -> versions (version_id));
diesel::joinable!(publication_invites -> publications (publication_id));
diesel::joinable!(circle_members -> reader_circles (circle_id));
diesel::joinable!(reading_progress -> publications (publication_id));
diesel::joinable!(publications -> works (work_id));
diesel::joinable!(comments -> publications (publication_id));
diesel::joinable!(comment_votes -> comments (comment_id));
diesel::joinable!(reactions -> publications (publication_id));
diesel::joinable!(review_questions -> publications (publication_id));
diesel::joinable!(review_answers -> review_questions (question_id));
diesel::joinable!(revision_tasks -> works (work_id));
diesel::joinable!(comment_reports -> comments (comment_id));
diesel::joinable!(comment_votes -> users (user_id));

diesel::allow_tables_to_appear_in_same_query!(
    annotations,
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
    writing_days,
    notebook_entries,
    scene_notebook_links,
    version_tags,
    publication_invites,
    reader_circles,
    circle_members,
    reading_progress,
    reactions,
    review_questions,
    review_answers,
    revision_tasks,
    comment_reports,
    blocked_readers,
    follows,
    work_collaborators,
    collaborator_chapters,
    work_activity,
    sessions,
    email_tokens,
    recovery_codes,
    audit_events,
    summaries,
    work_metadata,
    export_profiles,
    export_last_options,
    export_history,
);
