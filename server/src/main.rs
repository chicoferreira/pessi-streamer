use crate::server::State;
use anyhow::Context;
use clap::{command, Parser};
use env_logger::Env;
use log::{error, info};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use walkdir::WalkDir;

mod server;
mod video;

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

fn get_files(dir: PathBuf) -> Vec<PathBuf> {
    WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.path().to_path_buf())
        .collect()
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

    let video_folder = PathBuf::from("./videos");
    let videos = get_files(video_folder.clone());

    for video in videos {
        let state = state.clone();
        let video_folder = video_folder.clone();
        tokio::spawn(async move {
            state
                .start_streaming_video(video, video_folder)
                .await
                .expect("Failed to start streaming video");
        });
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
