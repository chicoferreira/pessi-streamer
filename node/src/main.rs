use anyhow::Context;
use log::info;
use std::env;
use std::net::{IpAddr, SocketAddr};
use tokio::net::UdpSocket;

struct Neighbour {}

struct State {
    neighbours: Vec<IpAddr>,
    udp_socket: UdpSocket,
}

fn get_node_address() -> anyhow::Result<SocketAddr> {
    env::args().nth(1)
        .ok_or_else(|| anyhow::anyhow!("Usage: node <node_ip>"))
        .and_then(|ip_str| ip_str.parse::<IpAddr>().map_err(|_| anyhow::anyhow!("Invalid IP address provided: {}", ip_str)))
        .map(|node_ip| SocketAddr::new(node_ip, common::PORT))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder().filter_level(log::LevelFilter::Debug).init();

    let node_addr = get_node_address()?;

    info!("Node starting at: {}", node_addr);

    let neighbours = common::neighbours::fetch_neighbours_with_retries(node_addr).await?;

    info!("Fetched neighbours: {:?}", neighbours);

    let udp_socket = UdpSocket::bind(node_addr)
        .await
        .context("Failed to bind UDP socket to node address")?;

    loop {
        let mut buf = [0u8; 1024];
        let (n, peer_addr) = udp_socket.recv_from(&mut buf).await?;

        info!("Received {} bytes from {}", n, peer_addr);
    }
}