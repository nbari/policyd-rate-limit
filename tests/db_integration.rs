use std::path::Path;

use anyhow::{Result, anyhow};
use sqlx::{AnyPool, any::AnyPoolOptions};
use testcontainers::{GenericImage, ImageExt, core::IntoContainerPort, runners::AsyncRunner};
use tokio::time::{Duration, sleep};

use policyd_rate_limit::{
    RateLimit,
    queries::{Queries, RateLimitWindow},
};

const POSTGRES_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS ratelimit (
    username VARCHAR(128) NOT NULL,
    quota INTEGER NOT NULL DEFAULT 0,
    used INTEGER NOT NULL DEFAULT 0,
    rate INTEGER NOT NULL DEFAULT 0,
    rdate TIMESTAMP WITHOUT TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (username, rate)
);
";

const MARIADB_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS ratelimit (
    username VARCHAR(128) NOT NULL,
    quota INT UNSIGNED NOT NULL DEFAULT 0,
    used INT UNSIGNED NOT NULL DEFAULT 0,
    rate INT UNSIGNED NOT NULL DEFAULT 0,
    rdate DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (username, rate)
) ENGINE=InnoDB;
";

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

fn configure_testcontainers_host() {
    static INIT: std::sync::OnceLock<()> = std::sync::OnceLock::new();

    INIT.get_or_init(|| {
        if std::env::var("DOCKER_HOST").is_ok() {
            return;
        }

        if let Ok(socket) = std::env::var("PODMAN_SOCKET") {
            let host = if socket.contains("://") {
                socket
            } else {
                format!("unix://{socket}")
            };
            // Safety: test setup runs before containers are started and only sets a single env var.
            unsafe {
                std::env::set_var("DOCKER_HOST", host);
            }
            return;
        }

        let mut candidates = Vec::new();
        if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
            candidates.push(format!("{dir}/podman/podman.sock"));
        }
        candidates.push("/run/podman/podman.sock".to_string());

        for path in candidates {
            if Path::new(&path).exists() {
                // Safety: test setup runs before containers are started and only sets a single env var.
                unsafe {
                    std::env::set_var("DOCKER_HOST", format!("unix://{path}"));
                }
                break;
            }
        }
    });
}

fn docker_tests_enabled() -> bool {
    std::env::var("RUN_DOCKER_TESTS").is_ok()
}

async fn connect_with_retry(dsn: &str, max_connections: u32) -> Result<AnyPool> {
    let mut last_err = None;

    for _ in 0..30 {
        match AnyPoolOptions::new()
            .max_connections(max_connections)
            .acquire_timeout(Duration::from_secs(2))
            .connect(dsn)
            .await
        {
            Ok(pool) => return Ok(pool),
            Err(err) => {
                last_err = Some(err);
                sleep(Duration::from_millis(500)).await;
            }
        }
    }

    Err(anyhow!(
        "timed out waiting for database to accept connections: {last_err:?}"
    ))
}

fn window_by_rate(windows: &[RateLimitWindow], rate: i32) -> Result<&RateLimitWindow> {
    windows
        .iter()
        .find(|window| window.rate == rate)
        .ok_or_else(|| anyhow!("missing window for rate {rate}"))
}

fn hourly_daily_windows() -> Vec<RateLimit> {
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
}

async fn exercise_missing_user(queries: &Queries) -> Result<()> {
    let missing = "missing@example.com";

    assert_eq!(queries.is_within_quota(missing).await?, None);
    queries.update_quota(missing).await?;
    assert_eq!(queries.is_within_quota(missing).await?, None);
    assert!(!queries.reset_quotas_if_expired(missing).await?);

    Ok(())
}

async fn exercise_zero_limit(queries: &Queries) -> Result<()> {
    let zero_limit = "zero@example.com";
    let zero_windows = vec![
        RateLimit { limit: 0, rate: 1 },
        RateLimit {
            limit: 10,
            rate: 3600,
        },
    ];

    queries.create_user(zero_limit, &zero_windows).await?;
    assert_eq!(queries.is_within_quota(zero_limit).await?, Some(false));
    queries.update_quota(zero_limit).await?;
    assert_eq!(queries.is_within_quota(zero_limit).await?, Some(false));

    Ok(())
}

async fn exercise_hourly_daily(queries: &Queries) -> Result<()> {
    let hourly_daily = "hourly-daily@example.com";
    let windows = hourly_daily_windows();

    queries.create_user(hourly_daily, &windows).await?;
    assert_eq!(queries.is_within_quota(hourly_daily).await?, Some(true));

    // Hitting the hourly limit blocks mail even though the daily limit remains available.
    for _ in 0..7 {
        queries.update_quota(hourly_daily).await?;
    }
    assert_eq!(queries.is_within_quota(hourly_daily).await?, Some(false));
    assert!(!queries.reset_quotas_if_expired(hourly_daily).await?);

    let windows = queries.get_windows(hourly_daily).await?;
    let hourly = window_by_rate(&windows, 3600)?;
    let daily = window_by_rate(&windows, 86400)?;
    assert_eq!(hourly.used, 7);
    assert_eq!(hourly.quota, 7);
    assert_eq!(daily.used, 7);
    assert_eq!(daily.quota, 100);

    Ok(())
}

