#![cfg(unix)]

use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use secrecy::SecretString;
use sqlx::SqlitePool;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    time::sleep,
};

use policyd_rate_limit::{
    RateLimit,
    cli::actions::{self, Action},
};
const SQLITE_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS ratelimit (
    username VARCHAR(128) NOT NULL,
    quota INTEGER NOT NULL DEFAULT 0,
    used INTEGER NOT NULL DEFAULT 0,
    rate INTEGER NOT NULL DEFAULT 0,
    rdate TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (username, rate)
);
";

fn socket_tests_enabled() -> bool {
    std::env::var_os("RUN_SOCKET_TESTS").is_some()
}

fn base_test_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("POLICYD_TEST_TMPDIR") {
        return PathBuf::from(dir);
    }

    if let Some(dir) = std::env::var_os("CARGO_TARGET_DIR") {
        return PathBuf::from(dir).join("policyd-test");
    }

    PathBuf::from("target").join("policyd-test")
}

fn unique_path(prefix: &str, ext: &str) -> Result<PathBuf> {
    let path = base_test_dir();
    std::fs::create_dir_all(&path)?;
    let mut path = std::fs::canonicalize(&path)?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_nanos();
    let pid = std::process::id();
    path.push(format!("policyd-rate-limit-{prefix}-{pid}-{nanos}{ext}"));
    Ok(path)
}

fn unique_socket_path() -> Result<PathBuf> {
    let base =
        std::env::var_os("XDG_RUNTIME_DIR").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    std::fs::create_dir_all(&base)?;

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_nanos();
    let pid = std::process::id();
    let filename = format!("prl-{pid}-{nanos}.sock");
    let mut path = base.join(filename);

    if path.to_string_lossy().len() >= 100 {
        path = PathBuf::from("/tmp").join(format!("prl-{pid}-{nanos}.sock"));
    }

    Ok(path)
}

fn socket_bind_supported(path: &Path) -> Result<bool> {
    match StdUnixListener::bind(path) {
        Ok(listener) => {
            drop(listener);
            let _ = std::fs::remove_file(path);
            Ok(true)
        }
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => Ok(false),
        Err(err) => Err(err.into()),
    }
}

async fn setup_sqlite_db(path: &Path) -> Result<String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if !path.exists() {
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
    }
    let dsn = format!("sqlite://{}?mode=rwc", path.display());
    let pool = SqlitePool::connect(&dsn).await?;
    sqlx::query(SQLITE_SCHEMA).execute(&pool).await?;
    pool.close().await;
    Ok(dsn)
}

async fn read_policy_response(stream: &mut UnixStream) -> Result<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 128];

    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        let Some(slice) = chunk.get(..read) else {
            return Err(anyhow!("failed to read response chunk"));
        };
        buffer.extend_from_slice(slice);
        if buffer.windows(2).any(|window| window == b"\n\n") {
            break;
        }
    }

    Ok(String::from_utf8_lossy(&buffer).to_string())
}

#[tokio::test]
async fn socket_creates_rows_for_new_user() -> Result<()> {
    if !socket_tests_enabled() {
        eprintln!("Skipping socket integration test; set RUN_SOCKET_TESTS=1 to run.");
        return Ok(());
    }

    let db_path = unique_path("socket", ".db")?;
    let socket_path = unique_socket_path()?;
    if !socket_bind_supported(&socket_path)? {
        eprintln!("Skipping socket integration test; unix sockets are not permitted.");
        return Ok(());
    }
    // Use a file-backed SQLite DB so the daemon and test can share it.
    let dsn = setup_sqlite_db(&db_path).await?;

    let windows = vec![
        RateLimit {
            limit: 7,
            rate: 3600,
        },
        RateLimit {
            limit: 100,
            rate: 86400,
        },
        RateLimit {
            limit: 10000,
            rate: 2_592_000,
        },
    ];

    let action = Action::Run {
        socket: socket_path.clone(),
        dsn: SecretString::from(dsn.clone()),
        pool: 1,
        windows,
    };

    // Run the daemon in the background for the socket test.
    let handle = tokio::spawn(async move { actions::run::handle(action).await });

    // Wait for the socket to be created before connecting.
    for _ in 0..50 {
        if socket_path.exists() {
            break;
        }
        if handle.is_finished() {
            let result = handle.await;
            return Err(anyhow!("daemon exited early: {result:?}"));
        }
        sleep(Duration::from_millis(100)).await;
    }

    if !socket_path.exists() {
        if handle.is_finished() {
            let result = handle.await;
            return Err(anyhow!("daemon exited early: {result:?}"));
        }
        handle.abort();
        let _ = handle.await;
        return Err(anyhow!("socket was not created"));
    }

    let mut stream = UnixStream::connect(&socket_path).await?;
    // Minimal policy request: sasl_username is the key used for rate limiting.
    let payload = "request=smtpd\nsasl_username=socket-user@example.com\n\n";
    stream.write_all(payload.as_bytes()).await?;

    let response = read_policy_response(&mut stream).await?;
    if !response.contains("action=") {
        handle.abort();
        let _ = handle.await;
        return Err(anyhow!("unexpected policy response: {response}"));
    }

    // Verify one row per configured window for the new user.
    let pool = SqlitePool::connect(&dsn).await?;
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM ratelimit WHERE username = ?")
        .bind("socket-user@example.com")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count.0, 3);

    handle.abort();
    let _ = handle.await;

    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&db_path);

    Ok(())
}
