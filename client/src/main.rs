use std::io::Read;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use anyhow::Context;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder().filter_level(log::LevelFilter::Debug).init();

    let address = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 8080);
    let mut stream = TcpStream::connect(address).context("Failed to connect to server")?;

    log::info!("Connected to server at {address}");

    let mut buffer = [0; 128];
    loop {
        stream.read(&mut buffer).context("Failed to read data from server")?;
        let message = String::from_utf8_lossy(&buffer);
        log::info!("Received: {}", message);
    }
}
