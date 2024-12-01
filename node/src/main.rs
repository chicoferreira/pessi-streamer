use crate::node::State;
use clap::{command, Parser};
use log::{error, info};
use std::net::{IpAddr, SocketAddr};

mod handle;
mod node;

/// Simple program to start a node
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Node IP address
    #[arg(short, long)]
    ip: IpAddr,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    let args = Args::parse();
    let node_addr = SocketAddr::new(args.ip, common::PORT);

    info!("Starting node...");

    let bootstrapper_addr = common::get_bootstrapper_address()?;

    let response =
        common::neighbours::fetch_bootstrapper_with_retries(node_addr, bootstrapper_addr).await;

    info!("Fetched from bootstrapper: {response:?}");

    let reliable_udp_socket = common::reliable::ReliableUdpSocket::new(node_addr).await?;

    let state = State::new(response.id, response.neighbours, reliable_udp_socket);

    tokio::select! {
        _ = tokio::signal::ctrl_c() => info!("Received Ctrl-C, shutting down..."),
        err = node::run_packet_task(state.clone()) => error!("Packet task ended unexpectedly: {err:?}"),
        err = node::run_check_neighbours_task(state) => error!("Check neighbours status ended unexpectedly: {err:?}"),
    }

    Ok(())
}
