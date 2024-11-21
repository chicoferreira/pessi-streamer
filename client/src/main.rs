mod video;
mod monitor;
mod ui;

use crate::video::VideoPlayer;
use anyhow::Context;
use circular_buffer::CircularBuffer;
use clap::Parser;
use common::packet::{ClientPacket, NodePacket, ServerPacket};
use dashmap::DashMap;
use log::{error, info, trace, warn};
use std::cmp;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::AtomicU64;
use std::sync::{atomic, Arc, Mutex, RwLock};
use tokio::net::UdpSocket;
use tokio::time::Instant;

/// A simple program to watch live streams
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Name of the stream to watch
    #[arg(long)]
    stream: String, // Todo: make this optional when ui is true
    /// Possible servers to connect to
    #[arg(short, long)]
    servers: Vec<IpAddr>,
    /// Open program ui
    #[arg(short, long, default_value_t = true)]
    ui: bool,
}

#[derive(Default)]
struct Metrics {
    latest_rtts: CircularBuffer<10, u128>,
}

impl Metrics {
    fn new() -> Self {
        Self {
            latest_rtts: CircularBuffer::new(),
        }
    }

    fn add_rtt(&mut self, rtt: u128) {
        self.latest_rtts.push_back(rtt);
    }

    fn average_rtt(&self) -> f64 {
        self.latest_rtts.iter().sum::<u128>() as f64 / self.latest_rtts.len() as f64
    }
}

#[derive(Default)]
struct NodeConnection {
    metrics: Metrics,
}

#[derive(Clone)]
struct Node {
    addr: SocketAddr,
    connection: Arc<Mutex<Option<NodeConnection>>>,
    pending_pings: Arc<DashMap<u64, Instant>>,
}

impl Node {
    fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            connection: Arc::new(Mutex::new(None)),
            pending_pings: Arc::new(DashMap::new()),
        }
    }

    async fn handle_ping_answer(&mut self, sequence_number: u64) {
        let now = Instant::now();

        if let Some((_, start_time)) = self.pending_pings.remove(&sequence_number) {
            let rtt = now.duration_since(start_time).as_millis();
            self.connection.lock().unwrap().get_or_insert(Default::default()).metrics.add_rtt(rtt);
        }
    }

    fn get_score(&self) -> Option<f64> {
        self.connection.lock().unwrap().as_ref().map(|conn| conn.metrics.average_rtt())
    }
}

#[derive(Clone)]
struct State {
    /// The UDP socket to communicate with the nodes
    socket: common::reliable::ReliableUdpSocket,
    /// The list of possible servers to connect to
    servers: Arc<DashMap<SocketAddr, Node>>,
    /// The list of pending pings to the nodes
    pending_pings: Arc<DashMap<u64, (SocketAddr, Instant)>>,
    /// The current sequence number to send ClientPing packets
    ping_sequence_number: Arc<AtomicU64>,
    /// The list of possible streams received by the nodes (stream_id, stream_name)
    video_list: Arc<RwLock<Option<Vec<(u8, String)>>>>,
    /// The list of currently playing video processes
    video_processes: Arc<DashMap<u8, VideoPlayer>>,
}

impl State {
    fn new(socket: common::reliable::ReliableUdpSocket, servers: Vec<IpAddr>) -> Self {
        let servers = servers.into_iter().map(|addr| {
            let socket_addr = SocketAddr::new(addr, common::PORT);
            (socket_addr, Node::new(socket_addr))
        }).collect();

        Self {
            socket,
            servers: Arc::new(servers),
            pending_pings: Arc::new(Default::default()),
            ping_sequence_number: Arc::new(AtomicU64::new(0)),
            video_list: Arc::new(Default::default()),
            video_processes: Arc::new(Default::default()),
        }
    }

    fn handle_ping_answer(&self, sequence_number: u64) {
        let now = Instant::now();

        if let Some((_, (addr, start_time))) = self.pending_pings.remove(&sequence_number) {
            let rtt = now.duration_since(start_time).as_millis();
            if let Some(node) = self.servers.get(&addr) {
                node.connection.lock().unwrap().get_or_insert(Default::default()).metrics.add_rtt(rtt);
            }
        }
    }

