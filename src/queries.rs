use std::error::Error;

pub struct Queries {
    pub pool: mysql::Pool,
}

#[must_use]
pub const fn new(pool: mysql::Pool) -> Queries {
    Queries { pool }
}

impl Queries {
    /// # Errors
    /// will return Err if no row is found
    pub fn get_user(&self, username: &str) -> Result<bool, Box<dyn Error>> {
        let row = self
            .pool
            .prep_exec(
                "SELECT IF(quota > used, true, false) FROM ratelimit WHERE username=?",
                (&username,),
            )?
            .last()
            .ok_or("expecting a row")??;
        Ok(mysql::from_row_opt::<bool>(row)?)
    }

    /// # Errors
    /// will return Err if cannot create the user
    pub fn create_user(
        &self,
        username: &str,
        limit: usize,
        rate: usize,
    ) -> Result<(), Box<dyn Error>> {
        self.pool
            .prepare("INSERT INTO ratelimit (username, quota, rate) VALUES (?, ?, ?)")?
            .execute((&username, limit, rate))?;
        Ok(())
    }

    /// # Errors
    /// will return Err if cannot update the used quota
    pub fn update_quota(&self, username: &str) -> Result<(), Box<dyn Error>> {
        self.pool.prep_exec(
            "UPDATE ratelimit SET used = used + 1 WHERE username=?",
            (&username,),
        )?;
        Ok(())
    }

    /// # Errors
    /// will return Err if could not reset the quota
    pub fn reset_quota(&self, username: &str) -> Result<u64, Box<dyn Error>> {
        let rs = self.pool.prep_exec(
            "UPDATE ratelimit SET used=0, rdate=NOW() WHERE rate < TIMESTAMPDIFF(SECOND, rdate, NOW()) AND username=?",
            (&username,),
        )?;
        Ok(rs.affected_rows())
    }
}
