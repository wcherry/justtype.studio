//! The OpenAPI document, assembled from the same annotations that define the
//! routes. `cargo xtask openapi` writes it to `docs/openapi.json`, and the
//! client types are generated from that file — so the schema cannot drift from
//! the handlers without the build noticing.
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Scriptorium",
        version = env!("CARGO_PKG_VERSION"),
        description = "A writing tool for long-form fiction, and a review surface for readers."
    ),
    tags(
        (name = "auth", description = "Accounts and sessions"),
        (name = "templates", description = "Structural skeletons for new works"),
        (name = "works", description = "Novels, novellas, short stories, serials"),
        (name = "chapters", description = "Ordered divisions of a work"),
        (name = "scenes", description = "The unit of writing"),
        (name = "changes", description = "Proposed edits, accepted or rejected"),
        (name = "versions", description = "Immutable snapshots"),
        (name = "publications", description = "A version offered for review"),
        (name = "comments", description = "Anchored, threaded, votable feedback"),
        (name = "profiles", description = "Authors, and who follows them"),
    )
)]
pub struct ApiDoc;