async fn exercise_backfill(queries: &Queries) -> Result<()> {
    let backfill = "backfill@example.com";
    let partial_windows = vec![RateLimit {
        limit: 3,
        rate: 3600,
    }];
    let windows = hourly_daily_windows();

    queries.create_user(backfill, &partial_windows).await?;
    queries.update_quota(backfill).await?;
    queries.ensure_windows(backfill, &windows).await?;

    let windows = queries.get_windows(backfill).await?;
    let hourly = window_by_rate(&windows, 3600)?;
    let daily = window_by_rate(&windows, 86400)?;
    assert_eq!(hourly.used, 1);
    assert_eq!(hourly.quota, 3);
    assert_eq!(daily.used, 0);
    assert_eq!(daily.quota, 100);

    Ok(())
}

async fn exercise_concurrent(queries: &Queries) -> Result<()> {
    let concurrent = "concurrent@example.com";
    let windows = hourly_daily_windows();

    queries.create_user(concurrent, &windows).await?;
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..10 {
        let queries = queries.clone();
        let user = concurrent.to_string();
        set.spawn(async move { queries.update_quota(&user).await });
    }
    while let Some(result) = set.join_next().await {
        result??;
    }

    let windows = queries.get_windows(concurrent).await?;
    for window in windows {
        assert_eq!(window.used, 10);
    }

    Ok(())
}

async fn exercise_daily_cap(queries: &Queries) -> Result<()> {
    let daily_cap = "daily-cap@example.com";
    let daily_windows = vec![
        RateLimit { limit: 2, rate: 1 },
        RateLimit {
            limit: 2,
            rate: 86400,
        },
    ];

    queries.create_user(daily_cap, &daily_windows).await?;
    for _ in 0..2 {
        queries.update_quota(daily_cap).await?;
    }
    assert_eq!(queries.is_within_quota(daily_cap).await?, Some(false));

    sleep(Duration::from_secs(2)).await;

    let reset = queries.reset_quotas_if_expired(daily_cap).await?;
    assert!(reset);

    // Short window resets do not override the daily cap once it is reached.
    let windows = queries.get_windows(daily_cap).await?;
    let short = window_by_rate(&windows, 1)?;
    let daily = window_by_rate(&windows, 86400)?;
    assert_eq!(short.used, 0);
    assert_eq!(daily.used, 2);
    assert_eq!(queries.is_within_quota(daily_cap).await?, Some(false));

    Ok(())
}

async fn exercise_queries(queries: &Queries) -> Result<()> {
    exercise_missing_user(queries).await?;
    exercise_zero_limit(queries).await?;
    exercise_hourly_daily(queries).await?;
    exercise_backfill(queries).await?;
    exercise_concurrent(queries).await?;
    exercise_daily_cap(queries).await?;

    Ok(())
}

async fn run_db_test(dsn: &str, schema: &str, max_connections: u32) -> Result<()> {
    sqlx::any::install_default_drivers();

    let pool = connect_with_retry(dsn, max_connections).await?;

    sqlx::query(schema).execute(&pool).await?;

    let queries = Queries::new(pool);
    exercise_queries(&queries).await
}

#[tokio::test]
async fn postgres_queries() -> Result<()> {
    if !docker_tests_enabled() {
        eprintln!("Skipping Docker integration tests; set RUN_DOCKER_TESTS=1 to run.");
        return Ok(());
    }

    configure_testcontainers_host();

    let image = GenericImage::new("postgres", "18-alpine")
        .with_exposed_port(5432.tcp())
        .with_env_var("POSTGRES_USER", "test")
        .with_env_var("POSTGRES_PASSWORD", "test")
        .with_env_var("POSTGRES_DB", "testdb");
    let container = image.start().await?;
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;
    let dsn = format!("postgres://test:test@{host}:{port}/testdb");

    run_db_test(&dsn, POSTGRES_SCHEMA, 5).await
}

#[tokio::test]
async fn mariadb_queries() -> Result<()> {
    if !docker_tests_enabled() {
        eprintln!("Skipping Docker integration tests; set RUN_DOCKER_TESTS=1 to run.");
        return Ok(());
    }

    configure_testcontainers_host();

    let image = GenericImage::new("mariadb", "11.8")
        .with_exposed_port(3306.tcp())
        .with_env_var("MARIADB_ROOT_PASSWORD", "password")
        .with_env_var("MARIADB_DATABASE", "testdb");
    let container = image.start().await?;
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(3306).await?;
    let dsn = format!("mysql://root:password@{host}:{port}/testdb");

    run_db_test(&dsn, MARIADB_SCHEMA, 5).await
}

#[tokio::test]
async fn sqlite_queries() -> Result<()> {
    let dsn = "sqlite::memory:";

    run_db_test(dsn, SQLITE_SCHEMA, 1).await
}
