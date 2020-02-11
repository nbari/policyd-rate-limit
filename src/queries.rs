use std::error::Error;

pub struct Queries {
    pub pool: mysql::Pool,
}

pub fn new(pool: mysql::Pool) -> Queries {
    return Queries { pool: pool };
}

impl Queries {
    pub fn get_user(&self, username: &str) -> Result<usize, Box<dyn Error>> {
        let row = self
            .pool
            .prep_exec(
                "SELECT IF(quota > used, 1, 0) FROM ratelimit WHERE email=?",
                (&username,),
            )?
            .last()
            .ok_or("expecting a row")??;
        Ok(mysql::from_row_opt::<usize>(row)?)
    }

    pub fn create_user(
        &self,
        username: &str,
        limit: usize,
        rate: usize,
    ) -> Result<(), Box<dyn Error>> {
        let _ = self
            .pool
            .prepare("INSERT INTO ratelimit (email, quota, expiry) VALUES (?, ?, ?)")?
            .execute((username, limit, rate))?;
        Ok(())
    }
}
