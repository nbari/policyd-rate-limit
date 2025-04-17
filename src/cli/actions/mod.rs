pub mod run;

use secrecy::SecretString;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Action {
    Run {
        dsn: SecretString,
        pool: u32,
        socket: PathBuf,
        limit: i32,
        rate: i32,
    },
}
