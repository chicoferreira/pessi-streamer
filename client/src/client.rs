use crate::video::{VideoPlayer, VideoPlayerType};
use anyhow::Context;
use circular_buffer::CircularBuffer;
use common::packet::{
    ClientPacket, NodePacket, Packet, ServerPacket, VideoListPacket, VideoPacket,
};
use common::reliable::ReliableUdpSocketError;
use common::VideoId;
use dashmap::DashMap;
use log::{debug, error, info, trace, warn};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

#[derive(PartialEq)]
pub enum NodeStatus {
    Connecting,
    Connected,
    Unresponsive,
}

pub struct Node {
    pub addr: SocketAddr,
    pub latest_rtts: CircularBuffer<10, Duration>,
    pub last_received_ping: Option<SystemTime>,
    pub status: NodeStatus,
    pub available_videos: Vec<VideoId>,
}

impl Node {
    fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            latest_rtts: Default::default(),
            last_received_ping: None,
            status: NodeStatus::Connecting,
            available_videos: vec![],
        }
    }

    fn register_ping_answer(&mut self, rtt: Duration, video_list: Vec<VideoId>) {
        self.latest_rtts.push_back(rtt);
        self.last_received_ping = Some(SystemTime::now());
        self.status = NodeStatus::Connected;
        self.available_videos = video_list;
    }

    pub fn average_rtt(&self) -> Duration {
        self.latest_rtts.iter().sum::<Duration>() / self.latest_rtts.len() as u32
    }
}

pub struct PlayingVideo {
    /// The source of the video stream
    /// None if the video is queued and waiting for a server to be connected
    pub source: Option<SocketAddr>,
    pub video_player: VideoPlayer,
    pub last_sequence_number: AtomicU64,
    pub bytes_written: AtomicUsize,
}

#[derive(Clone)]
pub struct State {
    /// The UDP socket to communicate with the nodes
    pub socket: common::reliable::ReliableUdpSocket,
    /// The list of possible servers to connect to
    pub nodes: Arc<DashMap<SocketAddr, Node>>,
    /// The list of pending pings to the nodes
    pub pending_pings: Arc<DashMap<u64, (SocketAddr, SystemTime)>>,
    /// The current sequence number to send ClientPing packets
    pub ping_sequence_number: Arc<AtomicU64>,
    /// The list of possible streams received by the nodes (stream_id, stream_name)
    pub video_names: Arc<DashMap<VideoId, String>>,
    /// The list of currently playing video processes
    pub playing_videos: Arc<DashMap<VideoId, PlayingVideo>>,
    /// The video player type to play the video
    pub video_player: VideoPlayerType,
}

impl State {
    pub fn new(
        socket: common::reliable::ReliableUdpSocket,
        servers: Vec<IpAddr>,
        video_player_type: VideoPlayerType,
    ) -> Self {
        let servers = servers
            .into_iter()
            .map(|addr| {
                let socket_addr = SocketAddr::new(addr, common::PORT);
                (socket_addr, Node::new(socket_addr))
            })
            .collect();

        Self {
            socket,
            nodes: Arc::new(servers),
            pending_pings: Arc::new(Default::default()),
            ping_sequence_number: Arc::new(Default::default()),
            video_names: Arc::new(Default::default()),
            playing_videos: Arc::new(Default::default()),
            video_player: video_player_type,
        }
    }

    pub fn available_videos(&self) -> Vec<VideoId> {
        let mut videos: Vec<_> = self
            .nodes
            .iter()
            .flat_map(|node| node.value().available_videos.clone())
            .collect();

        videos.sort_unstable();
        videos.dedup();

        videos
    }

    fn get_videos_playing_from_node(&self, addr: SocketAddr) -> Vec<VideoId> {
        self.playing_videos
            .iter()
            .filter(|entry| entry.value().source == Some(addr))
            .map(|entry| *entry.key())
            .collect()
    }

    async fn request_start_video(&self, addr: SocketAddr, video_id: VideoId) -> anyhow::Result<()> {
        let packet = Packet::ServerPacket(ServerPacket::RequestVideo(video_id));
        self.socket
            .send_reliable(&packet, addr)
            .await
            .context("Couldn't send request video packet")?;

        Ok(())
    }

    async fn request_stop_video(&self, addr: SocketAddr, video_id: VideoId) -> anyhow::Result<()> {
        let packet = Packet::ServerPacket(ServerPacket::StopVideo(video_id));
        self.socket
            .send_reliable(&packet, addr)
            .await
            .context("Couldn't send stop video packet")?;

        Ok(())
    }

