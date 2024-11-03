use crate::server::State;
use anyhow::Context;
use env_logger::Env;
use log::{error, info};
use std::env;
use std::net::{IpAddr, SocketAddr};
use tokio::net::UdpSocket;

mod video;
mod server;

fn get_server_address() -> anyhow::Result<SocketAddr> {
    env::args().nth(1)
        .ok_or_else(|| anyhow::anyhow!("Usage: server <server_ip>"))
        .and_then(|ip_str| ip_str.parse::<IpAddr>().map_err(|_| anyhow::anyhow!("Invalid IP address provided: {}", ip_str)))
        .map(|server_ip| SocketAddr::new(server_ip, common::PORT))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();
    ffmpeg_sidecar::download::auto_download().context("Failed to download ffmpeg")?;

    info!("Starting server...");

    let server_addr = get_server_address()?;

    let neighbours = common::neighbours::fetch_neighbours_with_retries(server_addr).await?
        .into_iter()
        .map(|ip| SocketAddr::new(ip, common::PORT))
        .collect();

    let clients_socket = UdpSocket::bind(server_addr)
        .await
        .context("Failed to bind to clients socket")?;

    let state = State::new(clients_socket, neighbours);

    let videos = vec!["video.mp4", ];

    for video in videos {
        if let Err(e) = state.start_streaming_video(video.to_string()).await {
            error!("Failed to start video task: {}", e);
        }
    }

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Received Ctrl-C, shutting down...");
        }
        _ = server::flood::run_periodic_flood_packets(state.clone()) => {
            error!("Periodic flood packets task ended unexpectedly");
        }
        Err(e) = server::run_client_socket(state) => {
            error!("Client socket task ended unexpectedly: {}", e);
        }
    }

    Ok(())
}
