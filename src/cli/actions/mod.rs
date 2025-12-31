pub mod run;

use std::path::PathBuf;

use secrecy::SecretString;

use crate::RateLimit;
#[derive(Debug)]
pub enum Action {
    Run {
        dsn: SecretString,
        pool: u32,
        socket: PathBuf,
        windows: Vec<RateLimit>,
    },
}
