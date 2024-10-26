mod video;

use crate::video::Video;
use common::{CSPacket, SCPacket};
use log::{debug, error, info, trace, warn};
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::net::UdpSocket;

struct Client {
    current_video: Option<String>,
    socket: SocketAddr,
}

struct State {
    clients: Vec<Client>,
    videos: HashMap<String, Video>,
}

impl State {
    fn register_video(&mut self, video: Video) {
        self.videos.insert(video.video_path.clone(), video);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder().filter_level(log::LevelFilter::Debug).init();
    ffmpeg_sidecar::download::auto_download().unwrap();

    info!("Starting server...");

    let mut state = State {
        clients: Vec::new(),
        videos: HashMap::new(),
    };

    let video = Video::new_video_task("video.mp4".to_string()).await.unwrap();
    state.register_video(video);

    let socket = UdpSocket::bind("0.0.0.0:8001").await.unwrap();
    info!("Waiting for client packets on {}", socket.local_addr().unwrap());

    handle_client_socket(socket).await?;

    Ok(())
}

async fn handle_client_socket(socket: UdpSocket) -> anyhow::Result<()> {
    let mut buf = [0u8; 16384];
    loop {
        let (n, addr) = socket.recv_from(&mut buf).await.unwrap();
        trace!("Received {} bytes from {}", n, addr);

        let buf = &buf[..n];

        let packet = bincode::deserialize(buf);
        let Ok(packet) = packet else {
            warn!("Client {} packet couldn't be deserialized: {:?}", addr, packet.err());
            continue;
        };

        match packet {
            CSPacket::Heartbeat => {
                debug!("Received heartbeat from {}", addr);
            }
            CSPacket::RequestVideo(video_path) => {
                debug!("Received request for video {} from {}", video_path, addr);

                // test sending video packet
                let packet = SCPacket::VideoPacket(vec![0, 0, 0]);
                let packet = &bincode::serialize(&packet).unwrap();
                socket.send_to(packet, addr).await.unwrap();
            }
            CSPacket::StopVideo(video_path) => {
                debug!("Received stop request for video {} from {}", video_path, addr);
            }
        }
    }
}
