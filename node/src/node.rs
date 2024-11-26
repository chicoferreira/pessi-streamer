use common::packet::{ClientPacket, NodePacket, Packet, ServerPacket};
use common::reliable::ReliableUdpSocket;
use dashmap::DashMap;
use log::{debug, error, info, trace};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

pub struct State {
    neighbours: Vec<IpAddr>,
    socket: ReliableUdpSocket,
    /// Availables videos in the network (received by the flood packets)
    available_videos: Vec<(u8, String)>,
    /// Map of video_id to list of clients interested in that video
    interested: Arc<DashMap<u8, Vec<SocketAddr>>>,
    /// Latest flood packet received (used temporarily to redirect packets for server)
    latest_received: Arc<Mutex<Option<SocketAddr>>>,
    /// Last sequence number received
    last_sequence_number: Arc<DashMap<u8, u64>>,
}

impl State {
    pub fn new(neighbours: Vec<IpAddr>, udp_socket: ReliableUdpSocket) -> Self {
        Self {
            neighbours,
            socket: udp_socket,
            available_videos: vec![],
            interested: Arc::new(DashMap::new()),
            latest_received: Arc::new(Mutex::new(None)),
            last_sequence_number: Arc::new(DashMap::new()),
        }
    }

    pub fn get_best_node_to_redirect(&self) -> Option<SocketAddr> {
        *self.latest_received.lock().unwrap()
    }
}

pub async fn run_node(mut state: State) -> anyhow::Result<()> {
    info!("Waiting for packets on {}...", state.socket.local_addr()?);

    let mut buf = [0u8; 65536];
    loop {
        let (packet, addr): (Packet, SocketAddr) = match state.socket.receive(&mut buf).await {
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

        handle_packet(&mut state, packet, addr).await?;
    }
}

async fn handle_packet(state: &mut State, packet: Packet, addr: SocketAddr) -> anyhow::Result<()> {
    match packet {
        Packet::NodePacket(node_packet) => match node_packet {
            NodePacket::ClientPing { sequence_number } => {
                debug!("Received ping from {}", addr);
                let packet = Packet::ClientPacket(ClientPacket::VideoList {
                    sequence_number,
                    videos: state.available_videos.clone(),
                    answer_creation_date: SystemTime::now(),
                });
                state.socket.send_reliable(&packet, addr).await?;
            }
            NodePacket::FloodPacket {
                hops,
                created_at_server_time: millis_created_at_server,
                videos_available,
            } => {
                debug!(
                    "Received flood packet from {} ({:?}) with {} hops and {} videos",
                    addr,
                    millis_created_at_server,
                    hops,
                    videos_available.len()
                );
                state.available_videos.clone_from(&videos_available);
                controlled_flood(
                    state,
                    addr,
                    hops,
                    millis_created_at_server,
                    videos_available,
                )
                .await;
            }
        },
        Packet::ServerPacket(server_packet) => {
            match server_packet {
                ServerPacket::RequestVideo(video_id) => {
                    let mut interested = state.interested.entry(video_id).or_default();
                    if !interested.value().contains(&addr) {
                        interested.value_mut().push(addr);
                    }
                    info!("Received request to start video {} from {}", video_id, addr);
                }
                ServerPacket::StopVideo(video_id) => {
                    if let Some(mut subscribers) = state.interested.get_mut(&video_id) {
                        subscribers.retain(|&subscriber| subscriber != addr);
                    }
                    info!("Received request to stop video {} from {}", video_id, addr);
                }
            }
            if let Some(node) = state.get_best_node_to_redirect() {
                state
                    .socket
                    .send_reliable(&Packet::ServerPacket(server_packet), node)
                    .await?;
            } else {
                error!("Couldn't find suitable servers for redirecting packet");
            }
        }
        Packet::VideoPacket {
            stream_id,
            sequence_number,
            stream_data,
        } => {
            let last_sequence_number = state
                .last_sequence_number
                .insert(stream_id, sequence_number)
                .unwrap_or(sequence_number);

            let expected = last_sequence_number + 1;

            trace!(
                "Received video packet (stream={}, seq={}, expected={}, n={}) from {}",
                stream_id,
                sequence_number,
                expected,
                stream_data.len(),
                addr
            );

            if expected != sequence_number {
                error!(
                    "Received out of order packet (expected {}, got {})",
                    expected, sequence_number
                );
            }
            if let Some(subscribers) = state.interested.get(&stream_id) {
                let subscribers = subscribers.clone();
                let packet = Packet::VideoPacket {
                    stream_id,
                    sequence_number,
                    stream_data,
                };
                state
                    .socket
                    .send_unreliable_broadcast(&packet, &subscribers)
                    .await?;
            }
        }
        _ => {
            error!("Received unexpected packet: {:?}", packet);
        }
    }
    Ok(())
}

/// Controlled flood consists of sending a packet to all neighbours except the one that sent it to us
async fn controlled_flood(
    state: &State,
    peer_addr: SocketAddr,
    hops: u8,
    millis_created_at_server: SystemTime,
    videos_available: Vec<(u8, String)>,
) {
    let flood_packet = Packet::NodePacket(NodePacket::FloodPacket {
        hops: hops + 1,
        created_at_server_time: millis_created_at_server,
        videos_available,
    });

    state.latest_received.lock().unwrap().replace(peer_addr);

    for addr in &state.neighbours {
        if *addr == peer_addr.ip() {
            continue;
        }

        match state
            .socket
            .send_reliable(&flood_packet, SocketAddr::new(*addr, common::PORT))
            .await
        {
            Ok(_) => {
                trace!("Sent flood packet to {}", addr);
            }
            Err(e) => {
                error!("Failed to send flood packet to {}: {}", addr, e);
            }
        }
    }
}
