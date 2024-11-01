use anyhow::Context;
use std::env;
use std::net::{IpAddr, Ipv4Addr};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};

async fn fetch_neighbours() -> anyhow::Result<Vec<IpAddr>> {
    let mut bootstraper_stream = TcpStream::connect((Ipv4Addr::new(127, 0, 0, 2), common::BOOTSTRAPER_PORT))
        .await
        .context("Failed to connect to bootstraper")?;

    let packet = common::packet::NBPacket::RequestNeighbours;
    bootstraper_stream.write_all(&bincode::serialize(&packet)?).await?;

    let mut buf = [0u8; 1024];
    let n = bootstraper_stream.read(&mut buf).await?;
    if n == 0 {
        anyhow::bail!("No packet received");
    }

    let packet = bincode::deserialize(&buf[..n])?;

    match packet {
        common::packet::BNPacket::Neighbours(neighbours) => Ok(neighbours),
    }
}

struct State {
    neighbours: Vec<IpAddr>,
    udp_socket: UdpSocket,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder().filter_level(log::LevelFilter::Debug).init();

    let node_ip: IpAddr = env::args().nth(0)
        .map(|ip| ip.parse().ok())
        .flatten()
        .context("Failed to parse IP address")?;

    let neighbours = fetch_neighbours().await?;
    println!("Received neighbours: {:?}", neighbours);

    let udp_socket = UdpSocket::bind((node_ip, 0)).await?;

    let state = State {
        neighbours,
        udp_socket,
    };

    Ok(())
}
