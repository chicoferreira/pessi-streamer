use anyhow::Context;
use log::{error, info, warn};
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpSocket;
use tokio::time::{sleep, timeout};

const RETRY_COUNT: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_secs(2);

pub async fn fetch_neighbours(
    node_ip: SocketAddr,
    bootstraper_addr: SocketAddr,
) -> anyhow::Result<Vec<IpAddr>> {
    let socket = TcpSocket::new_v4().context("Failed to create a new IPv4 socket")?;

    // quickly rebind a socket in windows
    socket.set_reuseaddr(true)?;

    socket
        .bind(node_ip)
        .context("Failed to bind the socket to the provided node IP address")?;

    let mut stream = timeout(Duration::from_secs(5), socket.connect(bootstraper_addr))
        .await
        .context("Connection attempt timed out")?
        .context("Failed to connect to the bootstrapper address")?;

    let packet = crate::packet::NBPacket::RequestNeighbours;
    let serialized_packet =
        bincode::serialize(&packet).context("Failed to serialize RequestNeighbours packet")?;

    stream
        .write_all(&serialized_packet)
        .await
        .context("Failed to send the RequestNeighbours packet")?;

    let mut buf = [0u8; 1024];
    let n = timeout(Duration::from_secs(5), stream.read(&mut buf))
        .await
        .context("Reading response timed out")?
        .context("Failed to read from the stream")?;

    if n == 0 {
        anyhow::bail!("No data received from bootstrapper");
    }

    let packet =
        bincode::deserialize(&buf[..n]).context("Failed to deserialize response packet")?;

    stream
        .shutdown()
        .await
        .context("Failed to shutdown the stream")?;

    match packet {
        crate::packet::BNPacket::Neighbours(neighbours) => Ok(neighbours),
    }
}

pub async fn fetch_neighbours_with_retries(node_ip: SocketAddr) -> anyhow::Result<Vec<IpAddr>> {
    let mut attempt = 0;

    let bootstraper_addr = crate::get_bootstraper_address()?;
    info!(
        "Fetching neighbours from bootstrapper at: {}",
        bootstraper_addr
    );

    loop {
        attempt += 1;
        match fetch_neighbours(node_ip, bootstraper_addr).await {
            Ok(neighbours) => {
                return Ok(neighbours);
            }
            Err(e) if attempt < RETRY_COUNT => {
                let delay = RETRY_DELAY * attempt;
                warn!(
                    "Failed to fetch neighbours (attempt {}): {}. Retrying in {:?}...",
                    attempt, e, delay
                );
                sleep(delay).await;
            }
            Err(e) => {
                error!(
                    "Failed to fetch neighbours after {} attempts: {}",
                    RETRY_COUNT, e
                );
                return Err(e);
            }
        }
    }
}
