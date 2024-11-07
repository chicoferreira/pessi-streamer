use crate::server::State;
use anyhow::Context;
use clap::{command, Parser};
use env_logger::Env;
use log::{error, info};
use std::net::{IpAddr, SocketAddr};
use tokio::net::UdpSocket;

mod video;
mod server;

/// Simple program to start a server
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Server IP address
    #[arg(short, long)]
    ip: IpAddr,
    /// Path to the videos directory
    #[arg(short, long, default_value = "videos")]
    videos: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();
    ffmpeg_sidecar::download::auto_download().context("Failed to download ffmpeg")?;

    let args = Args::parse();
    let server_addr = SocketAddr::new(args.ip, common::PORT);

    info!("Starting server...");

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
