use crate::server::State;
use anyhow::Context;
use env_logger::Env;
use log::{error, info};
use std::net::Ipv4Addr;
use tokio::net::UdpSocket;

mod video;
mod server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();
    ffmpeg_sidecar::download::auto_download().context("Failed to download ffmpeg")?;

    info!("Starting server...");

    let clients_socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, common::SERVER_PORT))
        .await
        .context("Failed to bind to clients socket")?;

    let state = State::new(clients_socket);

    let videos = list_videos("./videos".to_string());

    for video in videos {
        if let Err(e) = state.start_streaming_video(video.to_string()).await {
            error!("Failed to start video task: {}", e);
        }
    }

    server::run_client_socket(state).await?;

    Ok(())
}

fn list_videos(path: String) -> Vec<String> {
    let mut videos = Vec::new();
    for entry in std::fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_file() {
            videos.push(path.to_str().unwrap().to_string());
        }
    }

    videos
}
