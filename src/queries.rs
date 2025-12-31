use std::sync::Arc;

use sqlx::AnyPool;

use crate::RateLimit;
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct RateLimitWindow {
    pub rate: i32,
    pub quota: i32,
    pub used: i32,
}

#[derive(Clone)]
pub struct Queries {
    pool: Arc<AnyPool>,
}

impl Queries {
    /// Create a new query helper backed by the provided pool.
    #[must_use]
    pub fn new(pool: AnyPool) -> Self {
        Self {
            pool: Arc::new(pool),
        }
    }

    fn is_postgres(&self) -> bool {
        self.pool
            .as_ref()
            .connect_options()
            .database_url
            .scheme()
            .starts_with("postgres")
    }

    fn is_sqlite(&self) -> bool {
        self.pool
            .as_ref()
            .connect_options()
            .database_url
            .scheme()
            .starts_with("sqlite")
    }

    /// Fetch rate limit windows for a user.
    ///
    /// # Errors
    /// Returns an error if the database query fails.
    pub async fn get_windows(&self, username: &str) -> sqlx::Result<Vec<RateLimitWindow>> {
        let query = if self.is_postgres() {
            "SELECT rate, quota, used FROM ratelimit WHERE username = $1 ORDER BY rate"
        } else {
            "SELECT rate, quota, used FROM ratelimit WHERE username = ? ORDER BY rate"
        };

        sqlx::query_as(query)
            .bind(username)
            .fetch_all(&*self.pool)
            .await
    }

    /// Check whether all windows for the user are within quota.
    ///
    /// # Errors
    /// Returns an error if the database query fails.
    pub async fn is_within_quota(&self, username: &str) -> sqlx::Result<Option<bool>> {
        let windows = self.get_windows(username).await?;
        if windows.is_empty() {
            return Ok(None);
        }

        Ok(Some(
            windows.iter().all(|window| window.used < window.quota),
        ))
    }

    /// Insert new user windows with the provided limits and rates.
    ///
    /// # Errors
    /// Returns an error if the database insert fails.
    pub async fn create_user(&self, username: &str, windows: &[RateLimit]) -> sqlx::Result<()> {
        let query = if self.is_postgres() {
            "INSERT INTO ratelimit (username, quota, rate) VALUES ($1, $2, $3)"
        } else {
            "INSERT INTO ratelimit (username, quota, rate) VALUES (?, ?, ?)"
        };

        let mut tx = self.pool.begin().await?;
        for window in windows {
            sqlx::query(query)
                .bind(username)
                .bind(window.limit)
                .bind(window.rate)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Ensure windows exist for a user without overwriting existing rows.
    ///
    /// # Errors
    /// Returns an error if the database insert fails.
    pub async fn ensure_windows(&self, username: &str, windows: &[RateLimit]) -> sqlx::Result<()> {
        let query = if self.is_postgres() {
            "INSERT INTO ratelimit (username, quota, rate) VALUES ($1, $2, $3)
             ON CONFLICT (username, rate) DO NOTHING"
        } else if self.is_sqlite() {
            "INSERT OR IGNORE INTO ratelimit (username, quota, rate) VALUES (?, ?, ?)"
        } else {
            "INSERT IGNORE INTO ratelimit (username, quota, rate) VALUES (?, ?, ?)"
        };

        let mut tx = self.pool.begin().await?;
        for window in windows {
            sqlx::query(query)
                .bind(username)
                .bind(window.limit)
                .bind(window.rate)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Increment the usage counter for a user.
    ///
    /// # Errors
    /// Returns an error if the database update fails.
    pub async fn update_quota(&self, username: &str) -> sqlx::Result<()> {
        let query = if self.is_postgres() {
            "UPDATE ratelimit SET used = used + 1 WHERE username = $1"
        } else {
            "UPDATE ratelimit SET used = used + 1 WHERE username = ?"
        };

        sqlx::query(query)
            .bind(username)
            .execute(&*self.pool)
            .await?;
        Ok(())
    }

    /// Reset quotas for all expired windows for a user.
    ///
    /// # Errors
    /// Returns an error if the database update fails.
    pub async fn reset_quotas_if_expired(&self, username: &str) -> sqlx::Result<bool> {
        let rows_affected = if self.is_postgres() {
            sqlx::query(
                "WITH now_val AS (
                        SELECT NOW() AS now_time
                    )
                    UPDATE ratelimit
                    SET used = 0, rdate = (SELECT now_time FROM now_val)
                    WHERE username = $1
                    AND rate < EXTRACT(EPOCH FROM (SELECT now_time FROM now_val) - rdate)",
            )
            .bind(username)
            .execute(&*self.pool)
            .await?
            .rows_affected()
        } else if self.is_sqlite() {
            sqlx::query(
                "UPDATE ratelimit
                    SET used = 0, rdate = CURRENT_TIMESTAMP
                    WHERE username = ?
                    AND rate < (strftime('%s','now') - strftime('%s', rdate))",
            )
            .bind(username)
            .execute(&*self.pool)
            .await?
            .rows_affected()
        } else {
            sqlx::query(
                "UPDATE ratelimit
                    SET used = 0, rdate = NOW()
                    WHERE username = ?
                    AND rate < TIMESTAMPDIFF(SECOND, rdate, NOW())",
            )
            .bind(username)
            .execute(&*self.pool)
            .await?
            .rows_affected()
        };

        Ok(rows_affected > 0)
    }
}
