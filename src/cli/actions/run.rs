use crate::{cli::actions::Action, queries::Queries};
use anyhow::Result;
use futures::{SinkExt, StreamExt};
use secrecy::ExposeSecret;
use sqlx::any::AnyPoolOptions;
use std::{path::Path, time::Duration};
use tokio::net::{UnixListener, UnixStream};
use tokio_util::codec::{Framed, LinesCodec};
use tracing::{debug, error, info, warn};

/// Handle the create action
pub async fn handle(action: Action) -> Result<()> {
    match action {
        Action::Run {
            dsn,
            pool,
            socket,
            limit,
            rate,
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
            debug!("Connecting to database with DSN: {}", dsn_str);

            let pool = AnyPoolOptions::new()
                .max_connections(pool)
                .idle_timeout(Duration::from_secs(300))
                .connect(dsn_str)
                .await?;

            debug!(?pool, "Pool created");

            let queries = Queries::new(pool);

            // Start accepting connections
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        info!("New client connected: {:#?}", stream.local_addr());

                        // Spawn a new task to handle this client
                        tokio::spawn(handle_client(stream, queries.clone(), limit, rate));
                    }

                    Err(e) => {
                        error!("Failed to accept connection: {:?}", e);
                    }
                }
            }
        }
    }
}

async fn handle_client(stream: UnixStream, queries: Queries, limit: i32, rate: i32) -> Result<()> {
    let mut framed = Framed::new(stream, LinesCodec::new());
    let mut sasl_username: Option<String> = None;
    let mut received_lines = Vec::new();

    while let Some(Ok(line)) = framed.next().await {
        let trimmed = line.trim().to_string();

        if trimmed.is_empty() {
            break;
        }

        debug!("Received line: {}", trimmed);
        received_lines.push(trimmed.clone());

        if let Some(name) = trimmed.strip_prefix("sasl_username=") {
            sasl_username = Some(name.trim().to_string());
        }
    }

    let Some(username) = sasl_username else {
        send_policy_response(&mut framed, "action=REJECT user not provided").await?;

        warn!("No SASL username provided");

        return Ok(());
    };

    debug!(
        "SASL username: {}, Request:\n{}",
        username,
        received_lines.join("\n")
    );

    if let Some(within_quota) = queries.get_user(&username).await? {
        let allow = if within_quota {
            true
        } else {
            queries.reset_quota_if_expired(&username).await?
        };

        if allow {
            send_policy_response(&mut framed, "action=DUNNO").await?;
        } else {
            send_policy_response(&mut framed, "action=REJECT sending limit exceeded").await?;
        }

        queries.update_quota(&username).await?;
    } else {
        queries.create_user(&username, limit, rate).await?;
        send_policy_response(&mut framed, "action=DUNNO").await?;
    }

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
