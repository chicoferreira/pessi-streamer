mod client;
mod ui;
mod video;

use crate::client::{
    handle_packet_task, handle_unresponsive_pings_nodes_task, start_ping_nodes_task, State,
};
use crate::video::VideoPlayerType;
use clap::Parser;
use log::{error, info};
use std::net::{IpAddr, Ipv4Addr};
use std::process::exit;

/// A simple program to watch live streams
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Name of the stream to watch
    #[arg(long)]
    stream: Option<String>,

    /// Possible servers to connect to
    #[arg(short, long, num_args = 0..)]
    servers: Vec<IpAddr>,

    /// The video player to use
    #[arg(short, long, default_value = "mpv")]
    video_player: VideoPlayerType,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    let args = Args::parse();

    if !args.video_player.check_installation().await {
        panic!("Video player {:?} is not installed", args.video_player);
    }

    let addr = (Ipv4Addr::UNSPECIFIED, 0).into();
    let socket = common::reliable::ReliableUdpSocket::new(addr).await?;

    let state = State::new(socket.clone(), args.servers, args.video_player);

    tokio::spawn({
        let state = state.clone();
        async move {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => info!("Received Ctrl-C"),
                r = tokio::spawn(start_ping_nodes_task(state.clone())) =>
                    error!("Ping task ended unexpectedly: {r:?}"),
                r = tokio::spawn(handle_unresponsive_pings_nodes_task(state.clone())) =>
                    error!("Unresponsive nodes task ended unexpectedly: {r:?}"),
                r = tokio::spawn(handle_packet_task(state)) =>
                    error!("Packet task ended unexpectedly: {r:?}"),
            }
            exit(0);
        }
    });

    if let Err(e) = ui::run_ui(state) {
        error!("Failed to run UI: {}", e);
    }

    info!("Shutting down...");

    Ok(())
}
