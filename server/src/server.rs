use crate::video::VideoProcess;
use common::packet::{CSPacket, SCPacket};
use dashmap::DashMap;
use log::{debug, error, info, trace};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;

#[derive(Clone)]
pub struct State {
    /// Map of video paths to interested subscribers
    clients: Arc<DashMap<String, Vec<SocketAddr>>>,
    clients_socket: Arc<UdpSocket>,
    neighbours: Arc<Vec<SocketAddr>>,
}

impl State {
    pub fn new(clients_socket: UdpSocket, neighbours: Vec<SocketAddr>) -> Self {
        Self {
            clients_socket: Arc::new(clients_socket),
            clients: Arc::new(DashMap::new()),
            neighbours: Arc::new(neighbours),
        }
    }

    pub async fn start_streaming_video(&self, video_path: PathBuf, video_folder: &PathBuf) -> anyhow::Result<JoinHandle<anyhow::Result<()>>> {
        let video_process = VideoProcess::new_video_process(video_path.clone()).await?;
        let video_name = video_path.with_extension("")
            .strip_prefix(video_folder)?
            .to_str().unwrap().to_string();

        self.clients.insert(video_name.clone(), Vec::new());

        let state = self.clone();

        Ok(tokio::spawn(async move {
            let mut buf = [0u8; 65536];
            loop {
                if let Ok(n) = video_process.recv(&mut buf).await {
                    let clients_list = state.clients.get(&video_name).map(|v| v.clone()).unwrap_or_default();
                    let packet = SCPacket::VideoPacket(buf[..n].to_vec());

                    state.send_packets(packet, &clients_list).await;
                }
            }
        }))
    }

    async fn send_packets(&self, packet: SCPacket, addrs: &[SocketAddr]) {
        let packet = bincode::serialize(&packet).unwrap();
        for addr in addrs {
            self.clients_socket.send_to(&packet, addr).await.unwrap();
        }
    }

    pub fn get_video_list(&self) -> Vec<String> {
        self.clients.iter().map(|entry| entry.key().clone()).collect()
    }
}

pub async fn run_client_socket(state: State) -> anyhow::Result<()> {
    let socket = state.clients_socket;
    info!("Waiting for client packets on {}", socket.local_addr()?);

    let mut buf = [0u8; 16384];
    loop {
        let (n, addr) = match socket.recv_from(&mut buf).await {
            Ok(r) => r,
            Err(e) => {
                error!("Failed to receive packet: {}", e);
                continue;
            }
        };

        trace!("Received {} bytes from {}", n, addr);

        let packet = bincode::deserialize(&buf[..n]);
        match packet {
            Ok(CSPacket::Heartbeat) => debug!("Received heartbeat from {}", addr),
            Ok(CSPacket::RequestVideo(video_name)) => {
                info!("Received request to start video {}", video_name);
                state.clients
                    .entry(video_name)
                    .or_insert_with(Vec::new)
                    .push(addr);
            }
            Ok(CSPacket::StopVideo(video_path)) => {
                if let Some(mut subscribers) = state.clients.get_mut(&video_path) {
                    subscribers.retain(|&subscriber| subscriber != addr);
                }
            }
            Err(e) => error!("Failed to deserialize packet from client {:?}: {}", addr, e),
        }
    }
}


pub mod flood {
    use crate::server::State;
    use common::packet::SNCPacket;
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

            let packet = SNCPacket::FloodPacket {
                hops: 0,
                millis_created_at_server: now,
                videos_available: state.get_video_list(),
            };

            let Ok(packet) = bincode::serialize(&packet) else {
                error!("Failed to serialize flood packet");
                continue;
            };

            for addr in state.neighbours.iter() {
                debug!("Sending flood packet to neighbour {}", addr);
                if let Err(e) = state.clients_socket.send_to(&packet, addr).await {
                    error!("Failed to send flood packet to neighbour {}: {}", addr, e);
                }
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}
