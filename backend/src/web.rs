//! Serving the built web app from the same process as the API.
//!
//! Off unless `SCRIPTORIUM_WEB_DIR` names a directory holding a Vite build
//! (`web/dist`): in development Vite serves the app and proxies `/api` here.
//! In the Docker image it is on, so one container is the whole service.
//!
//! Two rules, wanting opposite caching:
//!
//! - `/assets/…` is Vite's content-hashed output. It is cached for a year and
//!   a miss is a 404 — never `index.html`, which would be cached for a year
//!   under a URL that will never hold anything else, and would fail as a
//!   script in a confusing way.
//! - Everything else is a file from the build's root (the icon, say) or,
//!   failing that, `index.html`, so a reload on `/review/…` opens the app at
//!   that route. These are revalidated every time, or a deploy would never
//!   reach a returning visitor.
//!
//! `/api/…` never falls through to the app: an unknown API path is a 404.
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};

const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const REVALIDATE: &str = "no-cache";

/// Whether `dir` holds a build to serve: the index is what every route falls
/// back to, so a directory without one is not a build.
pub fn present(dir: &Path) -> bool {
    dir.join("index.html").is_file()
}

pub fn mount(router: Router, dir: PathBuf) -> Router {
    let assets = dir.clone();
    router
        .route(
            "/assets/{*path}",
            axum::routing::get(move |request: Request| asset(assets.clone(), request)),
        )
        .fallback(move |request: Request| page(dir.clone(), request))
}

async fn asset(dir: PathBuf, request: Request) -> Response {
    // Rooted at the build, so `/assets/x.js` is `dir/assets/x.js`; no fallback.
    let response = ServeDir::new(dir).oneshot(request).await;
    with_cache(response.into_response(), IMMUTABLE)
}

async fn page(dir: PathBuf, request: Request) -> Response {
    if request.uri().path() == "/api" || request.uri().path().starts_with("/api/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let index = dir.join("index.html");
    let response = ServeDir::new(dir)
        .fallback(ServeFile::new(index))
        .oneshot(request)
        .await;
    with_cache(response.into_response(), REVALIDATE)
}

fn with_cache(mut response: Response<Body>, policy: &'static str) -> Response {
    if response.status().is_success() {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static(policy));
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn get(app: Router, path: &str) -> (StatusCode, String, Option<String>) {
        let response = app
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let cache = response
            .headers()
            .get(header::CACHE_CONTROL)
            .map(|v| v.to_str().unwrap().to_string());
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&body).into_owned(), cache)
    }

    #[tokio::test]
    async fn routes_get_the_app_assets_are_cached_and_the_api_is_left_alone() {
        let dir = std::env::temp_dir().join(format!("scriptorium-web-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), "<!doctype html>app").unwrap();
        std::fs::write(dir.join("assets/app-1a2b.js"), "console.log(1)").unwrap();
        std::fs::write(dir.join("favicon.svg"), "<svg/>").unwrap();
        assert!(present(&dir));

        let api = Router::new().route("/api/health", axum::routing::get(|| async { "ok" }));
        let app = mount(api, dir.clone());

        let (status, body, cache) = get(app.clone(), "/review/abc").await;
        assert_eq!(
            (status, body.as_str()),
            (StatusCode::OK, "<!doctype html>app")
        );
        assert_eq!(cache.as_deref(), Some(REVALIDATE));

        let (status, body, cache) = get(app.clone(), "/assets/app-1a2b.js").await;
        assert_eq!((status, body.as_str()), (StatusCode::OK, "console.log(1)"));
        assert_eq!(cache.as_deref(), Some(IMMUTABLE));

        let (status, body, _) = get(app.clone(), "/favicon.svg").await;
        assert_eq!((status, body.as_str()), (StatusCode::OK, "<svg/>"));

        let (status, _, cache) = get(app.clone(), "/assets/gone-9z.js").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(cache, None);

        assert_eq!(
            get(app.clone(), "/api/nothing").await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(get(app.clone(), "/api/health").await.1, "ok");
        assert_eq!(get(app, "/../../etc/passwd").await.1, "<!doctype html>app");

        std::fs::remove_dir_all(dir).unwrap();
    }
}
