use circular_buffer::CircularBuffer;
use common::packet::{FloodPacket, NodePacket, Packet, ServerPacket};
use common::reliable::ReliableUdpSocket;
use dashmap::DashMap;
use log::{error, info};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Default)]
pub enum RouteStatus {
    #[default]
    Active,
    Unresponsive,
}

#[derive(Debug, Default)]
enum NodeType {
    /// If the node is father, it must contain its parents.
    /// A father is a node that can reach the server without passing through the same node again
    Father(Vec<SocketAddr>),
    #[default]
    Child,
}

#[derive(Debug, Default)]
pub struct RouteInfo {
    hops: u8,
    last_delays_to_server: CircularBuffer<10, Duration>,
    videos: Vec<u8>,
    pub status: RouteStatus,
    node_type: NodeType,
}

impl RouteInfo {
    pub fn update_from_flood_packet(&mut self, flood_packet: &FloodPacket) {
        self.hops = flood_packet.hops;
        self.videos = flood_packet
            .videos_available
            .iter()
            .map(|(id, _)| *id)
            .collect();
        let duration = flood_packet
            .created_at_server_time
            .elapsed()
            .unwrap_or(Duration::ZERO);
        self.last_delays_to_server.push_back(duration);
        self.node_type = NodeType::Father(flood_packet.my_fathers.clone());
    }
}

#[derive(Clone)]
pub struct State {
    /// The id of this node
    pub id: u64,
    /// The known neighbours of this node
    pub neighbours: Arc<Vec<IpAddr>>,
    /// The udp socket to communicate with other nodes
    pub socket: ReliableUdpSocket,
    /// The known names of the videos
    pub video_names: Arc<DashMap<u8, String>>,
    /// Map of video_id to list of clients interested in that video
    pub interested: Arc<DashMap<u8, Vec<SocketAddr>>>,
    /// All the possible routes to reach the server
    pub available_routes: Arc<DashMap<SocketAddr, RouteInfo>>,
    /// The videos that are being received
    pub video_routes: Arc<DashMap<u8, SocketAddr>>,
    /// Last sequence number received to calculate video packet losses
    pub last_video_sequence_number: Arc<DashMap<u8, u64>>,
    /// Last sequence number received in a flood packet
    pub last_flood_packet_sequence_number: Arc<AtomicU64>,
}

impl State {
    pub fn new(id: u64, neighbours: Vec<IpAddr>, udp_socket: ReliableUdpSocket) -> Self {
        Self {
            id,
            neighbours: Arc::new(neighbours),
            socket: udp_socket,
            video_names: Default::default(),
            interested: Default::default(),
            available_routes: Default::default(),
            video_routes: Default::default(),
            last_video_sequence_number: Default::default(),
            last_flood_packet_sequence_number: Default::default(),
        }
    }

    pub fn get_best_node_to_redirect(&self, video_id: u8) -> Option<SocketAddr> {
        // Select the best node based on the following criteria (in order):
        // 1. Fewest hops
        // 2. If hops are equal, choose the node with the lowest delay TODO: change this to a threshold percentage
        // 3. If delay is also equal, choose the node with fewer requested videos
        // 4. If requested videos are also equal, choose the node with fewer available videos
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

    pub fn get_videos(&self) -> Vec<(u8, String)> {
        self.video_names
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect()
    }

    pub fn get_fathers(&self) -> Vec<SocketAddr> {
        self.available_routes
            .iter()
            .filter(|route_info| matches!(route_info.node_type, NodeType::Father(_)))
            .map(|route_info| *route_info.key())
            .collect()
    }

    pub fn update_video_names(&self, video_names: Vec<(u8, String)>) {
        for (id, video_name) in video_names {
            self.video_names.insert(id, video_name);
        }
    }
}

pub async fn run_node(state: State) -> anyhow::Result<()> {
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

        if let Err(e) = handle_packet(&state, packet, addr).await {
            error!("Failed to handle packet: {}", e);
        }
    }
}

async fn handle_packet(state: &State, packet: Packet, addr: SocketAddr) -> anyhow::Result<()> {
    match packet {
        Packet::NodePacket(node_packet) => match node_packet {
            NodePacket::ClientPing { sequence_number } => {
                state.handle_client_ping(sequence_number, addr).await?
            }
            NodePacket::FloodPacket(packet) => state.handle_flood_packet(addr, packet).await,
        },
        Packet::ServerPacket(server_packet) => match server_packet {
            ServerPacket::RequestVideo(id) => state.handle_request_video(id, addr).await?,
            ServerPacket::StopVideo(id) => state.handle_stop_video(id, addr).await?,
        },
        Packet::VideoPacket(packet) => state.handle_video_packet(addr, packet).await?,
        _ => anyhow::bail!("Received unexpected packet"),
    }
    Ok(())
}
