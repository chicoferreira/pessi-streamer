use log::{debug, info};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};

#[tokio::main]
async fn main() {
    env_logger::builder().filter_level(log::LevelFilter::Debug).init();

    let address = "127.0.0.1:8080";

    info!("Connecting to {}", address);
    let mut stream = TcpStream::connect("127.0.0.1:8080").unwrap();
    info!("Connected to the server!");

    let mut ffplay = Command::new("ffplay")
        .args("-fflags nobuffer -analyzeduration 100000 -probesize 500000 -f mpegts -i -".split(' '))
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();

    let mut ffplay_stdin = ffplay.stdin.take().unwrap();

    let mut buf = vec![0u8; 65536];
    loop {
        let n = stream.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        debug!("Received {} bytes", n);
        ffplay_stdin.write_all(&buf[..n]).unwrap();
    }
}
