use crate::server::State;
use anyhow::Context;
use clap::{command, Parser};
use env_logger::Env;
use log::{error, info};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

mod server;
mod video;

/// Simple program to start a server
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Server IP address
    #[arg(short, long, default_value = "0.0.0.0")]
    ip: IpAddr,
    /// Path to the videos directory
    #[arg(short, long, default_value = "videos")]
    videos: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();
    ffmpeg_sidecar::download::auto_download().context("Failed to download ffmpeg")?;

    let args = Args::parse();
    let server_addr = SocketAddr::new(args.ip, common::PORT);

    info!("Starting server...");

    let bootstrapper_addr = common::get_bootstrapper_address()?;

    let response =
        common::neighbours::fetch_bootstrapper_with_retries(server_addr, bootstrapper_addr).await;

    let neighbours = response
        .neighbours
        .into_iter()
        .map(|ip| SocketAddr::new(ip, common::PORT))
        .collect();

    info!("Fetched neighbours: {:?}", neighbours);

    let clients_socket = common::reliable::ReliableUdpSocket::new(server_addr).await?;

    let state = State::new(clients_socket, response.id, neighbours);

    tokio::select! {
        _ = tokio::signal::ctrl_c() => info!("Received Ctrl-C, shutting down..."),
        r = tokio::spawn(server::flood::run_periodic_flood_packets(state.clone())) =>
            error!("Periodic flood packets task ended unexpectedly: {r:?}"),
        r = tokio::spawn(server::watch_video_folder(state.clone(), args.videos)) =>
            error!("Watch video folder task ended unexpectedly: {r:?}"),
        r = tokio::spawn(server::run_client_socket(state)) =>
            error!("Client socket task ended unexpectedly: {r:?}"),
    }

    Ok(())
}
