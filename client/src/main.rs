mod monitor;
mod ui;
mod video;

use crate::video::{VideoPlayer, VideoPlayerType};
use anyhow::Context;
use circular_buffer::CircularBuffer;
use clap::Parser;
use common::packet::{ClientPacket, NodePacket, Packet, ServerPacket};
use dashmap::DashMap;
use log::{debug, error, info, trace, warn};
use std::cmp;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::AtomicU64;
use std::sync::{atomic, Arc, RwLock};
use std::time::{Duration, SystemTime};

/// A simple program to watch live streams
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Name of the stream to watch
    #[arg(long)]
    stream: Option<String>,

    /// Possible servers to connect to
    #[arg(short, long)]
    servers: Vec<IpAddr>,

    /// If the UI should be displayed
    #[arg(short, long, default_value_t = false)]
    no_ui: bool,

    /// The video player to use
    #[arg(short, long, default_value = "mpv")]
    video_player: VideoPlayerType,
}

#[derive(Default)]
struct Metrics {
    latest_rtts: CircularBuffer<10, Duration>,
}

impl Metrics {
    fn add_rtt(&mut self, rtt: Duration) {
        self.latest_rtts.push_back(rtt);
    }

    fn average_rtt(&self) -> Duration {
        self.latest_rtts.iter().sum::<Duration>() / self.latest_rtts.len() as u32
    }
}

#[derive(Default)]
struct NodeConnection {
    metrics: Metrics,
}

struct Node {
    addr: SocketAddr,
    connection: Option<NodeConnection>,
}

impl Node {
    fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            connection: None,
        }
    }

    fn average_rtt(&self) -> Option<Duration> {
        self.connection
            .as_ref()
            .map(|conn| conn.metrics.average_rtt())
    }
}

struct PlayingVideo {
    video_id: u8,
    source: SocketAddr,
    video_player: VideoPlayer,
    last_sequence_number: u64,
}

#[derive(Clone)]
struct State {
    /// The UDP socket to communicate with the nodes
    socket: common::reliable::ReliableUdpSocket,
    /// The list of possible servers to connect to
    servers: Arc<DashMap<SocketAddr, Node>>,
    /// The list of pending pings to the nodes
    pending_pings: Arc<DashMap<u64, (SocketAddr, SystemTime)>>,
    /// The current sequence number to send ClientPing packets
    ping_sequence_number: Arc<AtomicU64>,
    /// The list of possible streams received by the nodes (stream_id, stream_name)
    video_list: Arc<RwLock<Vec<(u8, String)>>>,
    /// The list of currently playing video processes
    playing_videos: Arc<DashMap<u8, PlayingVideo>>,
    /// The video player type to play the video
    video_player: VideoPlayerType,
}

