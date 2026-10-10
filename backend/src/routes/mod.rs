pub mod account;
pub mod admin;
pub mod annotations;
pub mod assistant;
pub mod audit;
pub mod auth;
pub mod book_details;
pub mod changes;
pub mod collaborators;
pub mod comments;
pub mod critiques;
pub mod discovery;
pub mod export;
pub mod export_settings;
pub mod feedback;
pub mod goals;
pub mod inbox;
pub mod moderation;
pub mod notebook;
pub mod openapi;
#[cfg(test)]
mod place_api_tests;
pub mod privacy;
pub mod profiles;
pub mod project;
pub mod prompts;
#[cfg(test)]
mod prompts_api_tests;
pub mod publications;
pub mod readers;
pub mod scenes;
pub mod search;
pub mod share;
pub mod status;
pub mod summaries;
pub mod support;
pub mod tasks;
pub mod templates;
pub mod versions;
pub mod words;
pub mod works;

use axum::Router;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::state::AppState;

/// One list, two outputs: the axum routes and the OpenAPI document are built
/// from the same `#[utoipa::path]` annotations, so a handler cannot be routed
/// without being documented or documented without being routed.
fn api_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(openapi::ApiDoc::openapi())
        .routes(routes!(health))
        .routes(routes!(status::status))
        .routes(routes!(status::metrics))
        .routes(routes!(search::search_work))
        .routes(routes!(crate::live::ticket))
        .routes(routes!(summaries::features))
        .routes(routes!(summaries::allow, summaries::withdraw))
        .routes(routes!(summaries::scene))
        .routes(routes!(summaries::chapter))
        .routes(routes!(summaries::work))
        .routes(routes!(assistant::allow, assistant::withdraw))
        .routes(routes!(assistant::assist))
        .routes(routes!(words::lookup))
        .routes(routes!(words::list))
        .routes(routes!(words::add, words::remove))
        .routes(routes!(annotations::list_draft, annotations::create_draft))
        .routes(routes!(
            annotations::list_publication,
            annotations::create_publication
        ))
        .routes(routes!(annotations::update, annotations::delete_annotation))
        .routes(routes!(share::make))
        .routes(routes!(share::revoke))
        // A WebSocket, so not in the OpenAPI document; see docs/API.md.
        .route("/api/live", axum::routing::get(crate::live::connect))
        .routes(routes!(auth::register))
        .routes(routes!(auth::login))
        .routes(routes!(auth::refresh))
        .routes(routes!(auth::logout))
        .routes(routes!(auth::me))
        .routes(routes!(goals::writing))
        .routes(routes!(goals::update_goals))
        .routes(routes!(templates::list))
        .routes(routes!(templates::get_one))
        .routes(routes!(works::list, works::create))
        .routes(routes!(works::get_one, works::update, works::delete_work))
        .routes(routes!(works::outline))
        .routes(routes!(works::put_place))
        .routes(routes!(notebook::templates))
        .routes(routes!(notebook::list, notebook::create))
        .routes(routes!(notebook::update, notebook::delete_entry))
        .routes(routes!(notebook::link))
        .routes(routes!(notebook::unlink))
        .routes(routes!(works::create_chapter))
        .routes(routes!(works::reorder_chapters))
        .routes(routes!(works::update_chapter, works::delete_chapter))
        .routes(routes!(scenes::create))
        .routes(routes!(scenes::reorder))
        .routes(routes!(
            scenes::get_one,
            scenes::update,
            scenes::delete_scene
        ))
        .routes(routes!(scenes::put_content))
        .routes(routes!(scenes::split_scene))
        .routes(routes!(scenes::merge_scene))
        .routes(routes!(scenes::list_revisions))
        .routes(routes!(scenes::get_revision))
        .routes(routes!(scenes::restore_revision))
        .routes(routes!(changes::list, changes::create))
        .routes(routes!(changes::accept))
        .routes(routes!(changes::resolve_all))
        .routes(routes!(changes::reject))
        .routes(routes!(versions::list, versions::create))
        .routes(routes!(versions::diff))
        .routes(routes!(versions::get_one, versions::update))
        .routes(routes!(versions::add_tag))
        .routes(routes!(versions::remove_tag))
        .routes(routes!(versions::pin))
        .routes(routes!(versions::restore))
        .routes(routes!(versions::content))
        .routes(routes!(export::export_version, export::export_version_with))
        .routes(routes!(export::export_draft, export::export_draft_with))
        .routes(routes!(export::preflight_draft))
        .routes(routes!(export::preflight_version))
        .routes(routes!(
            export_settings::list_profiles,
            export_settings::create_profile
        ))
        .routes(routes!(export_settings::import_profile))
        .routes(routes!(
            export_settings::update_profile,
            export_settings::delete_profile
        ))
        .routes(routes!(export_settings::last_used))
        .routes(routes!(export_settings::export_again))
        .routes(routes!(export_settings::history))
        .routes(routes!(export_settings::download_again))
        .routes(routes!(export_settings::bundle_draft))
        .routes(routes!(export_settings::bundle_version))
        .routes(routes!(export_settings::preview_draft))
        .routes(routes!(export_settings::preview_version))
        // Whole files in the request body, so a limit of their own.
        .merge(
            OpenApiRouter::new()
                .routes(routes!(project::import_work))
                .routes(routes!(project::import_document))
                .layer(axum::extract::DefaultBodyLimit::max(
                    project::MAX_IMPORT_BYTES,
                )),
        )
        .routes(routes!(
            book_details::get_metadata,
            book_details::update_metadata
        ))
        .routes(routes!(book_details::get_cover, book_details::delete_cover))
        // A cover is an image in the request body: a limit of its own.
        .merge(
            OpenApiRouter::new()
                .routes(routes!(book_details::put_cover))
                .layer(axum::extract::DefaultBodyLimit::max(
                    book_details::MAX_COVER_BYTES + 1024,
                )),
        )
        .routes(routes!(versions::publish))
        .routes(routes!(versions::finalize))
        .routes(routes!(publications::create))
        .routes(routes!(publications::feed))
        .routes(routes!(readers::list_invites, readers::invite))
        .routes(routes!(readers::uninvite))
        .routes(routes!(readers::list_circles, readers::create_circle))
        .routes(routes!(readers::delete_circle))
        .routes(routes!(readers::add_member))
        .routes(routes!(readers::remove_member))
        .routes(routes!(readers::record_progress, readers::list_progress))
        .routes(routes!(
            publications::get_one,
            publications::update,
            publications::delete_publication
        ))
        .routes(routes!(publications::content))
        .routes(routes!(comments::list, comments::create))
        .routes(routes!(comments::update, comments::delete_comment))
        .routes(routes!(comments::vote))
        .routes(routes!(feedback::list_reactions, feedback::react))
        .routes(routes!(feedback::list_questions, feedback::create_question))
        .routes(routes!(
            feedback::update_question,
            feedback::delete_question
        ))
        .routes(routes!(feedback::answer))
        .routes(routes!(feedback::list_answers))
        .routes(routes!(feedback::standing))
        .routes(routes!(comments::bulk_state))
        .routes(routes!(export::export_feedback))
        .routes(routes!(versions::since))
        .routes(routes!(tasks::list, tasks::create))
        .routes(routes!(tasks::from_comment))
        .routes(routes!(tasks::update, tasks::delete_task))
        .routes(routes!(moderation::report))
        .routes(routes!(moderation::list_reports))
        .routes(routes!(moderation::list_blocks, moderation::block))
        .routes(routes!(moderation::unblock))
        .routes(routes!(profiles::profile))
        .routes(routes!(profiles::update_profile))
        .routes(routes!(profiles::follow, profiles::unfollow))
        .routes(routes!(profiles::following))
        .routes(routes!(collaborators::list, collaborators::add))
        .routes(routes!(collaborators::update, collaborators::remove))
        .routes(routes!(collaborators::activity))
        .routes(routes!(account::resend_verification))
        .routes(routes!(account::verify))
        .routes(routes!(account::forgot))
        .routes(routes!(account::reset))
        .routes(routes!(account::change_password))
        .routes(routes!(account::setup_two_factor))
        .routes(routes!(account::enable_two_factor))
        .routes(routes!(account::disable_two_factor))
        .routes(routes!(account::list_sessions))
        .routes(routes!(account::revoke_session))
        .routes(routes!(account::sign_out_others))
        .routes(routes!(audit::list))
        .routes(routes!(privacy::terms))
        .routes(routes!(privacy::accept_terms))
        .routes(routes!(privacy::export))
        .routes(routes!(privacy::delete_account))
        .routes(routes!(account::update_settings))
        // Daily Prompts. The literal paths before `{day}`, so they are not
        // taken for a day.
        .routes(routes!(prompts::today_prompt))
        .routes(routes!(prompts::archive))
        .routes(routes!(
            prompts::get_enrollment,
            prompts::join,
            prompts::update_enrollment,
            prompts::leave
        ))
        .routes(routes!(prompts::unsubscribe))
        .routes(routes!(prompts::one_day))
        .routes(routes!(
            prompts::get_entry,
            prompts::save_entry,
            prompts::delete_entry
        ))
        .routes(routes!(prompts::submit))
        .routes(routes!(prompts::withdraw))
        .routes(routes!(prompts::feed))
        .routes(routes!(prompts::get_one))
        .routes(routes!(prompts::to_work_preview))
        .routes(routes!(prompts::to_work))
        .routes(routes!(critiques::list, critiques::create))
        .routes(routes!(critiques::update, critiques::delete_critique))
        .routes(routes!(critiques::report))
        .routes(routes!(critiques::list_reports))
        .routes(routes!(inbox::list))
        .routes(routes!(inbox::unread_count))
        .routes(routes!(inbox::read_one))
        .routes(routes!(inbox::read_digest))
        .routes(routes!(inbox::read_all))
        .routes(routes!(admin::list_prompts, admin::create_prompt))
        .routes(routes!(admin::reorder_prompts))
        .routes(routes!(admin::update_prompt, admin::delete_prompt))
        .routes(routes!(admin::jobs))
}

