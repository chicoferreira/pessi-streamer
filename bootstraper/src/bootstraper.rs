use common::{BNPacket, NBPacket};
use log::{info, trace};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::{TcpListener, TcpStream}};
use std::{collections::HashMap, net::Ipv4Addr, sync::{Arc, RwLock}};

#[derive(Clone)]
pub struct State {
    /// Map of nodes to their neighbours
    neighbours: Arc<RwLock<HashMap<String, Vec<Ipv4Addr>>>>,
    server_socket: Arc<TcpListener>,
}

impl State {
    pub fn new(server_socket: TcpListener) -> Self {
        Self {
            server_socket: Arc::new(server_socket),
            neighbours: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

pub async fn run_server(state: State) -> anyhow::Result<()> {
    let socket = &state.server_socket;
    info!("Waiting for connections on {}", socket.local_addr()?);

    loop {
        let (stream, addr) = socket.accept().await?;
        info!("Accepted connection from {}", addr);

        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, state).await {
                eprintln!("Error handling client: {}", e);
            }
        });
    }
}

async fn handle_client(mut stream: TcpStream, state: State) -> anyhow::Result<()> {
    let mut buf = [0u8; 65536];
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 { 
            trace!("Connection from {} closed", stream.peer_addr()?);
            break;
        }

        let packet: NBPacket = bincode::deserialize(&buf[..n])?;
        match packet {
            NBPacket::RequestNeighbours => {
                info!("Received request for neighbours from {}", stream.peer_addr()?);

                let neighbours_list = state.neighbours.read().unwrap().get(&stream.peer_addr()?.to_string()).cloned().unwrap_or_default();
                let packet: BNPacket = BNPacket::Neighbours(neighbours_list);
                let packet = bincode::serialize(&packet)?;

                stream.write_all(&packet).await?;
            }
        }
    }

    Ok(())
}
