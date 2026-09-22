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
    if let Some(parent) = std::path::Path::new(database_url).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let manager = ConnectionManager::<SqliteConnection>::new(database_url);
    let pool = R2d2Pool::builder()
        .max_size(8)
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
    tokio::task::spawn_blocking(move || {
        let mut conn = pool.get().map_err(|e| ApiError::internal(e.to_string()))?;
        f(&mut conn)
    })
    .await
    .map_err(|e| ApiError::internal(format!("task panicked: {e}")))?
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
}