pub fn router(state: AppState) -> Router {
    let test_clock = std::env::var("JUSTTYPE_TEST_CLOCK").as_deref() == Ok("1");
    let app_id = std::env::var("JUSTTYPE_APPLE_APP_ID").ok();
    let (mut router, api) = api_router().with_state(state.clone()).split_for_parts();
    if test_clock {
        tracing::warn!("JUSTTYPE_TEST_CLOCK is on: anyone can move this server's clock");
        router = router.merge(
            Router::new()
                .route(
                    "/api/test/clock",
                    axum::routing::post(admin::set_test_clock),
                )
                .with_state(state),
        );
    }
    // Served so a client generator can point straight at a running instance;
    // the checked-in docs/openapi.json is the same document.
    router
        .route(
            "/.well-known/apple-app-site-association",
            axum::routing::get(move || {
                let app_id = app_id.clone();
                async move { crate::web::app_site_association(app_id.as_deref()) }
            }),
        )
        .route(
            "/api/openapi.json",
            axum::routing::get(move || async move { axum::Json(api) }),
        )
}

/// The document on its own, for `cargo xtask openapi`. Needs no state: nothing
/// about the schema depends on a database or a data directory.
pub fn openapi_document() -> utoipa::openapi::OpenApi {
    api_router().into_openapi()
}

#[utoipa::path(
    get,
    path = "/api/health",
    tag = "health",
    responses((status = 200, description = "The service is up", body = HealthResponse))
)]
async fn health() -> axum::Json<HealthResponse> {
    axum::Json(HealthResponse {
        status: "ok".into(),
        service: env!("CARGO_PKG_NAME").into(),
        version: crate::app_version().into(),
    })
}

#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub service: String,
    pub version: String,
}
