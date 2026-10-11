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
        assistant_allowed_at -> Nullable<Text>,
        is_admin -> Bool,
        double_enter_splits_scenes -> Bool,
        theme -> Text,
        welcomed_at -> Nullable<Text>,
        is_moderator -> Bool,
        deactivated_at -> Nullable<Text>,
        closing_at -> Nullable<Text>,
        purge_at -> Nullable<Text>,
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
        moderation_state -> Text,
        original_body -> Nullable<Text>,
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
        moderation_state -> Text,
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
        moderation_state -> Text,
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
        subject_id -> Nullable<Text>,
        reason -> Nullable<Text>,
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
        cover_width -> Nullable<Integer>,
        cover_height -> Nullable<Integer>,
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
        content_hash -> Text,
        created_at -> Text,
    }
}

diesel::table! {
    dictionary_words (user_id, word) {
        user_id -> Text,
        word -> Text,
        created_at -> Text,
    }
}

diesel::table! {
    writing_places (user_id, work_id) {
        user_id -> Text,
        work_id -> Text,
        scene_id -> Text,
        sel_from -> Integer,
        sel_to -> Integer,
        updated_at -> Text,
    }
}

diesel::table! {
    site_invites (id) {
        id -> Text,
        inviter_id -> Text,
        email -> Text,
        mailed -> Integer,
        created_at -> Text,
    }
}

diesel::joinable!(export_last_options -> works (work_id));
diesel::joinable!(export_history -> works (work_id));
diesel::joinable!(export_profiles -> users (user_id));
diesel::joinable!(work_metadata -> works (work_id));
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

diesel::table! {
    job_runs (job, key) {
        job -> Text,
        key -> Text,
        started_at -> Text,
        finished_at -> Nullable<Text>,
        error -> Nullable<Text>,
        attempts -> Integer,
    }
}

diesel::table! {
    mail_queue (id) {
        id -> Text,
        to_address -> Text,
        subject -> Text,
        body -> Text,
        html -> Nullable<Text>,
        headers -> Nullable<Text>,
        created_at -> Text,
        attempts -> Integer,
        next_attempt_at -> Text,
        give_up_at -> Text,
        sent_at -> Nullable<Text>,
        failed_at -> Nullable<Text>,
        last_error -> Nullable<Text>,
    }
}

diesel::table! {
    daily_prompts (id) {
        id -> Text,
        day -> Nullable<Text>,
        position -> Nullable<Integer>,
        body -> Text,
        source -> Text,
        created_by -> Nullable<Text>,
        created_at -> Text,
        updated_at -> Text,
    }
}

diesel::table! {
    prompt_enrollments (user_id) {
        user_id -> Text,
        enrolled_at -> Text,
        left_at -> Nullable<Text>,
        email_enabled -> Bool,
        email_local_time -> Text,
        email_timezone -> Text,
        unsubscribe_token -> Text,
        updated_at -> Text,
    }
}

diesel::table! {
    prompt_entries (id) {
        id -> Text,
        prompt_id -> Text,
        author_id -> Text,
        body -> Text,
        content_hash -> Text,
        word_count -> Integer,
        state -> Text,
        submitted_at -> Nullable<Text>,
        edited_at -> Nullable<Text>,
        converted_work_id -> Nullable<Text>,
        created_at -> Text,
        updated_at -> Text,
        moderation_state -> Text,
    }
}

diesel::table! {
    prompt_critiques (id) {
        id -> Text,
        entry_id -> Text,
        parent_id -> Nullable<Text>,
        author_id -> Text,
        body -> Text,
        edited_at -> Nullable<Text>,
        deleted_at -> Nullable<Text>,
        created_at -> Text,
        updated_at -> Text,
        moderation_state -> Text,
        original_body -> Nullable<Text>,
    }
}

diesel::table! {
    prompt_critique_reports (id) {
        id -> Text,
        critique_id -> Text,
        reporter_id -> Text,
        reason -> Text,
        detail -> Nullable<Text>,
        created_at -> Text,
    }
}

diesel::table! {
    inbox_items (id) {
        id -> Text,
        user_id -> Text,
        kind -> Text,
        subject_type -> Text,
        subject_id -> Text,
        context_type -> Nullable<Text>,
        context_id -> Nullable<Text>,
        actor_id -> Nullable<Text>,
        detail -> Nullable<Text>,
        created_at -> Text,
        read_at -> Nullable<Text>,
    }
}

diesel::table! {
    inbox_digests (user_id, day) {
        user_id -> Text,
        day -> Text,
        summary_json -> Text,
        created_at -> Text,
        read_at -> Nullable<Text>,
    }
}

diesel::table! {
    prompt_email_sends (user_id, day) {
        user_id -> Text,
        day -> Text,
        sent_at -> Text,
    }
}

diesel::joinable!(prompt_entries -> daily_prompts (prompt_id));
diesel::joinable!(prompt_entries -> users (author_id));
diesel::joinable!(prompt_critiques -> prompt_entries (entry_id));
diesel::joinable!(prompt_critiques -> users (author_id));
diesel::joinable!(prompt_critique_reports -> prompt_critiques (critique_id));
diesel::joinable!(prompt_enrollments -> users (user_id));
diesel::joinable!(prompt_email_sends -> users (user_id));

