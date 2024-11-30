use crate::node::State;
use clap::{command, Parser};
use log::info;
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

    let bootstraper_addr = common::get_bootstraper_address()?;

    let response =
        common::neighbours::fetch_bootstrapper_with_retries(node_addr, bootstraper_addr).await;

    info!("Fetched from bootstrapper: {response:?}");

    let reliable_udp_socket = common::reliable::ReliableUdpSocket::new(node_addr).await?;

    let state = State::new(response.id, response.neighbours, reliable_udp_socket);

    node::run_node(state).await?;

    Ok(())
}
