//! Makes cargo notice when a migration changes.
//!
//! `embed_migrations!` bakes the SQL into the binary at compile time, but cargo
//! does not track the files it read unless told to. Without this, editing a
//! migration and running the tests recompiles nothing: the old SQL is still
//! embedded, the tests pass against it, and the change appears to be fine
//! right up until it runs somewhere real.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
    // Stamped in by the Docker build; see `app_version`.
    println!("cargo:rerun-if-env-changed=SCRIPTORIUM_VERSION");
}
