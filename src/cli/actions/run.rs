use crate::{cli::actions::Action, queries::Queries};
use anyhow::Result;
use futures::{SinkExt, StreamExt};
use secrecy::ExposeSecret;
use sqlx::any::AnyPoolOptions;
use std::path::Path;
use tokio::net::{UnixListener, UnixStream};
use tokio_util::codec::{Framed, LinesCodec};
use tracing::{debug, error, info};

/// Handle the create action
pub async fn handle(action: Action) -> Result<()> {
    match action {
        Action::Run { dsn, socket } => {
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

            debug!("Attempting to connect to database...");
            let pool = AnyPoolOptions::new()
                .max_connections(5)
                .connect(dsn_str)
                .await?;

            let queries = Queries::new(pool);

            // Start accepting connections
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        info!("New client connected: {:#?}", stream.local_addr());

                        // Spawn a new task to handle this client
                        tokio::spawn(handle_client(stream, queries.clone()));
                    }

                    Err(e) => {
                        error!("Failed to accept connection: {:?}", e);
                    }
                }
            }
        }
    }
}

async fn handle_client(stream: UnixStream, queries: Queries) -> Result<()> {
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
        send_policy_response(&mut framed, "action=DUNNO").await?;
        return Ok(());
    };

    info!(
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
        // TODO cuser.limit, cuser.rate)
        queries.create_user(&username, 10, 3600).await?;
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
