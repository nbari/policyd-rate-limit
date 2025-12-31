use std::{path::Path, sync::Arc, time::Duration};

use anyhow::Result;
use futures::{SinkExt, StreamExt};
use secrecy::ExposeSecret;
use sqlx::any::AnyPoolOptions;
use tokio::net::{UnixListener, UnixStream};
use tokio_util::codec::{Framed, LinesCodec};
use tracing::{debug, error, info, warn};

use crate::{RateLimit, cli::actions::Action, queries::Queries};

fn redact_dsn(dsn: &str) -> String {
    let Some((scheme, rest)) = dsn.split_once("://") else {
        return dsn.to_string();
    };

    let Some(at_pos) = rest.find('@') else {
        return dsn.to_string();
    };

    let (creds, host) = rest.split_at(at_pos);
    let Some(colon_pos) = creds.find(':') else {
        return dsn.to_string();
    };

    let user = &creds[..colon_pos];
    format!("{scheme}://{user}:***{host}")
}

/// Handle the create action.
///
/// # Errors
/// Returns an error if the socket setup, database operations, or client handling fails.
pub async fn handle(action: Action) -> Result<()> {
    match action {
        Action::Run {
            dsn,
            pool,
            socket,
            windows,
        } => {
            if Path::new(&socket).exists() {
                std::fs::remove_file(&socket)?;
            }

            let listener = UnixListener::bind(&socket)?;

            println!(
                "{} - {}, listening on UNIX socket at {}...",
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
                &socket.display()
            );

            // Install default drivers for sqlx::any
            sqlx::any::install_default_drivers();

            let dsn_str = dsn.expose_secret();
            debug!("Connecting to database with DSN: {}", redact_dsn(dsn_str));

            let pool = AnyPoolOptions::new()
                .max_connections(pool)
                .idle_timeout(Duration::from_secs(300))
                .connect(dsn_str)
                .await?;

            debug!(?pool, "Pool created");

            let queries = Queries::new(pool);
            let windows = Arc::new(windows);

            // Start accepting connections
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        debug!("New client connected: {:#?}", stream.local_addr());

                        // Spawn a new task to handle this client
                        tokio::spawn(handle_client(stream, queries.clone(), windows.clone()));
                    }

                    Err(e) => {
                        error!("Failed to accept connection: {:?}", e);
                    }
                }
            }
        }
    }
}

async fn handle_client(
    stream: UnixStream,
    queries: Queries,
    windows: Arc<Vec<RateLimit>>,
) -> Result<()> {
    let mut framed = Framed::new(stream, LinesCodec::new());
    let mut sasl_username: Option<String> = None;
    let mut received_lines = Vec::new();

    while let Some(Ok(line)) = framed.next().await {
        let trimmed = line.trim().to_string();

        if trimmed.is_empty() {
            break;
        }

        received_lines.push(trimmed.clone());

        if let Some(name) = trimmed.strip_prefix("sasl_username=") {
            sasl_username = Some(name.trim().to_string());
        }
    }

    // Handle unauthenticated or empty SASL username (incoming mail)
    let Some(username) = sasl_username else {
        send_policy_response(&mut framed, "action=DUNNO").await?;

        warn!("No SASL username in policy request. Likely incoming mail.");

        return Ok(());
    };

    if username.is_empty() {
        send_policy_response(&mut framed, "action=DUNNO").await?;

        debug!("Empty SASL username in policy request. Skipping rate limit.");

        return Ok(());
    }

    debug!(
        "SASL username: {}, Request:\n{}",
        username,
        received_lines.join("\n")
    );

    match queries.reset_quotas_if_expired(&username).await {
        Ok(true) => info!("Reset expired quotas for user {}", username),
        Ok(false) => (),
        Err(e) => error!("Error checking quota expiration: {:?}", e),
    }

    let mut active_windows = queries.get_windows(&username).await?;
    if active_windows.is_empty() {
        info!("User {} not found, creating new user", username);

        // User not found, create a new one
        queries.create_user(&username, windows.as_ref()).await?;

        send_policy_response(&mut framed, "action=DUNNO").await?;
        return Ok(());
    }

    if active_windows.len() < windows.len() {
        if let Err(e) = queries.ensure_windows(&username, windows.as_ref()).await {
            error!("Failed to add missing windows for {}: {:?}", username, e);
        } else {
            active_windows = queries.get_windows(&username).await?;
        }
    }

    let allow = active_windows
        .iter()
        .all(|window| window.used < window.quota);

    if allow {
        info!("User {} is within quota", username);

        send_policy_response(&mut framed, "action=DUNNO").await?;
    } else {
        info!(
            "User {} is not within quota, sending limit exceeded, action=REJECT",
            username
        );
        send_policy_response(&mut framed, "action=REJECT sending limit exceeded").await?;
    }

    queries.update_quota(&username).await?;

    Ok(())
}

/// Send a policy response to the client
/// Postfix’s policy protocol expects two \n
async fn send_policy_response(
    framed: &mut Framed<UnixStream, LinesCodec>,
    response: &str,
) -> Result<()> {
    framed.send(response).await?;
    framed.send("").await?;

    Ok(())
}
