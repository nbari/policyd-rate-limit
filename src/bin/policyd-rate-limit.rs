use anyhow::Result;

use policyd_rate_limit::cli::{actions, actions::Action, start};

#[tokio::main]
async fn main() -> Result<()> {
    // Start the agent
    let action = start()?;

    match action {
        Action::Run { .. } => actions::run::handle(action).await?,
    }

    Ok(())
}
