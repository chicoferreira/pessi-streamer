use std::net::{IpAddr, SocketAddr};

use common::packet::NodePacket;
use common::reliable::ReliableUdpSocket;
use log::{error, info, trace};

pub struct Neighbour {}

pub struct State {
    /// List of neighbours
    neighbours: Vec<IpAddr>,
    socket: ReliableUdpSocket,
}

impl State {
    pub fn new(neighbours: Vec<IpAddr>, udp_socket: ReliableUdpSocket) -> Self {
        Self {
            neighbours,
            socket: udp_socket,
        }
    }
}

pub async fn run_node(state: State) -> anyhow::Result<()> {
    info!("Waiting for packets on {}...", state.socket.local_addr()?);

    let mut buf = [0u8; 1024];
    loop {
        let (packet, addr): (NodePacket, SocketAddr) = match state.socket.receive(&mut buf).await {
            Ok(Some(result)) => result,
            Ok(None) => {
                // Acknowledgement packet received
                continue;
            }
            Err(e) => {
                error!("Failed to receive packet: {}", e);
                continue;
            }
        };

        match packet {
            NodePacket::FloodPacket { hops, millis_created_at_server, videos_available } => {
                info!("Received flood packet from {} with {} hops and {} videos", addr, hops, videos_available.len());
                controlled_flood(&state, addr, hops, millis_created_at_server, videos_available).await;
            }
        }
    }
}

/// Controlled flood consists of sending a packet to all neighbours except the one that sent it to us
async fn controlled_flood(state: &State, peer_addr: SocketAddr, hops: u8, millis_created_at_server: u128, videos_available: Vec<String>) {
    let flood_packet = NodePacket::FloodPacket {
        hops: hops + 1,
        millis_created_at_server,
        videos_available,
    };

    for addr in &state.neighbours {
        // We don't want to send the packet back to the peer that sent it to us, evicting cycles
        if *addr == peer_addr.ip() {
            continue;
        }

        let packet = bincode::serialize(&flood_packet).unwrap();
        match state.socket.send_reliable(&packet, SocketAddr::new(*addr, common::PORT)).await {
            Ok(_) => {
                trace!("Sent flood packet to {}", addr);
            }
            Err(e) => {
                error!("Failed to send flood packet to {}: {}", addr, e);
            }
        }
    }
}
