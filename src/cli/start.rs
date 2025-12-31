use anyhow::Result;

use crate::cli::{actions::Action, commands, dispatch::handler, telemetry};
/// Start the CLI.
///
/// # Errors
/// Returns an error if telemetry initialization or argument dispatch fails.
pub fn start() -> Result<Action> {
    let matches = commands::new().get_matches();

    let verbosity_level = match matches.get_count("verbose") {
        0 => None,
        1 => Some(tracing::Level::INFO),
        2 => Some(tracing::Level::DEBUG),
        _ => Some(tracing::Level::TRACE),
    };

    telemetry::init(verbosity_level)?;

    let action = handler(&matches)?;

    Ok(action)
}
