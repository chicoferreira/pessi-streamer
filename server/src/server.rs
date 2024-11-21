use crate::video::VideoProcess;
use common::packet::{NodePacket, ServerPacket};
use common::reliable::ReliableUdpSocket;
use dashmap::DashMap;
use log::{debug, error, info};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicU8;
use std::sync::Arc;
use tokio::task::JoinHandle;

struct Video {
    /// The id of the video stream
    name: String,
    /// The path to the video file
    interested: Vec<SocketAddr>,
}

#[derive(Clone)]
pub struct State {
    /// Map of video paths to interested subscribers
    videos: Arc<DashMap<u8, Video>>,
    last_video_id: Arc<AtomicU8>,
    clients_socket: ReliableUdpSocket,
    neighbours: Arc<Vec<SocketAddr>>,
}

impl State {
    pub fn new(clients_socket: ReliableUdpSocket, neighbours: Vec<SocketAddr>) -> Self {
        Self {
            clients_socket,
            videos: Arc::new(DashMap::new()),
            last_video_id: Arc::new(AtomicU8::new(0)),
            neighbours: Arc::new(neighbours),
        }
    }

    fn get_next_video_id(&self) -> u8 {
        self.last_video_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    fn new_video(&self, name: String) -> Video {
        Video {
            name,
            interested: Vec::new(),
        }
    }

    pub async fn start_streaming_video(&self, video_path: PathBuf, video_folder: &PathBuf) -> anyhow::Result<JoinHandle<anyhow::Result<()>>> {
        let video_process = VideoProcess::new_video_process(video_path.clone()).await?;
        let video_name = video_path.with_extension("")
            .strip_prefix(video_folder)?
            .to_str().unwrap().to_string();

        let id = self.get_next_video_id();
        self.videos.insert(id, self.new_video(video_name.clone()));

        let state = self.clone();

        Ok(tokio::spawn(async move {
            let mut buf = [0u8; 65536];
            loop {
                if let Ok(n) = video_process.recv(&mut buf).await {
                    if let Some(video) = state.videos.get(&id) {
                        if video.interested.is_empty() {
                            continue;
                        }

                        let stream_data = buf[..n].to_vec();

                        let packet = NodePacket::VideoPacket {
                            stream_id: id,
                            stream_data,
                        };

                        if let Err(e) = state.clients_socket.send_unreliable_broadcast(&packet, &video.interested).await {
                            error!("Failed to send video packet: {}", e);
                        }
                    }
                }
            }
        }))
    }

    pub fn get_video_list(&self) -> Vec<(u8, String)> {
        self.videos.iter().map(|entry| (*entry.key(), entry.value().name.clone())).collect()
    }
}

pub async fn run_client_socket(state: State) -> anyhow::Result<()> {
    let socket = state.clients_socket;
    info!("Waiting for client packets on {}", socket.local_addr()?);

    let mut buf = [0u8; 16384];
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
            ServerPacket::RequestVideo(video_id) => {
                info!("Received request to start video {}", video_id);
                if let Some(mut video) = state.videos.get_mut(&video_id) {
                    video.interested.push(socket_addr);
                }
            }
            ServerPacket::StopVideo(video_id) => {
                if let Some(mut subscribers) = state.videos.get_mut(&video_id) {
                    subscribers.interested.retain(|&subscriber| subscriber != socket_addr);
                }
            }
        }
    }
}


pub mod flood {
    use crate::server::State;
    use common::packet::NodePacket;
    use log::{debug, error, info};
    use std::time::{Duration, SystemTimeError};

    fn get_current_millis() -> Result<u128, SystemTimeError> {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
    }

    pub async fn run_periodic_flood_packets(state: State) -> anyhow::Result<()> {
        info!("Starting periodic flood packets to neighbours");

        loop {
            let Ok(now) = get_current_millis() else {
                error!("Failed to get current time. Clock may have gone backwards.");
                continue;
            };

            let packet = NodePacket::FloodPacket {
                hops: 0,
                millis_created_at_server: now,
                videos_available: state.get_video_list(),
            };

            state.clients_socket.send_unreliable_broadcast(&packet, &state.neighbours).await?;
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}
