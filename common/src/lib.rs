use anyhow::Context;
use std::env;
use std::net::SocketAddr;
use std::time::Duration;

/// Port used by the server and the nodes to communicate with each other
pub const PORT: u16 = 8010;

pub const FLOOD_PACKET_INTERVAL: Duration = Duration::from_secs(1);

pub mod neighbours;
pub mod packet;
pub mod reliable;

pub fn get_bootstrapper_address() -> anyhow::Result<SocketAddr> {
    env::var("BOOTSTRAPPER_IP")
        .context("BOOTSTRAPPER_IP environment variable not set")
        .and_then(|ip_str| {
            ip_str.parse().map_err(|_| {
                anyhow::anyhow!("Couldn't parse BOOTSTRAPPER_IP as a valid address: {ip_str}")
            })
        })
}
