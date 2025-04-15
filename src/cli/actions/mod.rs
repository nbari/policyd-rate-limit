pub mod run;

use secrecy::SecretString;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Action {
    Run { dsn: SecretString, socket: PathBuf },
}
