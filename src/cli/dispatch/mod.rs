use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use secrecy::SecretString;

use crate::RateLimit;
use crate::cli::actions::Action;

/// Build an action from parsed CLI arguments.
///
/// # Errors
/// Returns an error if required arguments are missing or cannot be converted.
pub fn handler(matches: &clap::ArgMatches) -> Result<Action> {
    let socket = matches
        .get_one::<PathBuf>("socket")
        .cloned()
        .ok_or_else(|| anyhow!("socket required"))?;

    let limits: Vec<u32> = matches
        .get_many("limit")
        .map_or_else(|| vec![10], |values| values.copied().collect());
    let rates: Vec<u32> = matches
        .get_many("rate")
        .map_or_else(|| vec![86400], |values| values.copied().collect());

    if limits.len() != rates.len() {
        return Err(anyhow!("limit/rate pairs must match"));
    }

    let unique_rates: HashSet<u32> = rates.iter().copied().collect();
    if unique_rates.len() != rates.len() {
        return Err(anyhow!("rate values must be unique"));
    }

    let windows = limits
        .into_iter()
        .zip(rates)
        .map(|(limit, rate)| {
            Ok(RateLimit {
                limit: i32::try_from(limit).map_err(|_| anyhow!("limit must fit in i32"))?,
                rate: i32::try_from(rate).map_err(|_| anyhow!("rate must fit in i32"))?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(Action::Run {
        socket,
        dsn: SecretString::from(
            matches
                .get_one::<String>("dsn")
                .cloned()
                .unwrap_or_default(),
        ),
        pool: matches.get_one::<u32>("pool").copied().unwrap_or(5),
        windows,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use secrecy::ExposeSecret;

    use super::*;
    use crate::cli::commands::new;

    #[test]
    fn test_handler() -> Result<()> {
        let matches = new().try_get_matches_from(["bin", "--dsn", "", "-s", "/tmp/a.sock"]);

        let m = matches?;

        assert_eq!(
            m.get_one::<PathBuf>("socket").map(PathBuf::as_path),
            Some(Path::new("/tmp/a.sock"))
        );

        assert_eq!(m.get_one::<u8>("verbose").copied(), Some(0));

        assert_eq!(m.get_one::<String>("dsn").map(String::as_str), Some(""));

        let action = handler(&m)?;

        match action {
            Action::Run {
                socket,
                dsn,
                pool,
                windows,
            } => {
                assert_eq!(socket, Path::new("/tmp/a.sock"));
                assert_eq!(dsn.expose_secret(), "");
                assert_eq!(
                    windows,
                    vec![RateLimit {
                        limit: 10,
                        rate: 86400
                    }]
                );
                assert_eq!(pool, 5);
            }
        }

        Ok(())
    }

    #[test]
    fn test_multiple_windows() -> Result<()> {
        let matches = new().try_get_matches_from([
            "bin",
            "--dsn",
            "",
            "-s",
            "/tmp/a.sock",
            "-l",
            "7",
            "-r",
            "3600",
            "-l",
            "100",
            "-r",
            "86400",
        ]);

        let m = matches?;
        let action = handler(&m)?;

        match action {
            Action::Run { windows, .. } => {
                assert_eq!(
                    windows,
                    vec![
                        RateLimit {
                            limit: 7,
                            rate: 3600,
                        },
                        RateLimit {
                            limit: 100,
                            rate: 86400,
                        },
                    ]
                );
            }
        }

        Ok(())
    }

    #[test]
    fn test_mismatched_windows() -> Result<()> {
        let matches = new().try_get_matches_from([
            "bin",
            "--dsn",
            "",
            "-s",
            "/tmp/a.sock",
            "-l",
            "7",
            "-l",
            "100",
            "-r",
            "3600",
        ]);

        let m = matches?;

        assert!(handler(&m).is_err());

        Ok(())
    }

    #[test]
    fn test_duplicate_rates() -> Result<()> {
        let matches = new().try_get_matches_from([
            "bin",
            "--dsn",
            "",
            "-s",
            "/tmp/a.sock",
            "-l",
            "7",
            "-r",
            "3600",
            "-l",
            "10",
            "-r",
            "3600",
        ]);

        let m = matches?;

        assert!(handler(&m).is_err());

        Ok(())
    }

    #[test]
    fn test_mismatched_trailing_limit() -> Result<()> {
        let matches = new().try_get_matches_from([
            "bin",
            "--dsn",
            "",
            "-s",
            "/tmp/a.sock",
            "-l",
            "7",
            "-r",
            "3600",
            "-l",
            "100",
            "-r",
            "86400",
            "-l",
            "1000",
            "-r",
            "2592000",
            "-l",
            "10000",
        ]);

        let m = matches?;

        assert!(handler(&m).is_err());

        Ok(())
    }
}
