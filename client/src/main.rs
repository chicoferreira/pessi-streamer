use clap::Parser;
use common::packet::{CSPacket, SCPacket};
use log::trace;
use std::io::Write;
use std::net::Ipv4Addr;
use std::process::{Command, Stdio};
use tokio::net::UdpSocket;

/// A simple program to watch live streams
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Name of the stream to watch
    #[arg(short, long)]
    name: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    if args.name.is_empty() {
        return Ok(());
    }
    

    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    // Server
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await.unwrap();
    socket.connect((Ipv4Addr::LOCALHOST, common::PORT)).await.unwrap();

    let packet = CSPacket::RequestVideo(args.name.to_string());
    let packet = bincode::serialize(&packet).unwrap();

    socket.send(&packet).await.unwrap();

    let mut ffplay = Command::new("ffplay")
        .args(
            "-fflags nobuffer -analyzeduration 200000 -probesize 1000000 -f mpegts -i -".split(' '),
        )
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
                match ffplay_stdin.write_all(&data) {
                    Ok(_) => {
                        // Write was successful
                        ffplay_stdin.flush().unwrap();
                    }
                    Err(_e) => {
                        // Handle the error, e.g., log it, display a user-friendly message, etc.
                        println!("Stream was closed. Goodbye!");
                        return Ok(());
                    }
                }
            }
        }
    }
}
