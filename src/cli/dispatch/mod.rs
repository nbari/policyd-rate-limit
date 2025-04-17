use crate::cli::actions::Action;
use anyhow::Result;
use secrecy::SecretString;
use std::path::PathBuf;

pub fn handler(matches: &clap::ArgMatches) -> Result<Action> {
    let socket: &PathBuf = matches.get_one("socket").expect("socket required");

    Ok(Action::Run {
        socket: socket.to_path_buf(),
        dsn: SecretString::from(
            matches
                .get_one::<String>("dsn")
                .map(|s| s.to_string())
                .unwrap_or_default(),
        ),
        pool: matches.get_one::<u32>("pool").copied().unwrap_or(5),
        limit: matches.get_one::<u32>("limit").copied().unwrap_or(10) as i32,
        rate: matches.get_one::<u32>("rate").copied().unwrap_or(86400) as i32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::commands::new;
    use secrecy::ExposeSecret;
    use std::path::Path;

    #[test]
    fn test_handler() -> Result<()> {
        let matches = new().try_get_matches_from(["bin", "--dsn", "", "-s", "/tmp/a.sock"]);

        assert!(matches.is_ok());

        let m = matches.unwrap();

        assert_eq!(
            m.get_one::<PathBuf>("socket").map(|s| s.as_path()),
            Some(Path::new("/tmp/a.sock"))
        );

        assert_eq!(m.get_one::<u8>("verbose").copied(), Some(0));

        assert_eq!(m.get_one::<String>("dsn").map(|s| s.as_str()), Some(""));

        let action = handler(&m)?;

        match action {
            Action::Run {
                socket,
                dsn,
                pool,
                limit,
                rate,
            } => {
                assert_eq!(socket, Path::new("/tmp/a.sock"));
                assert_eq!(dsn.expose_secret(), "");
                assert_eq!(limit, 10);
                assert_eq!(rate, 86400);
                assert_eq!(pool, 5);
            }
        }

        Ok(())
    }
}
