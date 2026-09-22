pub mod auth;
pub mod changes;
pub mod comments;
pub mod publications;
pub mod scenes;
pub mod support;
pub mod templates;
pub mod versions;
pub mod works;

use axum::routing::{get, patch, post, put};
use axum::Router;

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        // --- auth ---------------------------------------------------------
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/refresh", post(auth::refresh))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        // --- templates ----------------------------------------------------
        .route("/templates", get(templates::list))
        .route("/templates/{template_id}", get(templates::get_one))
        // --- works --------------------------------------------------------
        .route("/works", get(works::list).post(works::create))
        .route(
            "/works/{work_id}",
            get(works::get_one)
                .patch(works::update)
                .delete(works::delete_work),
        )
        .route("/works/{work_id}/outline", get(works::outline))
        // --- chapters -----------------------------------------------------
        .route("/works/{work_id}/chapters", post(works::create_chapter))
        .route(
            "/works/{work_id}/chapters/reorder",
            post(works::reorder_chapters),
        )
        .route(
            "/chapters/{chapter_id}",
            patch(works::update_chapter).delete(works::delete_chapter),
        )
        // --- scenes -------------------------------------------------------
        .route("/chapters/{chapter_id}/scenes", post(scenes::create))
        .route(
            "/chapters/{chapter_id}/scenes/reorder",
            post(scenes::reorder),
        )
        .route(
            "/scenes/{scene_id}",
            get(scenes::get_one)
                .patch(scenes::update)
                .delete(scenes::delete_scene),
        )
        .route("/scenes/{scene_id}/content", put(scenes::put_content))
        .route("/scenes/{scene_id}/revisions", get(scenes::list_revisions))
        .route(
            "/scenes/{scene_id}/revisions/{revision_id}",
            get(scenes::get_revision),
        )
        // --- tracked changes ----------------------------------------------
        .route(
            "/scenes/{scene_id}/changes",
            get(changes::list).post(changes::create),
        )
        .route("/changes/{change_id}/accept", post(changes::accept))
        .route("/changes/{change_id}/reject", post(changes::reject))
        // --- versions -----------------------------------------------------
        .route(
            "/works/{work_id}/versions",
            get(versions::list).post(versions::create),
        )
        .route("/works/{work_id}/versions/diff", get(versions::diff))
        .route("/versions/{version_id}", get(versions::get_one))
        .route("/versions/{version_id}/content", get(versions::content))
        .route("/versions/{version_id}/publish", post(versions::publish))
        .route("/versions/{version_id}/finalize", post(versions::finalize))
        // --- publications --------------------------------------------------
        .route(
            "/versions/{version_id}/publications",
            post(publications::create),
        )
        .route("/publications", get(publications::feed))
        .route(
            "/publications/{publication_id}",
            get(publications::get_one)
                .patch(publications::update)
                .delete(publications::delete_publication),
        )
        .route(
            "/publications/{publication_id}/content",
            get(publications::content),
        )
        // --- comments -------------------------------------------------------
        .route(
            "/publications/{publication_id}/comments",
            get(comments::list).post(comments::create),
        )
        .route(
            "/comments/{comment_id}",
            patch(comments::update).delete(comments::delete_comment),
        )
        .route("/comments/{comment_id}/vote", post(comments::vote))
        .with_state(state);

    Router::new().nest("/api", api)
}

async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "ok",
        "service": "scriptorium-backend",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