    pub async fn select_best_server_and_request(&self, video_id: VideoId) -> Option<SocketAddr> {
        let best_node_addr = self.select_best_node(video_id);

        if let Some(best_node_addr) = best_node_addr {
            if let Err(e) = self.request_start_video(best_node_addr, video_id).await {
                error!("Failed to request video to {}: {}", best_node_addr, e);
            }
        }

        best_node_addr
    }

    pub fn ui_start_playing(&self, video_id: VideoId) {
        let state = self.clone();
        tokio::spawn(async move {
            let best_node_addr = state.select_best_server_and_request(video_id).await;

            if let Some(addr) = best_node_addr {
                info!("Selected server {addr} for video {video_id}");
            } else {
                warn!("Couldn't find any servers to play video {video_id}. Waiting...");
            }

            if let Err(e) = state.start_video_process(best_node_addr, video_id) {
                error!("Failed to start video process: {e}");
            }
        });
    }

    pub fn ui_stop_playing_id(&self, video_id: VideoId) {
        let state = self.clone();
        tokio::spawn(async move {
            if let Some((video_id, video)) = state.playing_videos.remove(&video_id) {
                if let Some(source) = video.source {
                    if let Err(e) = state.request_stop_video(source, video_id).await {
                        error!("Failed to request stop video to {}: {}", source, e);
                    }
                }
            }
        });
    }

    async fn handle_ping_answer(&self, addr: SocketAddr, video_list_packet: VideoListPacket) {
        let sequence_number = video_list_packet.sequence_number;
        for (video_id, video_name) in video_list_packet.videos.clone() {
            self.video_names.insert(video_id, video_name);
        }

        if let Some((_, (saved_addr, start_time))) = self.pending_pings.remove(&sequence_number) {
            if saved_addr != addr {
                warn!("Received ping answer from unexpected address {addr} (expect {saved_addr})");
            }

            let now = SystemTime::now();

            let date = video_list_packet.answer_creation_date;

            let to = date.duration_since(start_time).unwrap_or(Duration::ZERO);
            let from = now.duration_since(date).unwrap_or(Duration::ZERO);

            let total_rtt = now.duration_since(start_time).unwrap_or(Duration::ZERO);
            debug!(
                "Received ping answer from {addr} in {total_rtt:?} (TO: {to:?}, FROM: {from:?})"
            );

            let videos: Vec<_> = video_list_packet.videos.iter().map(|(id, _)| *id).collect();
            if let Some(mut node) = self.nodes.get_mut(&addr) {
                node.register_ping_answer(total_rtt, videos.clone());
            } else {
                warn!("Received ping answer from unknown node {}", addr);
            }

            let pending_videos: Vec<_> = self
                .playing_videos
                .iter_mut()
                .filter(|entry| entry.value().source.is_none())
                .filter(|entry| videos.contains(entry.key()))
                .map(|entry| *entry.key())
                .collect();

            for video_id in pending_videos {
                info!("Found available node for video {video_id} ({addr}). Redirecting...");
                let addr = self.select_best_server_and_request(video_id).await;
                self.playing_videos.get_mut(&video_id).unwrap().source = addr;
            }
        }
    }

    async fn handle_video_packet(&self, source: SocketAddr, video_packet: VideoPacket) {
        let stream_id = video_packet.stream_id;
        if let Some(video) = self.playing_videos.get(&stream_id) {
            let sequence_number = video_packet.sequence_number;
            trace!(
                "Received video packet for stream {stream_id} (SEQ={sequence_number}, DATA_LEN={})",
                video_packet.stream_data.len()
            );

            let last_sequence_number = video
                .last_sequence_number
                .swap(sequence_number, Ordering::Relaxed);

            let expected_sequence_number = last_sequence_number + 1;
            if sequence_number != expected_sequence_number {
                warn!(
                    "Received video packet out of order (GOT={} EXPECTED={}) for stream {}",
                    sequence_number, expected_sequence_number, stream_id
                );
            }

            let bytes = video_packet.stream_data.len();
            video.bytes_written.fetch_add(bytes, Ordering::Relaxed);

            if let Err(e) = video.video_player.write(video_packet.stream_data).await {
                error!("Failed to write video packet: {}", e);
                if let Err(e) = self.request_stop_video(source, stream_id).await {
                    error!("Failed to request stop video to {}: {}", source, e);
                }
                drop(video);
                self.playing_videos.remove(&stream_id);
            }
        } else {
            warn!("Received video packet for unwanted stream {}", stream_id);
        }
    }

