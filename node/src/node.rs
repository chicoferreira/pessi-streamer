use std::net::{IpAddr, SocketAddr};

use common::packet::SNCPacket;
use log::{error, info, trace};
use tokio::net::UdpSocket;

pub struct Neighbour {}

pub struct State {
    /// List of neighbours
    neighbours: Vec<IpAddr>,
    udp_socket: UdpSocket,
}

impl State {
    pub fn new(neighbours: Vec<IpAddr>, udp_socket: UdpSocket) -> Self {
        Self {
            neighbours,
            udp_socket,
        }
    }
}

pub async fn run_node(state: State) -> anyhow::Result<()> {
    info!("Waiting for packets on {}...", state.udp_socket.local_addr()?);

    let mut buf = [0u8; 1024];
    loop {
        let (n, peer_addr) = match state.udp_socket.recv_from(&mut buf).await {
            Ok(r) => r,
            Err(e) => {
                error!("Failed to receive packet: {}", e);
                continue;
            }
        };

        let packet = bincode::deserialize(&buf[..n]);
        match packet {
            Ok(SNCPacket::FloodPacket { hops, millis_created_at_server, videos_available } ) => {
                info!("Received flood packet from {} with {} hops and {} videos", peer_addr, hops, videos_available.len());
                controlled_flood(&state, peer_addr, hops, millis_created_at_server, videos_available).await;
            }
            Err(e) => error!("Failed to deserialize packet from client {:?}: {}", peer_addr, e),
        }
    }
}

/// Controlled flood consists of sending a packet to all neighbours except the one that sent it to us
async fn controlled_flood(state: &State, peer_addr: SocketAddr, hops: u8, millis_created_at_server: u128, videos_available: Vec<String>) {
    let flood_packet = SNCPacket::FloodPacket {
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
        match state.udp_socket.send_to(&packet, SocketAddr::new(*addr, common::PORT)).await {
            Ok(_) => {
                trace!("Sent flood packet to {}", addr);
            }
            Err(e) => {
                error!("Failed to send flood packet to {}: {}", addr, e);
            }
        }
    }
}