impl State {
    fn new(
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
            servers: Arc::new(servers),
            pending_pings: Arc::new(Default::default()),
            ping_sequence_number: Arc::new(AtomicU64::new(0)),
            video_list: Arc::new(Default::default()),
            playing_videos: Arc::new(Default::default()),
            video_player: video_player_type,
        }
    }

    async fn wait_for_best_server_and_video_list(&self, _video_id: u8) -> SocketAddr {
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if let Some(best_node_addr) = self.select_best_node() {
                return best_node_addr;
            }
        }
    }

    async fn request_start_video(&self, addr: SocketAddr, video_id: u8) -> anyhow::Result<()> {
        let packet = Packet::ServerPacket(ServerPacket::RequestVideo(video_id));
        self.socket
            .send_reliable(&packet, addr)
            .await
            .context("Couldn't send request video packet")?;

        Ok(())
    }

    async fn request_stop_video(&self, addr: SocketAddr, video_id: u8) -> anyhow::Result<()> {
        let packet = Packet::ServerPacket(ServerPacket::StopVideo(video_id));
        self.socket
            .send_reliable(&packet, addr)
            .await
            .context("Couldn't send stop video packet")?;

        Ok(())
    }

    pub async fn start_playing(&self, video_id: u8) {
        let state = self.clone();
        let best_node_addr = state.wait_for_best_server_and_video_list(video_id).await;
        info!("Selected server {} for video {}", best_node_addr, video_id);

        if let Err(e) = state.request_start_video(best_node_addr, video_id).await {
            error!("Failed to request video to {}: {}", best_node_addr, e);
        } else if let Err(e) = state.start_video_process(best_node_addr, video_id) {
            error!("Failed to start video process: {}", e);
        }
    }

    pub async fn stop_playing(&self, video: &mut PlayingVideo) {
        let state = self.clone();
        let addr = video.source;

        if let Err(e) = state.request_stop_video(addr, video.video_id).await {
            error!("Failed to request video to {}: {}", addr, e);
        } else if let Err(e) = video.video_player.kill().await {
            error!("Failed to start video process: {}", e);
        }
    }

    pub async fn stop_playing_id(&self, video_id: u8) {
        if let Some((_, mut video)) = self.playing_videos.remove(&video_id) {
            self.stop_playing(&mut video).await;
        }
    }

    pub fn start_playing_sync(&self, video_id: u8) {
        let state = self.clone();
        tokio::spawn(async move {
            state.start_playing(video_id).await;
        });
    }

    pub fn stop_playing_id_sync(&self, video_id: u8) {
        let state = self.clone();
        tokio::spawn(async move {
            state.stop_playing_id(video_id).await;
        });
    }

    fn handle_ping_answer(
        &self,
        sequence_number: u64,
        videos: Vec<(u8, String)>,
        answer_creation_date: SystemTime,
    ) {
        self.set_video_list(videos);

        if let Some((_, (addr, start_time))) = self.pending_pings.remove(&sequence_number) {
            let now = SystemTime::now();

            let to = answer_creation_date
                .duration_since(start_time)
                .unwrap_or(Duration::from_secs(0));

            let from = now
                .duration_since(answer_creation_date)
                .unwrap_or(Duration::from_secs(0));

            let total_rtt = now
                .duration_since(start_time)
                .unwrap_or(Duration::from_secs(0));
            debug!(
                "Received ping answer from {} in {:?} (TO: {:?}, FROM: {:?})",
                addr, total_rtt, to, from
            );
            if let Some(mut node) = self.servers.get_mut(&addr) {
                node.connection
                    .get_or_insert(Default::default())
                    .metrics
                    .add_rtt(total_rtt);
            }
        }
    }

    fn select_best_node(&self) -> Option<SocketAddr> {
        self.servers
            .iter()
            .filter_map(|node| node.average_rtt().map(|score| (node.addr, score)))
            .min_by(|(_, a_score), (_, b_score)| {
                a_score.partial_cmp(b_score).unwrap_or(cmp::Ordering::Equal)
            })
            .map(|(addr, _)| addr)
    }

    fn set_video_list(&self, video_list: Vec<(u8, String)>) {
        *self.video_list.write().unwrap() = video_list;
    }

    fn start_video_process(&self, source: SocketAddr, video_id: u8) -> anyhow::Result<()> {
        let video_player = VideoPlayer::launch(self.video_player)?;
        self.playing_videos.insert(
            video_id,
            PlayingVideo {
                video_id,
                source,
                video_player,
                last_sequence_number: 0,
            },
        );
        Ok(())
    }
}

async fn start_ping_nodes_task(state: State) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        for node in state.servers.iter() {
            let sequence_number = state
                .ping_sequence_number
                .fetch_add(1, atomic::Ordering::Relaxed);
            let packet = Packet::NodePacket(NodePacket::ClientPing { sequence_number });
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

async fn handle_packet_task(state: State) {
    let mut buf = [0u8; 65536];
    loop {
        let result = state.socket.receive(&mut buf).await;
        let (packet, addr) = match result {
            Ok(Some((packet, addr))) => (packet, addr),
            Ok(None) => continue,
            Err(e) => {
                error!("Failed to receive packet: {}", e);
                continue;
            }
        };
        match packet {
            Packet::VideoPacket {
                stream_id,
                sequence_number,
                stream_data,
            } => {
                if let Some(mut video) = state.playing_videos.get_mut(&stream_id) {
                    trace!(
                        "Received video packet for stream {} (SEQ={}, DATA_LEN={})",
                        stream_id,
                        sequence_number,
                        stream_data.len()
                    );
                    if sequence_number != video.last_sequence_number + 1 {
                        warn!(
                            "Received video packet out of order (LAST={} NEW={}) for stream {}",
                            video.last_sequence_number, sequence_number, stream_id
                        );
                    }
                    video.last_sequence_number = sequence_number;

                    if let Err(e) = video.video_player.write(&stream_data).await {
                        error!("Failed to write video packet: {}", e);
                        state.stop_playing(&mut video).await;
                        drop(video);
                        state.playing_videos.remove(&stream_id);
                    }
                } else {
                    warn!("Received video packet for unwanted stream {}", stream_id);
                }
            }
            Packet::ClientPacket(ClientPacket::VideoList {
                sequence_number,
                videos,
                answer_creation_date,
            }) => {
                state.handle_ping_answer(sequence_number, videos, answer_creation_date);
            }
            _ => {
                error!("Received unexpected packet from {}: {:?}", addr, packet)
            }
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    let args = Args::parse();

    if !args.video_player.check_installation().await {
        panic!("Video player {:?} is not installed", args.video_player);
    }

    let addr = (Ipv4Addr::new(127, 0, 0, 2), 0).into();
    let socket = common::reliable::ReliableUdpSocket::new(addr).await?;

    let state = State::new(socket.clone(), args.servers, args.video_player);

    tokio::spawn(start_ping_nodes_task(state.clone()));
    tokio::spawn(handle_packet_task(state.clone()));

    if !args.no_ui {
        if let Err(e) = ui::run_ui(state) {
            error!("Failed to run UI: {}", e);
        }
    }

    Ok(())
}
