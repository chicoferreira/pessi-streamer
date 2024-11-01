use crate::video::VideoProcess;
use common::packet::{CSPacket, SCPacket};
use log::{debug, info, trace};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;

#[derive(Clone)]
pub struct State {
    /// Map of video paths to interested subscribers
    clients: Arc<RwLock<HashMap<String, Vec<SocketAddr>>>>,
    clients_socket: Arc<UdpSocket>,
}

impl State {
    pub fn new(clients_socket: UdpSocket) -> Self {
        Self {
            clients_socket: Arc::new(clients_socket),
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start_streaming_video(&self, video_path: String) -> anyhow::Result<JoinHandle<anyhow::Result<()>>> {
        let video_process = VideoProcess::new_video_process(&video_path).await?;
        self.clients.write().unwrap().insert(video_path.clone(), Vec::new());

        let state = self.clone();

        Ok(tokio::spawn(async move {
            let mut buf = [0u8; 65536];
            loop {
                if let Ok(n) = video_process.recv(&mut buf).await {
                    let clients_list = state.clients.read().unwrap().get(&video_path).cloned().unwrap_or_default();
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
}

pub async fn run_client_socket(state: State) -> anyhow::Result<()> {
    let socket = state.clients_socket;
    info!("Waiting for client packets on {}", socket.local_addr()?);

    let mut buf = [0u8; 16384];
    loop {
        let (n, addr) = socket.recv_from(&mut buf).await?;
        trace!("Received {} bytes from {}", n, addr);

        let packet: CSPacket = bincode::deserialize(&buf[..n])?;
        match packet {
            CSPacket::Heartbeat => debug!("Received heartbeat from {}", addr),
            CSPacket::RequestVideo(video_path) => {
                info!("Received request to start video {}", video_path);
                state.clients.write().unwrap()
                    .entry(video_path)
                    .or_insert_with(Vec::new)
                    .push(addr);
            }
            CSPacket::StopVideo(video_path) => {
                if let Some(subscribers) = state.clients.write().unwrap().get_mut(&video_path) {
                    subscribers.retain(|&subscriber| subscriber != addr);
                }
            }
        }
    }
}
