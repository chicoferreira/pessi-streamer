use std::net::{IpAddr, SocketAddr};

use common::packet::{ClientPacket, NodePacket};
use common::reliable::ReliableUdpSocket;
use log::{error, info, trace};

pub struct Neighbour {}

pub struct State {
    /// List of neighbours
    neighbours: Vec<IpAddr>,
    socket: ReliableUdpSocket,
    available_videos: Vec<(u8, String)>,
}

impl State {
    pub fn new(neighbours: Vec<IpAddr>, udp_socket: ReliableUdpSocket) -> Self {
        Self {
            neighbours,
            socket: udp_socket,
            available_videos: vec![],
        }
    }
}

pub async fn run_node(mut state: State) -> anyhow::Result<()> {
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
                info!("Received flood packet from {} ({}) with {} hops and {} videos", addr, millis_created_at_server, hops, videos_available.len());
                state.available_videos.clone_from(&videos_available);
                controlled_flood(&state, addr, hops, millis_created_at_server, videos_available).await;
            }
            NodePacket::ClientPing { sequence_number } => {
                info!("Received ping from {}", addr);
                let packet = ClientPacket::VideoList {
                    sequence_number,
                    videos: state.available_videos.clone(),
                };
                state.socket.send_reliable(&packet, addr).await?;
            }
            NodePacket::RedirectToServer(_) => todo!()
        }
    }
}

/// Controlled flood consists of sending a packet to all neighbours except the one that sent it to us
async fn controlled_flood(state: &State, peer_addr: SocketAddr, hops: u8, millis_created_at_server: u128, videos_available: Vec<(u8, String)>) {
    let flood_packet = NodePacket::FloodPacket {
        hops: hops + 1,
        millis_created_at_server,
        videos_available,
    };

    for addr in &state.neighbours {
        if *addr == peer_addr.ip() {
            continue;
        }

        match state.socket.send_reliable(&flood_packet, SocketAddr::new(*addr, common::PORT)).await {
            Ok(_) => {
                trace!("Sent flood packet to {}", addr);
            }
            Err(e) => {
                error!("Failed to send flood packet to {}: {}", addr, e);
            }
        }
    }
}
