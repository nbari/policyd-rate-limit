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
        let result = sqlx::query(
            "UPDATE ratelimit SET used = 0, rdate = NOW()
             WHERE rate < EXTRACT(EPOCH FROM NOW() - rdate) AND username = $1",
        )
        .bind(username)
        .execute(&*self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}
