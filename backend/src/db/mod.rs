pub mod models;
pub mod schema;

use diesel::connection::SimpleConnection;
use diesel::r2d2::{ConnectionManager, CustomizeConnection, Pool as R2d2Pool, PooledConnection};
use diesel::SqliteConnection;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};

use crate::error::{ApiError, ApiResult};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

pub type Pool = R2d2Pool<ConnectionManager<SqliteConnection>>;
pub type Conn = PooledConnection<ConnectionManager<SqliteConnection>>;

/// SQLite needs telling, per connection, that we mean it about foreign keys and
/// about not serialising every reader behind the writer.
#[derive(Debug)]
struct SqlitePragmas;

impl CustomizeConnection<SqliteConnection, diesel::r2d2::Error> for SqlitePragmas {
    fn on_acquire(&self, conn: &mut SqliteConnection) -> Result<(), diesel::r2d2::Error> {
        conn.batch_execute(
            "PRAGMA journal_mode = WAL;\
             PRAGMA synchronous = NORMAL;\
             PRAGMA foreign_keys = ON;\
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(diesel::r2d2::Error::QueryError)
    }
}

pub fn build_pool(database_url: &str) -> anyhow::Result<Pool> {
    build_pool_of(database_url, 8)
}

/// A pool of a given size. One, for maintenance on a file nothing else has
/// open: eight connections each switching a fresh file to WAL at once is a
/// burst of lock errors that mean nothing.
pub fn build_pool_of(database_url: &str, size: u32) -> anyhow::Result<Pool> {
    if let Some(parent) = std::path::Path::new(database_url).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let manager = ConnectionManager::<SqliteConnection>::new(database_url);
    let pool = R2d2Pool::builder()
        .max_size(size)
        .connection_customizer(Box::new(SqlitePragmas))
        .build(manager)?;
    Ok(pool)
}

pub fn run_migrations(pool: &Pool) -> anyhow::Result<()> {
    let mut conn = pool.get()?;
    conn.run_pending_migrations(MIGRATIONS)
        .map_err(|e| anyhow::anyhow!("migration failed: {e}"))?;
    Ok(())
}

/// Diesel is synchronous, so every query hops onto the blocking pool. Handlers
/// call this instead of touching the connection pool directly.
pub async fn blocking<F, T>(pool: &Pool, f: F) -> ApiResult<T>
where
    F: FnOnce(&mut Conn) -> ApiResult<T> + Send + 'static,
    T: Send + 'static,
{
    let pool = pool.clone();
    // Read here, on the request's task: the blocking pool's threads do not
    // carry task-locals.
    let writing = crate::metrics::writing();
    let started = std::time::Instant::now();
    let result = tokio::task::spawn_blocking(move || {
        let mut conn = pool.get().map_err(|e| ApiError::internal(e.to_string()))?;
        f(&mut conn)
    })
    .await
    .map_err(|e| ApiError::internal(format!("task panicked: {e}")))?;
    if writing {
        let metrics = crate::metrics::global();
        // Waiting for a pooled connection counts: it is time a writer spent.
        metrics.record_db_write(started.elapsed());
        if matches!(&result, Err(ApiError::Internal(m)) if m.contains("database is locked")) {
            metrics.record_busy();
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use diesel::prelude::*;

    /// Migrations plus seeding on a throwaway database. This is the check that
    /// `schema.rs` and the SQL have not drifted apart: Diesel's generated
    /// queries fail loudly against a real table definition.
    #[test]
    fn migrations_and_seed_apply_cleanly() {
        let dir = std::env::temp_dir().join(format!("scriptorium-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.db");

        let pool = build_pool(path.to_str().unwrap()).unwrap();
        run_migrations(&pool).unwrap();

        let mut conn = pool.get().unwrap();
        crate::domain::templates::seed(&mut conn, &crate::domain::now_iso()).unwrap();
        // Seeding twice must be a no-op, because it runs on every boot.
        crate::domain::templates::seed(&mut conn, &crate::domain::now_iso()).unwrap();

        let count: i64 = schema::templates::table
            .count()
            .get_result(&mut conn)
            .unwrap();
        assert_eq!(count, 5);

        let foreign_keys: i32 =
            diesel::dsl::sql::<diesel::sql_types::Integer>("PRAGMA foreign_keys")
                .get_result(&mut conn)
                .unwrap();
        assert_eq!(
            foreign_keys, 1,
            "foreign keys must be on for every connection"
        );

        drop(conn);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Every `down.sql`, one at a time.
    ///
    /// Rollbacks are the one part of the schema that gets written and never
    /// executed, so they rot silently: a `DROP COLUMN` SQLite refuses, tables
    /// dropped in the wrong order against a foreign key, a `down.sql` that
    /// simply does not match its `up.sql`.
    ///
    /// Each migration is reverted and immediately re-applied, rather than
    /// reverting the whole stack at once. That distinction matters: the first
    /// migration drops every table, so a full rollback would mask a later
    /// `down.sql` that failed to remove its own column — the table it lived on
    /// disappears regardless. Going one step at a time, a rollback that did not
    /// undo its migration fails on re-apply with a duplicate column.
    #[test]
    fn every_migration_reverts_and_re_applies_on_its_own() {
        let dir =
            std::env::temp_dir().join(format!("scriptorium-rollback-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.db");

        let pool = build_pool(path.to_str().unwrap()).unwrap();
        run_migrations(&pool).unwrap();

        let mut conn = pool.get().unwrap();
        let applied = conn
            .applied_migrations()
            .expect("could not read applied migrations");
        assert!(
            applied.len() > 1,
            "expected several migrations, found {}",
            applied.len()
        );

        // Peel the stack one deeper each time, then put it all back.
        //
        // Reverting the whole stack in one go and re-applying it would prove
        // much less than it appears to: the first migration drops every table,
        // so a later `down.sql` that failed to remove its own column is
        // invisible — the table it lived on is dropped regardless. Peeling
        // progressively means each migration's `down.sql` is followed by its
        // own `up.sql`, and one that did not undo its migration fails to
        // re-apply with a duplicate column.
        for depth in 1..=applied.len() {
            for step in 0..depth {
                conn.revert_last_migration(MIGRATIONS).unwrap_or_else(|e| {
                    panic!("a down.sql failed {step} step(s) into a {depth}-deep rollback: {e}")
                });
            }
            conn.run_pending_migrations(MIGRATIONS).unwrap_or_else(|e| {
                panic!(
                    "could not re-apply after rolling back {depth} migration(s) — \
                     a down.sql does not undo its own up.sql: {e}"
                )
            });
            assert_eq!(
                conn.applied_migrations().unwrap().len(),
                applied.len(),
                "the stack did not come back to full depth after a {depth}-deep rollback"
            );
        }

        // And once more all the way down, which is what an operator rolling a
        // release back actually does.
        for _ in 0..applied.len() {
            conn.revert_last_migration(MIGRATIONS)
                .expect("full rollback failed");
        }

        // Everything is off now; put it all back and check the result is the
        // schema the application actually expects.
        assert!(
            conn.applied_migrations().unwrap().is_empty(),
            "something was left applied after reverting the whole stack"
        );
        conn.run_pending_migrations(MIGRATIONS)
            .expect("migrations failed to re-apply from a bare database");

        crate::domain::templates::seed(&mut conn, &crate::domain::now_iso()).unwrap();
        assert_eq!(
            schema::templates::table
                .count()
                .get_result::<i64>(&mut conn)
                .unwrap(),
            5
        );
        // Columns and tables the later migrations added — the ones a bad
        // rollback would have left in the wrong shape.
        for count in [
            schema::version_tags::table
                .count()
                .get_result::<i64>(&mut conn),
            schema::bible_entries::table
                .count()
                .get_result::<i64>(&mut conn),
            schema::writing_days::table
                .count()
                .get_result::<i64>(&mut conn),
        ] {
            assert_eq!(count.unwrap(), 0);
        }
        assert_eq!(
            schema::users::table
                .filter(schema::users::daily_word_goal.is_not_null())
                .count()
                .get_result::<i64>(&mut conn)
                .unwrap(),
            0
        );

        drop(conn);
        std::fs::remove_dir_all(&dir).ok();
    }
}
