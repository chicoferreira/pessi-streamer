use crate::node::State;
use anyhow::Context;
use clap::{command, Parser};
use log::info;
use std::net::{IpAddr, SocketAddr};
use tokio::net::UdpSocket;

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
    env_logger::builder().filter_level(log::LevelFilter::Debug).init();

    let args = Args::parse();
    let node_addr = SocketAddr::new(args.ip, common::PORT);

    info!("Starting node...");

    let neighbours = common::neighbours::fetch_neighbours_with_retries(node_addr).await?;

    info!("Fetched neighbours: {:?}", neighbours);

    let udp_socket = UdpSocket::bind(node_addr)
        .await
        .context("Failed to bind UDP socket to node address")?;

    let state = State::new(neighbours, udp_socket);

    node::run_node(state).await?;

    Ok(())
}
