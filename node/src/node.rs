use circular_buffer::CircularBuffer;
use common::packet::{
    ClientPacket, FloodPacket, NodePacket, Packet, ServerPacket, VideoListPacket, VideoPacket,
};
use common::reliable::ReliableUdpSocket;
use dashmap::DashMap;
use log::{debug, error, info, trace};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

#[derive(Debug, Default)]
struct RouteInfo {
    hops: u8,
    last_delays_to_server: CircularBuffer<10, Duration>,
    videos: Vec<u8>,
}

impl RouteInfo {
    fn update_from_flood_packet(&mut self, flood_packet: &FloodPacket) {
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
    }
}

pub struct State {
    neighbours: Vec<IpAddr>,
    socket: ReliableUdpSocket,
    /// The known names of the videos
    video_names: Arc<DashMap<u8, String>>,
    /// Map of video_id to list of clients interested in that video
    interested: Arc<DashMap<u8, Vec<SocketAddr>>>,
    /// All the possible routes to reach the server
    available_routes: Arc<DashMap<SocketAddr, RouteInfo>>,
    /// The videos that are being received
    video_routes: Arc<DashMap<u8, SocketAddr>>,
    /// Last sequence number received to calculate video packet losses
    last_video_sequence_number: Arc<DashMap<u8, u64>>,
    /// Last sequence number received in a flood packet
    last_flood_packet_sequence_number: AtomicU64,
}

impl State {
    pub fn new(neighbours: Vec<IpAddr>, udp_socket: ReliableUdpSocket) -> Self {
        Self {
            neighbours,
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

    pub async fn handle_request_video(&self, video_id: u8, addr: SocketAddr) -> anyhow::Result<()> {
        let mut interested = self.interested.entry(video_id).or_default();
        if !interested.value().contains(&addr) {
            interested.value_mut().push(addr);
        }

        if self.video_routes.contains_key(&video_id) {
            // We are already receiving this video
            return Ok(());
        }

        let best_node = self.get_best_node_to_redirect(video_id);
        if let Some(node) = best_node {
            info!("Selected best node to redirect: {:?}", best_node);
            let packet = Packet::ServerPacket(ServerPacket::RequestVideo(video_id));
            self.socket.send_reliable(&packet, node).await?;
            self.video_routes.insert(video_id, node);
            Ok(())
        } else {
            // TODO: Add to queue and wait for a better node
            anyhow::bail!("Couldn't find suitable servers for redirecting packet")
        }
    }

    pub async fn handle_stop_video(&self, video_id: u8, addr: SocketAddr) -> anyhow::Result<()> {
        if let Some(mut subscribers) = self.interested.get_mut(&video_id) {
            subscribers.retain(|&subscriber| subscriber != addr);
        }

        let remaining_subscribers = self
            .interested
            .get(&video_id)
            .map_or(0, |s| s.value().len());

        if remaining_subscribers == 0 {
            if let Some((_, node_addr)) = self.video_routes.remove(&video_id) {
                let packet = Packet::ServerPacket(ServerPacket::StopVideo(video_id));
                self.socket.send_reliable(&packet, node_addr).await?;
            }
        }

        Ok(())
    }

    pub async fn handle_flood_packet(&self, from_addr: SocketAddr, flood_packet: FloodPacket) {
        debug!("Received flood packet {flood_packet:?} from {from_addr}");

        self.update_video_names(flood_packet.videos_available.clone());

        let mut node_route_info = self
            .available_routes
            .entry(from_addr)
            .or_insert(RouteInfo::default());

        node_route_info.update_from_flood_packet(&flood_packet);

        let last_flood_packet_sequence_number = self
            .last_flood_packet_sequence_number
            .load(Ordering::Relaxed);

        let sequence_number = flood_packet.sequence_number;
        if sequence_number > last_flood_packet_sequence_number {
            self.last_flood_packet_sequence_number
                .store(sequence_number, Ordering::Relaxed);

            let flood_packet = Packet::NodePacket(NodePacket::FloodPacket(FloodPacket {
                hops: flood_packet.hops + 1,
                ..flood_packet
            }));

            self.broadcast_flood_packet(from_addr, flood_packet).await;
        }
    }

    async fn broadcast_flood_packet(&self, from_addr: SocketAddr, flood_packet: Packet) {
        let broadcast_to: Vec<SocketAddr> = self
            .neighbours
            .iter()
            .map(|addr| SocketAddr::new(*addr, common::PORT))
            .filter(|addr| *addr != from_addr)
            .collect();

        debug!("Broadcasting flood packet ({flood_packet:?}) to {broadcast_to:?}");

        for addr in broadcast_to {
            match self.socket.send_reliable(&flood_packet, addr).await {
                Ok(()) => trace!("Sent flood packet to {}", addr),
                Err(e) => error!("Failed to send flood packet to {}: {}", addr, e),
            }
            // TODO: handle not received flood packet
        }
    }

    async fn handle_video_packet(
        &self,
        addr: SocketAddr,
        packet: VideoPacket,
    ) -> anyhow::Result<()> {
        let stream_id = packet.stream_id;
        let sequence_number = packet.sequence_number;
        let last_sequence_number = self
            .last_video_sequence_number
            .insert(stream_id, sequence_number)
            .unwrap_or(sequence_number);

        let stream_data_len = packet.stream_data.len();
        trace!("Received video packet (stream={stream_id}, seq={sequence_number}, n={stream_data_len}) from {addr}");

        let expected = last_sequence_number + 1;
        if expected != sequence_number {
            error!("Received out of order packet (expected {expected}, got {sequence_number})");
        }

        if let Some(subscribers) = self.interested.get(&stream_id) {
            let subscribers = subscribers.clone();

            trace!("Sending video packet (stream={stream_id}, seq={sequence_number}) to {subscribers:?}");

            let packet = Packet::VideoPacket(packet);

            self.socket
                .send_unreliable_broadcast(&packet, &subscribers)
                .await?;
        }

        Ok(())
    }

    pub fn get_videos(&self) -> Vec<(u8, String)> {
        self.video_names
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect()
    }

    pub async fn handle_client_ping(
        &self,
        sequence_number: u64,
        addr: SocketAddr,
    ) -> anyhow::Result<()> {
        debug!("Received ping from {}", addr);
        let packet = Packet::ClientPacket(ClientPacket::VideoList(VideoListPacket {
            sequence_number,
            videos: self.get_videos(),
            answer_creation_date: SystemTime::now(),
        }));
        self.socket.send_reliable(&packet, addr).await?;
        Ok(())
    }

    fn update_video_names(&self, video_names: Vec<(u8, String)>) {
        for (id, video_name) in video_names {
            self.video_names.insert(id, video_name);
        }
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

        if let Err(e) = handle_packet(&mut state, packet, addr).await {
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
