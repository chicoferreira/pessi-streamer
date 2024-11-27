use circular_buffer::CircularBuffer;
use common::packet::{ClientPacket, NodePacket, Packet, ServerPacket};
use common::reliable::ReliableUdpSocket;
use dashmap::DashMap;
use log::{debug, error, info, trace};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

struct RouteInfo {
    hops: u8,
    last_delays_to_server: CircularBuffer<10, Duration>,
    videos: Vec<u8>,
}

pub struct State {
    neighbours: Vec<IpAddr>,
    socket: ReliableUdpSocket,
    /// Availables videos in the network (received by the flood packets)
    available_videos: Vec<(u8, String)>,
    /// Map of video_id to list of clients interested in that video
    interested: Arc<DashMap<u8, Vec<SocketAddr>>>,
    /// All the possible routes to reach the server
    available_routes: Arc<DashMap<SocketAddr, RouteInfo>>,
    /// The videos that are being received
    video_routes: Arc<DashMap<u8, SocketAddr>>,
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
            available_routes: Arc::new(DashMap::new()),
            video_routes: Arc::new(DashMap::new()),
            last_sequence_number: Arc::new(DashMap::new()),
        }
    }

    pub fn get_best_node_to_redirect(&self, video_id: u8) -> Option<SocketAddr> {
        /// best by the number of hops
        /// if the number of hops is the same and the delay is more than 20% of the average, choose the one with the less delay
        /// if the number of hops is the same, choose the one with the less videos requested
        /// if the number of videos requested is the same, choose the one with the less videos available
        self.available_routes
            .iter()
            .filter(|entry| entry.videos.contains(&video_id))
            .min_by_key(|entry| {
                let route_info = entry.value();
                let hops = route_info.hops;
                let last_delays_to_server = &route_info.last_delays_to_server;
                let average_delay = last_delays_to_server.iter().sum::<Duration>()
                    / last_delays_to_server.len() as u32;
                let videos_requested = self.get_number_of_videos_requested_via_node(*entry.key());
                let videos_available = route_info.videos.len();

                (hops, average_delay, videos_requested, videos_available)
            })
            .map(|entry| *entry.key())
    }

    /// Helper method to get the number of videos requested via a given node
    fn get_number_of_videos_requested_via_node(&self, node: SocketAddr) -> usize {
        self.video_routes
            .iter()
            .filter(|entry| *entry.value() == node)
            .count()
    }

    pub async fn handle_request_video(&self, video_id: u8, addr: SocketAddr) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn handle_stop_video(&self, video_id: u8, addr: SocketAddr) -> anyhow::Result<()> {
        Ok(())
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
                created_at_server_time,
                videos_available,
            } => {
                debug!(
                    "Received flood packet from {} ({:?}) with {} hops and {} videos",
                    addr,
                    created_at_server_time,
                    hops,
                    videos_available.len()
                );
                // TODO: Change this if we want to have multiple servers
                state.available_videos.clone_from(&videos_available);

                let new_flood_packet = Packet::NodePacket(NodePacket::FloodPacket {
                    hops: hops + 1,
                    created_at_server_time,
                    videos_available,
                });

                controlled_flood(state, addr, new_flood_packet).await;
            }
        },
        Packet::ServerPacket(server_packet) => {
            match server_packet {
                ServerPacket::RequestVideo(video_id) => {
                    state.handle_request_video(video_id, addr).await?;
                    // let mut interested = state.interested.entry(video_id).or_default();
                    // if !interested.value().contains(&addr) {
                    //     interested.value_mut().push(addr);
                    // }
                    // info!("Received request to start video {} from {}", video_id, addr);
                }
                ServerPacket::StopVideo(video_id) => {
                    state.handle_stop_video(video_id, addr).await?;
                    // if let Some(mut subscribers) = state.interested.get_mut(&video_id) {
                    //     subscribers.retain(|&subscriber| subscriber != addr);
                    // }
                    // info!("Received request to stop video {} from {}", video_id, addr);
                }
            }
            // if let Some(node) = state.get_best_node_to_redirect() {
            //     state
            //         .socket
            //         .send_reliable(&Packet::ServerPacket(server_packet), node)
            //         .await?;
            // } else {
            //     error!("Couldn't find suitable servers for redirecting packet");
            // }
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

                trace!(
                    "Sending video packet (stream={}, seq={}) to {:?}",
                    stream_id,
                    sequence_number,
                    subscribers
                );
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
async fn controlled_flood(state: &State, peer_addr: SocketAddr, flood_packet: Packet) {
    // state.latest_received.lock().unwrap().replace(peer_addr);

    for addr in &state.neighbours {
        if *addr == peer_addr.ip() {
            continue;
        }

        let socket_addr = SocketAddr::new(*addr, common::PORT);
        match state.socket.send_reliable(&flood_packet, socket_addr).await {
            Ok(()) => {
                trace!("Sent flood packet to {}", addr);
            }
            Err(e) => {
                error!("Failed to send flood packet to {}: {}", addr, e);
            }
        }
    }
}
