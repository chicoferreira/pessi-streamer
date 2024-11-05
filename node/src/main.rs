use crate::node::State;
use anyhow::Context;
use log::info;
use std::env;
use std::net::{IpAddr, SocketAddr};
use tokio::net::UdpSocket;

mod node;

fn get_node_address() -> anyhow::Result<SocketAddr> {
    env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("Usage: node <node_ip>"))
        .and_then(|ip_str| {
            ip_str
                .parse::<IpAddr>()
                .map_err(|_| anyhow::anyhow!("Invalid IP address provided: {}", ip_str))
        })
        .map(|node_ip| SocketAddr::new(node_ip, common::PORT))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    info!("Starting node...");

    let node_addr = get_node_address()?;

    let neighbours = common::neighbours::fetch_neighbours_with_retries(node_addr).await?;

    info!("Fetched neighbours: {:?}", neighbours);

    let udp_socket = UdpSocket::bind(node_addr)
        .await
        .context("Failed to bind UDP socket to node address")?;

    let state = State::new(neighbours, udp_socket);

    node::run_node(state).await?;

    Ok(())
}
