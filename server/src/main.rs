use std::net::{Ipv4Addr, SocketAddr};
use anyhow::Context;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

struct State {
    data: Arc<String>,
}

impl Clone for State {
    fn clone(&self) -> Self {
        Self { data: self.data.clone() }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder().filter_level(log::LevelFilter::Debug).init();

    let state = State {
        data: Arc::new("Hello, world!".to_string()),
    };

    let address = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 8080);
    let listener = TcpListener::bind(address).await.context("Failed to bind to address")?;
    
    log::info!("Listening on {}", address);
    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                log::info!("Accepted connection from {}", addr);
                let state = state.clone();
                tokio::spawn(async move {
                    handle_connection(stream, state.clone()).await;
                });
            }
            Err(err) => {
                log::error!("Failed to accept connection: {}", err);
            }
        }
    }
}

async fn handle_connection(mut stream: TcpStream, state: State) {
    loop {
        if let Err(e) = stream.write_all(state.data.as_bytes()).await {
            log::error!("Failed to write to stream: {}", e);
            return;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
