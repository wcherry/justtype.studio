pub mod auth;
pub mod bible;
pub mod changes;
pub mod comments;
pub mod export;
pub mod goals;
pub mod openapi;
pub mod publications;
pub mod scenes;
pub mod support;
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
        .routes(routes!(versions::get_one))
        .routes(routes!(versions::content))
        .routes(routes!(export::export_version))
        .routes(routes!(versions::publish))
        .routes(routes!(versions::finalize))
        .routes(routes!(publications::create))
        .routes(routes!(publications::feed))
        .routes(routes!(
            publications::get_one,
            publications::update,
            publications::delete_publication
        ))
        .routes(routes!(publications::content))
        .routes(routes!(comments::list, comments::create))
        .routes(routes!(comments::update, comments::delete_comment))
        .routes(routes!(comments::vote))
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
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub service: String,
    pub version: String,
}
