use anyhow::Context;
use std::env;
use std::net::SocketAddr;

/// Port used by the server and the nodes to communicate with each other
pub const PORT: u16 = 8010;

pub mod packet;
pub mod neighbours;
mod reliable;

pub fn get_bootstraper_address() -> anyhow::Result<SocketAddr> {
    env::var("BOOTSTRAPER_IP")
        .context("BOOTSTRAPER_IP environment variable not set")
        .and_then(|ip_str| ip_str.parse().map_err(|_| anyhow::anyhow!("Couldn't parse BOOTSTRAPER_IP as a valid address: {}", ip_str)))
}