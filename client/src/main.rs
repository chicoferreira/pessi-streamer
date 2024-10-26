use common::{CSPacket, SCPacket};
use log::info;
use std::net::Ipv4Addr;
use tokio::net::UdpSocket;

#[tokio::main]
async fn main() {
    env_logger::builder().filter_level(log::LevelFilter::Debug).init();

    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await.unwrap();
    socket.connect((Ipv4Addr::LOCALHOST, 8001)).await.unwrap();

    let packet = CSPacket::RequestVideo("video.mp4".to_string());
    let packet = bincode::serialize(&packet).unwrap();

    socket.send(&packet).await.unwrap();

    let mut buf = [0u8; 16384];
    loop {
        let n = socket.recv(&mut buf).await.unwrap();
        let packet: SCPacket = bincode::deserialize(&buf[..n]).unwrap();

        match packet {
            SCPacket::VideoPacket(data) => {
                info!("Received video packet with {} bytes", data.len());
            }
        }
    }
}
