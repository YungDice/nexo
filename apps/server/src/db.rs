//! Postgres connection pool.

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// Builds a connection pool from `DATABASE_URL`, connecting eagerly so a bad
/// connection string or an unreachable database fails fast at startup rather
/// than on the first request.
pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn users_table_is_reachable_after_migration() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL not set (needs a running local Postgres)");
            return;
        };
        let pool = create_pool(&database_url)
            .await
            .expect("connect to test database — is `docker compose up -d` running?");
        let row = sqlx::query!("SELECT count(*) AS count FROM users")
            .fetch_one(&pool)
            .await
            .expect("users table exists and is queryable — did you run `sqlx migrate run`?");
        assert!(row.count.unwrap_or(0) >= 0);
    }
}
