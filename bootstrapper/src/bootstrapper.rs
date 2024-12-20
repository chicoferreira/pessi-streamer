use common::packet::BootstrapperNeighboursResponse;
use log::{error, info};
use serde::Deserialize;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::net::{IpAddr, SocketAddr};
use std::{collections::HashMap, sync::Arc};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
};

#[derive(Deserialize)]
pub struct Neighbours {
    #[serde(flatten)]
    neighbours: HashMap<IpAddr, Vec<IpAddr>>,
}

impl Neighbours {
    pub fn get(&self, node: &IpAddr) -> Option<&Vec<IpAddr>> {
        self.neighbours.get(node)
    }

    pub fn len(&self) -> usize {
        self.neighbours.len()
    }
}

#[derive(Clone)]
pub struct State {
    /// Map of nodes to their neighbours
    neighbours: Arc<Neighbours>,
}

impl State {
    pub fn new(neighbours: Neighbours) -> Self {
        Self {
            neighbours: Arc::new(neighbours),
        }
    }
}

pub async fn run_server(state: State, socket: TcpListener) -> anyhow::Result<()> {
    info!("Waiting for connections on {}...", socket.local_addr()?);

    loop {
        let (stream, addr) = socket.accept().await?;
        info!("Accepted connection from {}", addr);

        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, addr, state).await {
                error!("Error handling client: {}", e);
            }
        });
    }
}

fn calculate_id(addr: SocketAddr) -> u64 {
    let mut hasher = DefaultHasher::new();
    addr.hash(&mut hasher);
    hasher.finish()
}

async fn handle_client(
    mut stream: TcpStream,
    addr: SocketAddr,
    state: State,
) -> anyhow::Result<()> {
    let neighbours = state
        .neighbours
        .get(&addr.ip())
        .cloned()
        .unwrap_or_default();

    info!("Sending neighbours list to {addr}: {neighbours:?}");

    let id = calculate_id(addr);
    let packet = BootstrapperNeighboursResponse { neighbours, id };

    match bincode::serialize(&packet) {
        Ok(bytes) => stream.write_all(&bytes).await?,
        Err(err) => error!("Error serializing packet: {}", err),
    };

    Ok(())
}
