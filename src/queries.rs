use sqlx::AnyPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct Queries {
    pool: Arc<AnyPool>,
}

impl Queries {
    pub fn new(pool: AnyPool) -> Self {
        Self {
            pool: Arc::new(pool),
        }
    }

    pub async fn get_user(&self, username: &str) -> sqlx::Result<Option<bool>> {
        let result: Option<(bool,)> =
            sqlx::query_as("SELECT quota > used FROM ratelimit WHERE username = $1")
                .bind(username)
                .fetch_optional(&*self.pool)
                .await?;

        Ok(result.map(|r| r.0))
    }

    pub async fn create_user(&self, username: &str, limit: i32, rate: i32) -> sqlx::Result<()> {
        sqlx::query("INSERT INTO ratelimit (username, quota, rate) VALUES ($1, $2, $3)")
            .bind(username)
            .bind(limit)
            .bind(rate)
            .execute(&*self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_quota(&self, username: &str) -> sqlx::Result<()> {
        sqlx::query("UPDATE ratelimit SET used = used + 1 WHERE username = $1")
            .bind(username)
            .execute(&*self.pool)
            .await?;
        Ok(())
    }

    pub async fn reset_quota_if_expired(&self, username: &str) -> sqlx::Result<bool> {
        // Get the connection info to determine the database type
        let db_url = &self.pool.as_ref().connect_options().database_url;

        // CTE to get the current time and use it to reset the quota - postgres
        if db_url.scheme().starts_with("postgres") {
            let row = sqlx::query_scalar::<_, i64>(
                "WITH now_val AS (
                        SELECT NOW() AS now_time
                    )
                    UPDATE ratelimit
                    SET used = 0, rdate = (SELECT now_time FROM now_val)
                    WHERE username = $1
                    AND rate < EXTRACT(EPOCH FROM (SELECT now_time FROM now_val) - rdate)
                    RETURNING 1",
            )
            .bind(username)
            .fetch_optional(&*self.pool)
            .await?;
            Ok(row.is_some())
        } else {
            let row = sqlx::query_scalar::<_, i64>(
                "UPDATE ratelimit SET used = 0, rdate = NOW()
                    WHERE rate < EXTRACT(EPOCH FROM NOW() - rdate) AND username = $1
                    RETURNING 1",
            )
            .bind(username)
            .fetch_optional(&*self.pool)
            .await?;

            Ok(row.is_some())
        }
    }
}
