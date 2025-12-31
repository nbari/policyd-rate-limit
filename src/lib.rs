#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateLimit {
    pub limit: i32,
    pub rate: i32,
}

pub mod cli;
pub mod queries;
