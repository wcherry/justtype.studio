pub mod account;
pub mod annotations;
pub mod audit;
pub mod auth;
pub mod bible;
pub mod changes;
pub mod collaborators;
pub mod comments;
pub mod discovery;
pub mod export;
pub mod feedback;
pub mod goals;
pub mod moderation;
pub mod openapi;
pub mod privacy;
pub mod profiles;
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
        .routes(routes!(bible::templates))
        .routes(routes!(bible::list, bible::create))
        .routes(routes!(bible::update, bible::delete_entry))
        .routes(routes!(bible::link))
        .routes(routes!(bible::unlink))
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
        .routes(routes!(export::export_version))
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
}

pub fn router(state: AppState) -> Router {
    let (router, api) = api_router().with_state(state).split_for_parts();
    // Served so a client generator can point straight at a running instance;
    // the checked-in docs/openapi.json is the same document.
    router.route(
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