    fn select_best_node(&self) -> Option<SocketAddr> {
        self.servers.iter()
            .filter_map(|node| node.get_score().map(|score| (node.addr, score)))
            .min_by(|(_, a_score), (_, b_score)| a_score.partial_cmp(b_score).unwrap_or(cmp::Ordering::Equal))
            .map(|(addr, _)| addr)
    }

    fn set_video_list(&self, video_list: Vec<(u8, String)>) {
        *self.video_list.write().unwrap() = Some(video_list);
    }

    fn get_video_id(&self, name: &str) -> Option<u8> {
        self.video_list
            .read()
            .unwrap()
            .as_ref()
            .and_then(|list| list.iter().find(|(_, stream_name)| stream_name == name)
                .map(|(id, _)| *id))
    }

    fn start_video_process(&self, video_id: u8) -> anyhow::Result<()> {
        self.video_processes.insert(video_id, VideoPlayer::launch()?);
        Ok(())
    }
}

async fn start_ping_nodes_task(state: State) {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        for node in state.servers.iter() {
            let sequence_number = state.ping_sequence_number.fetch_add(1, atomic::Ordering::Relaxed);
            let packet = NodePacket::ClientPing { sequence_number };
            if let Err(e) = state.socket.send_reliable(&packet, node.addr).await {
                error!("Failed to send ping to {}: {}", node.addr, e);
            } else {
                state.pending_pings.insert(sequence_number, (node.addr, Instant::now()));
            }
        }
    }
}

async fn handle_packet_task(state: State) {
    let mut buf = [0u8; 16384];
    loop {
        match state.socket.receive(&mut buf).await {
            Ok(Some((ClientPacket::VideoPacket { stream_id, stream_data }, addr))) => {
                trace!("Received video packet from {}", addr);
                if let Some(mut video_process) = state.video_processes.get_mut(&stream_id) {
                    if let Err(e) = video_process.write(&stream_data).await {
                        error!("Failed to write video packet: {}", e);
                    }
                } else {
                    warn!("Received video packet for unwanted stream {}", stream_id);
                }
            }
            Ok(Some((ClientPacket::VideoList { sequence_number, videos }, addr))) => {
                info!("Received video list from {}", addr);
                state.handle_ping_answer(sequence_number);
                state.set_video_list(videos);
            }
            Ok(None) => continue,
            Err(e) => {
                error!("Failed to receive packet: {}", e);
                continue;
            }
        }
    }
}

async fn wait_for_best_server_and_video_list(video_stream: &str, state: &State) -> (u8, SocketAddr) {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        if let Some(video_id) = state.get_video_id(video_stream) {
            if let Some(best_node_addr) = state.select_best_node() {
                return (video_id, best_node_addr);
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

    let socket = UdpSocket::bind((Ipv4Addr::new(127, 0, 0, 2), 0)).await.context("Failed to bind UDP socket")?;
    let socket = common::reliable::ReliableUdpSocket::new(socket);

    let state = State::new(socket.clone(), args.servers);

    tokio::spawn(start_ping_nodes_task(state.clone()));
    tokio::spawn(handle_packet_task(state.clone()));

    let state_clone = state.clone();
    tokio::spawn(async move {
        let (video_id, best_node_addr) = wait_for_best_server_and_video_list(&args.stream, &state_clone).await;
        info!("Selected server {} for video {}", best_node_addr, video_id);

        if let Err(e) = state_clone.socket.send_reliable(&NodePacket::RedirectToServer(ServerPacket::RequestVideo(video_id)), best_node_addr).await {
            error!("Failed to send request video to {}: {}", best_node_addr, e);
        } else if let Err(e) = state_clone.start_video_process(video_id) {
            error!("Failed to start video process: {}", e);
        }
    });

    if args.ui {
        if let Err(e) = ui::run_ui(state) {
            error!("Failed to run UI: {}", e);
        }
    }

    Ok(())
}