    fn select_best_node(&self, video_id: VideoId) -> Option<SocketAddr> {
        // Select the best node based on the following criteria (in order):
        // 1. Choose nodes whose average delay is within 30% of the minimum average delay
        // 2. Among these nodes, choose the one with the fewest videos requested
        // 3. If number of requested videos are equal, choose the node with the fewest available videos
        const DELAY_THRESHOLD_PERCENTAGE: u32 = 30;

        let min_average_delay = self
            .nodes
            .iter()
            .filter(|node| node.status == NodeStatus::Connected)
            .filter(|node| node.available_videos.contains(&video_id))
            .map(|node| node.average_rtt())
            .min()?;

        let max_delay_threshold =
            min_average_delay + (min_average_delay / 100 * DELAY_THRESHOLD_PERCENTAGE);

        self.nodes
            .iter()
            .filter(|node| node.status == NodeStatus::Connected)
            .filter(|node| node.available_videos.contains(&video_id))
            .filter(|node| node.average_rtt() <= max_delay_threshold)
            .min_by_key(|entry| {
                let videos_requested = self.get_videos_playing_from_node(*entry.key()).len();
                let videos_available = entry.available_videos.len();

                (videos_requested, videos_available)
            })
            .map(|entry| *entry.key())
    }

    fn start_video_process(
        &self,
        source: Option<SocketAddr>,
        video_id: VideoId,
    ) -> anyhow::Result<()> {
        let video_player = VideoPlayer::launch(self.video_player)?;
        self.playing_videos.insert(
            video_id,
            PlayingVideo {
                source,
                video_player,
                last_sequence_number: Default::default(),
                bytes_written: Default::default(),
            },
        );
        Ok(())
    }
}

pub async fn handle_unresponsive_pings_nodes_task(state: State) {
    loop {
        tokio::time::sleep(common::CLIENT_PING_INTERVAL).await;
        let mut unresponsive = vec![];
        for entry in state.nodes.iter() {
            let node = entry.value();

            if node.status != NodeStatus::Connected {
                continue;
            }

            if let Some(last_ping) = node.last_received_ping {
                let now = SystemTime::now();
                let elapsed = now.duration_since(last_ping).unwrap_or(Duration::ZERO);

                if elapsed > common::CLIENT_PING_INTERVAL * 3 {
                    warn!("Node {} is not responding...", node.addr);
                    unresponsive.push(*entry.key());
                }
            }
        }

        for addr in unresponsive {
            if let Some(mut node) = state.nodes.get_mut(&addr) {
                node.status = NodeStatus::Unresponsive;
            }

            let requested_videos: Vec<_> = state
                .playing_videos
                .iter_mut()
                .filter(|entry| entry.value().source == Some(addr))
                .map(|entry| *entry.key())
                .collect();

            for video_id in requested_videos {
                let addr = state.select_best_server_and_request(video_id).await;
                if let Some(mut video) = state.playing_videos.get_mut(&video_id) {
                    video.source = addr;
                }
                if let Some(addr) = addr {
                    info!("Redirecting video {video_id} from unresponsive node to {addr}");
                } else {
                    warn!(
                        "Couldn't find any more servers to redirect video {video_id}. Waiting..."
                    );
                }
            }
        }
    }
}

pub async fn start_ping_nodes_task(state: State) {
    loop {
        tokio::time::sleep(common::CLIENT_PING_INTERVAL).await;

        for node in state.nodes.iter() {
            let sequence_number = state.ping_sequence_number.fetch_add(1, Ordering::Relaxed);
            let requested_videos = state.get_videos_playing_from_node(node.addr);
            let packet = Packet::NodePacket(NodePacket::ClientPing {
                sequence_number,
                requested_videos,
            });
            if let Err(e) = state.socket.send_reliable(&packet, node.addr).await {
                error!("Failed to send ping to {}: {}", node.addr, e);
            } else {
                let ping_info = (node.addr, SystemTime::now());
                state.pending_pings.insert(sequence_number, ping_info);
            }
            debug!("Sent ping ({packet:?}) to {}", node.addr);
        }
    }
}

pub async fn handle_packet_task(state: State) {
    let mut buf = [0u8; 65536];
    loop {
        let result = state.socket.receive(&mut buf).await;
        let (packet, addr) = match result {
            Ok(Some((packet, addr))) => (packet, addr),
            Ok(None) => continue,
            Err(e) => {
                if let ReliableUdpSocketError::IoError(e) = &e {
                    if e.kind() == std::io::ErrorKind::ConnectionReset {
                        // ignore flood on windows
                        continue;
                    }
                }
                error!("Failed to receive packet: {}", e);
                continue;
            }
        };
        match packet {
            Packet::VideoPacket(video_packet) => {
                state.handle_video_packet(addr, video_packet).await;
            }
            Packet::ClientPacket(ClientPacket::VideoList(video_list_packet)) => {
                state.handle_ping_answer(addr, video_list_packet).await;
            }
            _ => error!("Received unexpected packet from {}: {:?}", addr, packet),
        }
    }
}
