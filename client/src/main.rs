use std::io::Write;
use common::{BNPacket, CSPacket, NBPacket, SCPacket};
use log::{info, trace};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::net::Ipv4Addr;
use std::process::{Command, Stdio};
use tokio::net::{TcpStream, UdpSocket};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder().filter_level(log::LevelFilter::Debug).init();

    // Bootstraper
    let mut stream = TcpStream::connect(("localhost", common::BOOTSTRAPER_PORT))
        .await
        .expect("Failed to connect to bootstraper. Try running it first.");

    let packet = NBPacket::RequestNeighbours;
    let packet = bincode::serialize(&packet).unwrap();

    stream.write_all(&packet).await.unwrap();

    let mut buf = [0u8; 16384];
    let n = stream.read(&mut buf).await.unwrap();
    let packet: BNPacket = bincode::deserialize(&buf[..n]).unwrap();

    match packet {
        BNPacket::Neighbours(neighbours) => {
            info!("Received neighbours: {:?}", neighbours);
        }
    }

    // Server
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await.unwrap();
    socket.connect((Ipv4Addr::LOCALHOST, common::SERVER_PORT)).await.unwrap();

    let packet = CSPacket::RequestVideo("video.mp4".to_string());
    let packet = bincode::serialize(&packet).unwrap();

    socket.send(&packet).await.unwrap();

    let mut ffplay = Command::new("ffplay")
        .args("-fflags nobuffer -analyzeduration 200000 -probesize 1000000 -f mpegts -i -".split(' '))
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();

    let mut ffplay_stdin = ffplay.stdin.take().unwrap();

    let mut buf = [0u8; 16384];
    loop {
        let n = socket.recv(&mut buf).await.unwrap();
        let packet: SCPacket = bincode::deserialize(&buf[..n]).unwrap();

        match packet {
            SCPacket::VideoPacket(data) => {
                trace!("Received video packet with {} bytes", data.len());
                ffplay_stdin.write_all(&data).unwrap();
                ffplay_stdin.flush().unwrap();
            }
        }
    }
}
