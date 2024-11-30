use crate::video;
use common::packet::{Packet, ServerPacket, VideoPacket};
use common::reliable::ReliableUdpSocket;
use dashmap::DashMap;
use log::{debug, error, info, trace};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicU8;
use std::sync::Arc;
use tokio::select;
use walkdir::WalkDir;

struct Video {
    /// The id of the video stream
    name: String,
    /// The path to the video file
    interested: Vec<SocketAddr>,
    sequence_number: u64,
}

#[derive(Clone)]
pub struct State {
    id: u64,
    /// Map of video paths to interested subscribers
    videos: Arc<DashMap<u8, Video>>,
    last_video_id: Arc<AtomicU8>,
    clients_socket: ReliableUdpSocket,
    neighbours: Arc<Vec<SocketAddr>>,
}

impl State {
    pub fn new(clients_socket: ReliableUdpSocket, id: u64, neighbours: Vec<SocketAddr>) -> Self {
        Self {
            id,
            clients_socket,
            videos: Arc::new(DashMap::new()),
            last_video_id: Arc::new(AtomicU8::new(0)),
            neighbours: Arc::new(neighbours),
        }
    }

    pub fn get_video_name_from_path(
        video_path: &PathBuf,
        video_folder: &PathBuf,
    ) -> Result<String, std::path::StripPrefixError> {
        Ok(video_path
            .with_extension("")
            .strip_prefix(video_folder)?
            .to_str()
            .unwrap()
            .to_string())
    }

    pub fn contains_video(&self, video_path: &PathBuf, video_folder: &PathBuf) -> bool {
        let Ok(video_name) = Self::get_video_name_from_path(video_path, video_folder) else {
            return false;
        };

        self.videos
            .iter()
            .any(|entry| entry.value().name == video_name)
    }

    fn get_next_video_id(&self) -> u8 {
        self.last_video_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    fn new_video(&self, name: String) -> Video {
        Video {
            name,
            interested: Vec::new(),
            sequence_number: 0,
        }
    }

    pub async fn start_streaming_video(
        self,
        video_path: PathBuf,
        video_folder: PathBuf,
    ) -> anyhow::Result<()> {
        let (stream_socket, mut child_process) =
            video::new_video_process(video_path.clone()).await?;
        let video_name = Self::get_video_name_from_path(&video_path, &video_folder)?;

        let id = self.get_next_video_id();
        self.videos.insert(id, self.new_video(video_name.clone()));

        let child_future = async { child_process.wait().await };

        tokio::pin!(child_future);

        let mut buf = [0u8; 65536];
        loop {
            select! {
                _ = &mut child_future => {
                    error!("Child process ended unexpectedly");
                    return Err(anyhow::anyhow!("Child process ended unexpectedly"));
                }
                packet = stream_socket.recv(&mut buf) => {
                    let n = match packet {
                        Ok(n) => n,
                        Err(e) => {
                            error!("Failed to receive packet: {}", e);
                            continue;
                        }
                    };

                    if let Some(mut video) = self.videos.get_mut(&id) {
                        if video.interested.is_empty() {
                            continue;
                        }

                        video.sequence_number += 1;

                        let stream_data = buf[..n].to_vec();

                        let packet = Packet::VideoPacket(VideoPacket {
                            stream_id: id,
                            sequence_number: video.sequence_number,
                            stream_data,
                        });

                        if let Err(e) = self
                            .clients_socket
                            .send_unreliable_broadcast(&packet, &video.interested)
                            .await
                        {
                            error!("Failed to send video packet: {}", e);
                        }

                        trace!(
                            "Sent video packet (video={}, seq={}, size={}) to {} subscribers",
                            id,
                            video.sequence_number,
                            n,
                            video.interested.len()
                        );
                    }
                }
            }
        }
    }

    pub fn get_video_list(&self) -> Vec<(u8, String)> {
        self.videos
            .iter()
            .map(|entry| (*entry.key(), entry.value().name.clone()))
            .collect()
    }
}

pub async fn run_client_socket(state: State) -> anyhow::Result<()> {
    let socket = state.clients_socket;
    info!("Waiting for client packets on {}", socket.local_addr()?);

    let mut buf = [0u8; 65536];
    loop {
        let (packet, socket_addr) = match socket.receive(&mut buf).await {
            Ok(Some(result)) => result,
            Ok(None) => {
                // Acknowledgement packet received
                debug!("Received acknowledgement packet");
                continue;
            }
            Err(e) => {
                error!("Failed to receive packet: {}", e);
                continue;
            }
        };

        match packet {
            Packet::ServerPacket(ServerPacket::RequestVideo(video_id)) => {
                info!(
                    "Received request to start video {} from {}",
                    video_id, socket_addr
                );
                if let Some(mut video) = state.videos.get_mut(&video_id) {
                    if !video.interested.contains(&socket_addr) {
                        video.interested.push(socket_addr);
                    }
                }
            }
            Packet::ServerPacket(ServerPacket::StopVideo(video_id)) => {
                info!(
                    "Received request to stop video {} from {}",
                    video_id, socket_addr
                );
                if let Some(mut subscribers) = state.videos.get_mut(&video_id) {
                    subscribers
                        .interested
                        .retain(|&subscriber| subscriber != socket_addr);
                }
            }
            _ => {
                error!(
                    "Received unexpected packet from {}: {:?}",
                    socket_addr, packet
                );
            }
        }
    }
}

fn get_files(dir: &PathBuf) -> Vec<PathBuf> {
    WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.path().to_path_buf())
        .collect()
}

pub async fn watch_video_folder(state: State) -> anyhow::Result<()> {
    let video_folder = PathBuf::from("./videos");

    loop {
        for video in get_files(&video_folder) {
            if !state.contains_video(&video, &video_folder) {
                info!("Starting streaming for video {:?}", video);
                let state = state.clone();
                tokio::spawn(state.start_streaming_video(video, video_folder.clone()));
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    }
}

pub mod flood {
    use crate::server::State;
    use common::packet::{FloodPacket, NodePacket, Packet};
    use log::info;
    use std::time::{Duration, SystemTime};

    pub async fn run_periodic_flood_packets(state: State) -> anyhow::Result<()> {
        info!("Starting periodic flood packets to neighbours");
        let mut sequence_number = 0;

        loop {
            let packet = Packet::NodePacket(NodePacket::FloodPacket(FloodPacket {
                sequence_number,
                hops: 0,
                created_at_server_time: SystemTime::now(),
                videos_available: state.get_video_list(),
                visited_nodes: vec![state.id],
                my_fathers: vec![],
            }));

            sequence_number += 1;

            state
                .clients_socket
                .send_unreliable_broadcast(&packet, &state.neighbours)
                .await?;
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}
