use common::packet::{ClientPacket, NodePacket, ServerPacket};
use common::reliable::ReliableUdpSocket;
use dashmap::DashMap;
use log::{error, info, trace};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

pub struct State {
    neighbours: Vec<IpAddr>,
    socket: ReliableUdpSocket,
    /// Availables videos in the network (received by the flood packets)
    available_videos: Vec<(u8, String)>,
    /// Map of video_id to list of clients interested in that video
    interested: Arc<DashMap<u8, Vec<SocketAddr>>>,
    /// Latest flood packet received (used temporarily to redirect packets for server)
    latest_received: Arc<Mutex<Option<SocketAddr>>>,
}

impl State {
    pub fn new(neighbours: Vec<IpAddr>, udp_socket: ReliableUdpSocket) -> Self {
        Self {
            neighbours,
            socket: udp_socket,
            available_videos: vec![],
            interested: Arc::new(DashMap::new()),
            latest_received: Arc::new(Mutex::new(None)),
        }
    }

    /// Returns true if the given IP address is a client
    /// For now we only check if the IP address is in the neighbours list
    ///
    /// This is used to check if a packet sent will be a ClientPacket or a NodePacket
    /// TODO: improve
    pub fn is_client(&self, ip_addr: IpAddr) -> bool {
        self.neighbours.contains(&ip_addr)
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
            NodePacket::RedirectToServer(packet) => {
                match packet {
                    ServerPacket::RequestVideo(video_id) => {
                        state.interested.entry(video_id).or_insert(vec![]).push(addr);
                    }
                    ServerPacket::StopVideo(video_id) => {
                        if let Some(mut subscribers) = state.interested.get_mut(&video_id) {
                            subscribers.retain(|&subscriber| subscriber != addr);
                        }
                    }
                }

                if let Some(latest_received) = state.latest_received.lock().unwrap().as_ref() {
                    state.socket.send_reliable(&packet, latest_received.clone()).await?;
                } else {
                    error!("Couldn't find suitable servers for redirecting packet");
                }
            }
            NodePacket::VideoPacket { stream_id, stream_data } => {
                trace!("Received video packet for stream {}", stream_id);
                if let Some(subscribers) = state.interested.get(&stream_id) {
                    for subscriber in subscribers.iter() {
                        if state.is_client(subscriber.ip()) {
                            state.socket.send_unreliable(&ClientPacket::VideoPacket { stream_id, stream_data: stream_data.clone() }, subscriber.clone()).await?;
                        } else {
                            state.socket.send_unreliable(&NodePacket::VideoPacket { stream_id, stream_data: stream_data.clone() }, subscriber.clone()).await?;
                        }
                    }
                }
            }
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

    state.latest_received.lock().unwrap().replace(peer_addr);

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
