use circular_buffer::CircularBuffer;
use common::packet::{FloodPacket, NodePacket, Packet, ServerPacket};
use common::reliable::ReliableUdpSocket;
use dashmap::DashMap;
use log::{error, info, warn};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

#[derive(Debug, Default, Eq, PartialEq)]
pub enum RouteStatus {
    #[default]
    Active,
    Unresponsive,
}

#[derive(Debug)]
struct ParentData {
    parents: Vec<SocketAddr>,
    date_last_flood_packet_received: SystemTime,
}

#[derive(Debug, Default)]
enum NodeType {
    /// If the node is parent, it must contain its parents.
    /// A parent is a node that can reach the server without passing through the same node again
    Parent(ParentData),
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

        self.status = RouteStatus::Active;

        self.node_type = NodeType::Parent(ParentData {
            parents: flood_packet.my_parents.clone(),
            date_last_flood_packet_received: SystemTime::now(),
        });
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
    /// Last sequence number received in a flood packet, used to ignore old packets
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
            .filter(|entry| entry.status == RouteStatus::Active)
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

    pub fn get_parents(&self) -> Vec<SocketAddr> {
        self.available_routes
            .iter()
            .filter(|route_info| matches!(route_info.node_type, NodeType::Parent(_)))
            .map(|route_info| *route_info.key())
            .collect()
    }

    pub async fn mark_node_as_unresponsive(&self, addr: SocketAddr) {
        {
            let mut route_info = self.available_routes.get_mut(&addr).unwrap();
            route_info.status = RouteStatus::Unresponsive;
            // drop route_info to release the lock
        }

        // Remove all videos that were being received from the
        // unresponsive node and redirect them to another node
        let videos_from_addr = self
            .video_routes
            .iter()
            .filter(|entry| *entry.value() == addr)
            .map(|entry| *entry.key());

        for video_id in videos_from_addr {
            let addr = self.get_best_node_to_redirect(video_id);
            if let Some(addr) = addr {
                if let Err(e) = self.request_video_to_node(video_id, addr).await {
                    error!("Failed to redirect video {}: {}", video_id, e);
                }
            } else {
                warn!("Couldn't find a suitable node to redirect video {video_id}");
            }
        }
    }

    pub async fn request_video_to_node(
        &self,
        video_id: u8,
        node: SocketAddr,
    ) -> anyhow::Result<()> {
        let packet = Packet::ServerPacket(ServerPacket::RequestVideo(video_id));
        self.socket.send_reliable(&packet, node).await?;
        self.video_routes.insert(video_id, node);
        Ok(())
    }

    async fn handle_packet(&self, packet: Packet, addr: SocketAddr) -> anyhow::Result<()> {
        match packet {
            Packet::NodePacket(node_packet) => match node_packet {
                NodePacket::ClientPing { sequence_number } => {
                    self.handle_client_ping(sequence_number, addr).await?
                }
                NodePacket::FloodPacket(packet) => self.handle_flood_packet(addr, packet).await,
            },
            Packet::ServerPacket(server_packet) => match server_packet {
                ServerPacket::RequestVideo(id) => self.handle_request_video(id, addr).await?,
                ServerPacket::StopVideo(id) => self.handle_stop_video(id, addr).await?,
            },
            Packet::VideoPacket(packet) => self.handle_video_packet(addr, packet).await?,
            _ => anyhow::bail!("Received unexpected packet"),
        }
        Ok(())
    }
}

pub async fn run_check_neighbours_task(state: State) {
    loop {
        tokio::time::sleep(common::FLOOD_PACKET_INTERVAL).await;
        for entry in state.available_routes.iter() {
            let addr = *entry.key();
            let route_info = entry.value();

            let parent_data = match &route_info.node_type {
                NodeType::Parent(parent_data) => parent_data,
                NodeType::Child => continue,
            };

            let last_flood_packet_received = parent_data.date_last_flood_packet_received;
            let time_since_last_flood_packet =
                last_flood_packet_received.elapsed().unwrap_or_default();

            if time_since_last_flood_packet > common::FLOOD_PACKET_INTERVAL * 2 {
                warn!("Parent node {addr} hasn't sent a flood packet in {time_since_last_flood_packet:?}. Marking it as unresponsive.");
                drop(entry);
                // Node is unresponsive mark it as such
                state.mark_node_as_unresponsive(addr).await;
            }
        }
    }
}

pub async fn run_packet_task(state: State) -> anyhow::Result<()> {
    info!("Waiting for packets on {}...", state.socket.local_addr()?);

    let mut buf = [0u8; 65536];
    loop {
        let (packet, addr): (Packet, SocketAddr) = match state.socket.receive(&mut buf).await {
            Ok(Some(result)) => result,
            Ok(None) => continue, // Acknowledgement packet received
            Err(e) => {
                error!("Failed to receive packet: {}", e);
                continue;
            }
        };

        if let Err(e) = state.handle_packet(packet, addr).await {
            error!("Failed to handle packet: {}", e);
        }
    }
}
