use clap::{
    Arg, ArgAction, ColorChoice, Command, ValueHint,
    builder::styling::{AnsiColor, Effects, Styles},
};
use std::path::PathBuf;

pub mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

/// Custom validator that checks if the socket path's directory exists
fn validate_socket_path(path: &str) -> Result<PathBuf, String> {
    let path_buf = PathBuf::from(path);
    path_buf.parent().map_or_else(
        || Err("Invalid socket path".to_string()),
        |parent| {
            if parent.exists() && parent.is_dir() {
                Ok(path_buf.clone())
            } else {
                Err(format!("Directory does not exist: {}", parent.display()))
            }
        },
    )
}

pub fn new() -> Command {
    let styles = Styles::styled()
        .header(AnsiColor::Yellow.on_default() | Effects::BOLD)
        .usage(AnsiColor::Green.on_default() | Effects::BOLD)
        .literal(AnsiColor::Blue.on_default() | Effects::BOLD)
        .placeholder(AnsiColor::Green.on_default());

    Command::new("policyd-rate-limit")
        .about("Postfix policy daemon for rate limiting")
        .version(env!("CARGO_PKG_VERSION"))
        .long_version(built_info::GIT_COMMIT_HASH.to_owned())
        .color(ColorChoice::Auto)
        .styles(styles)
        .arg(
            Arg::new("socket")
                .short('s')
                .long("socket")
                .help("Path to the Unix domain socket")
                .default_value("/tmp/policy-rate-limit.sock")
                .value_parser(validate_socket_path)
                .value_name("SOCKET")
                .value_hint(ValueHint::FilePath),
        )
        .arg(
            Arg::new("dsn")
                .long("dsn")
                .help("Database connection string")
                .env("DSN")
                .required(true),
        )
        .arg(
            Arg::new("pool")
                .long("pool")
                .help("Pool size for database connections")
                .default_value("5")
                .value_parser(clap::value_parser!(u32)),
        )
        .arg(
            Arg::new("limit")
                .short('l')
                .long("limit")
                .help("Maximum allowed messages")
                .default_value("10")
                .value_parser(clap::value_parser!(u32)),
        )
        .arg(
            Arg::new("rate")
                .short('r')
                .long("rate")
                .help("rate in seconds, limits the messages to be sent in the defined period")
                .default_value("86400")
                .value_parser(clap::value_parser!(u32)),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .help("Increase verbosity, -vv for debug")
                .action(ArgAction::Count),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::path::Path;

    #[test]
    fn test_defaults() {
        let matches = new().try_get_matches_from(["bin"]);

        assert!(matches.is_err());
    }

    #[test]
    fn test_defaults_with_dsn() -> Result<()> {
        let matches =
            new().try_get_matches_from(["bin", "-s", "/tmp/a.sock", "--dsn", "redis://localhost"]);

        assert!(matches.is_ok());

        let m = matches.unwrap();

        assert_eq!(
            m.get_one::<PathBuf>("socket").map(|s| s.as_path()),
            Some(Path::new("/tmp/a.sock"))
        );

        assert_eq!(m.get_one::<u8>("verbose").copied(), Some(0));

        assert_eq!(
            m.get_one::<String>("dsn").map(|s| s.as_str()),
            Some("redis://localhost")
        );

        Ok(())
    }

    #[test]
    fn test_verbose() -> Result<()> {
        let matches = new().try_get_matches_from(["bin", "-vv", "--dsn", "", "-s", "/tmp/a.sock"]);

        assert!(matches.is_ok());

        let m = matches.unwrap();

        assert_eq!(
            m.get_one::<PathBuf>("socket").map(|s| s.as_path()),
            Some(Path::new("/tmp/a.sock"))
        );

        assert_eq!(m.get_one::<u8>("verbose").copied(), Some(2));

        assert_eq!(m.get_one::<String>("dsn").map(|s| s.as_str()), Some(""));

        Ok(())
    }

    #[test]
    fn test_limit() -> Result<()> {
        let matches =
            new().try_get_matches_from(["bin", "-l", "20", "--dsn", "", "-s", "/tmp/a.sock"]);

        assert!(matches.is_ok());

        let m = matches.unwrap();

        assert_eq!(
            m.get_one::<PathBuf>("socket").map(|s| s.as_path()),
            Some(Path::new("/tmp/a.sock"))
        );

        assert_eq!(m.get_one::<u8>("verbose").copied(), Some(0));

        assert_eq!(m.get_one::<String>("dsn").map(|s| s.as_str()), Some(""));

        assert_eq!(m.get_one::<u32>("limit").copied(), Some(20));

        Ok(())
    }

    #[test]
    fn test_rate() -> Result<()> {
        let matches =
            new().try_get_matches_from(["bin", "-r", "3600", "--dsn", "", "-s", "/tmp/a.sock"]);

        assert!(matches.is_ok());

        let m = matches.unwrap();

        assert_eq!(
            m.get_one::<PathBuf>("socket").map(|s| s.as_path()),
            Some(Path::new("/tmp/a.sock"))
        );

        assert_eq!(m.get_one::<u8>("verbose").copied(), Some(0));

        assert_eq!(m.get_one::<String>("dsn").map(|s| s.as_str()), Some(""));

        assert_eq!(m.get_one::<u32>("rate").copied(), Some(3600));

        Ok(())
    }
}