diesel::table! {
    staff_notes (id) {
        id -> Text,
        user_id -> Text,
        author_id -> Text,
        body -> Text,
        created_at -> Text,
    }
}

diesel::table! {
    restrictions (id) {
        id -> Text,
        user_id -> Text,
        kind -> Text,
        scope -> Text,
        starts_at -> Text,
        ends_at -> Nullable<Text>,
        reason_code -> Text,
        reason_text -> Text,
        internal_note -> Nullable<Text>,
        created_by -> Text,
        created_at -> Text,
        lifted_at -> Nullable<Text>,
        lifted_by -> Nullable<Text>,
        lift_reason -> Nullable<Text>,
        ended_notified_at -> Nullable<Text>,
        linked_items -> Nullable<Text>,
    }
}

diesel::table! {
    appeals (id) {
        id -> Text,
        restriction_id -> Text,
        body -> Text,
        state -> Text,
        decided_by -> Nullable<Text>,
        decision_text -> Nullable<Text>,
        created_at -> Text,
        decided_at -> Nullable<Text>,
    }
}

diesel::table! {
    permanent_bans (id) {
        id -> Text,
        email -> Text,
        email_canonical -> Text,
        former_user_id -> Nullable<Text>,
        display_name -> Text,
        reason_code -> Text,
        reason_text -> Text,
        restriction_id -> Nullable<Text>,
        banned_by -> Text,
        created_at -> Text,
        removed_at -> Nullable<Text>,
        removed_by -> Nullable<Text>,
        removed_reason -> Nullable<Text>,
    }
}

diesel::table! {
    blocked_domains (domain) {
        domain -> Text,
        note -> Text,
        created_by -> Text,
        created_at -> Text,
    }
}

diesel::table! {
    reports (id) {
        id -> Text,
        subject_type -> Text,
        subject_id -> Text,
        subject_author_id -> Text,
        reporter_id -> Text,
        reason -> Text,
        detail -> Nullable<Text>,
        created_at -> Text,
    }
}

diesel::table! {
    moderation_items (id) {
        id -> Text,
        subject_type -> Text,
        subject_id -> Text,
        subject_author_id -> Text,
        state -> Text,
        sources -> Text,
        report_count -> Integer,
        top_reason -> Nullable<Text>,
        severity -> Integer,
        labels -> Nullable<Text>,
        first_flagged_at -> Text,
        last_flagged_at -> Text,
        claimed_by -> Nullable<Text>,
        claimed_until -> Nullable<Text>,
        decided_by -> Nullable<Text>,
        decided_at -> Nullable<Text>,
        decision -> Nullable<Text>,
    }
}

diesel::table! {
    word_filter_terms (id) {
        id -> Text,
        term -> Text,
        normalized -> Text,
        match_kind -> Text,
        action -> Text,
        applies_to -> Text,
        note -> Text,
        created_by -> Text,
        created_at -> Text,
        updated_at -> Text,
    }
}

diesel::table! {
    word_filter_allow (word) {
        word -> Text,
        created_by -> Text,
        created_at -> Text,
    }
}

diesel::table! {
    moderation_settings (key) {
        key -> Text,
        value -> Text,
        updated_by -> Nullable<Text>,
        updated_at -> Text,
    }
}

diesel::table! {
    external_checks (id) {
        id -> Text,
        subject_type -> Text,
        subject_id -> Text,
        provider -> Text,
        external_id -> Nullable<Text>,
        state -> Text,
        attempts -> Integer,
        recommended_action -> Nullable<Text>,
        labels -> Nullable<Text>,
        created_at -> Text,
        checked_at -> Nullable<Text>,
        next_attempt_at -> Text,
    }
}

diesel::table! {
    external_events (id) {
        id -> Text,
        provider -> Text,
        event_id -> Text,
        kind -> Text,
        payload -> Text,
        received_at -> Text,
        applied_at -> Nullable<Text>,
        error -> Nullable<Text>,
    }
}

diesel::table! {
    external_outbox (id) {
        id -> Text,
        provider -> Text,
        external_id -> Text,
        decision -> Text,
        attempts -> Integer,
        created_at -> Text,
        sent_at -> Nullable<Text>,
    }
}

diesel::table! {
    emailed_exports (id) {
        id -> Text,
        token_hash -> Text,
        email -> Text,
        path -> Text,
        expires_at -> Text,
        downloaded_at -> Nullable<Text>,
        created_at -> Text,
    }
}

diesel::joinable!(restrictions -> users (user_id));
diesel::joinable!(appeals -> restrictions (restriction_id));
diesel::joinable!(staff_notes -> users (user_id));

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
    dictionary_words,
    writing_places,
    site_invites,
    job_runs,
    mail_queue,
    daily_prompts,
    prompt_enrollments,
    prompt_entries,
    prompt_critiques,
    prompt_critique_reports,
    inbox_items,
    inbox_digests,
    prompt_email_sends,
    staff_notes,
    restrictions,
    appeals,
    permanent_bans,
    blocked_domains,
    reports,
    moderation_items,
    word_filter_terms,
    word_filter_allow,
    moderation_settings,
    external_checks,
    external_events,
    external_outbox,
    emailed_exports,
);
