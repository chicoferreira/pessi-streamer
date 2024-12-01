use crate::packet::BootstrapperNeighboursResponse;
use anyhow::Context;
use log::{info, warn};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpSocket;
use tokio::time::timeout;

const RETRY_DELAY: Duration = Duration::from_secs(5);

pub async fn fetch_neighbours(
    node_ip: SocketAddr,
    bootstrapper_addr: SocketAddr,
) -> anyhow::Result<BootstrapperNeighboursResponse> {
    let socket = TcpSocket::new_v4().context("Failed to create a new IPv4 socket")?;

    // quickly rebind a socket in windows
    socket.set_reuseaddr(true)?;

    socket
        .bind(node_ip)
        .context("Failed to bind the socket to the provided node IP address")?;

    let mut stream = timeout(Duration::from_secs(5), socket.connect(bootstrapper_addr))
        .await
        .context("Connection attempt timed out")?
        .context("Failed to connect to the bootstrapper address")?;

    let mut buf = [0u8; 1024];
    let n = timeout(Duration::from_secs(5), stream.read(&mut buf))
        .await
        .context("Reading response timed out")?
        .context("Failed to read from the stream")?;

    if n == 0 {
        anyhow::bail!("No data received from bootstrapper");
    }

    let response =
        bincode::deserialize(&buf[..n]).context("Failed to deserialize response packet")?;

    stream
        .shutdown()
        .await
        .context("Failed to shutdown the stream")?;

    Ok(response)
}

pub async fn fetch_bootstrapper_with_retries(
    node_ip: SocketAddr,
    bootstrapper_addr: SocketAddr,
) -> BootstrapperNeighboursResponse {
    let mut attempt = 0;

    info!("Fetching neighbours from bootstrapper at: {bootstrapper_addr}");

    loop {
        attempt += 1;
        match fetch_neighbours(node_ip, bootstrapper_addr).await {
            Ok(response) => return response,
            Err(e) => {
                warn!("Failed to fetch neighbours (attempt {attempt}): {e}. Retrying in {RETRY_DELAY:?}...",);
                tokio::time::sleep(RETRY_DELAY).await;
            }
        }
    }
}
