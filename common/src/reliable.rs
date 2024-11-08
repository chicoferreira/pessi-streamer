use dashmap::DashMap;
use log::error;
use serde::{Deserialize, Serialize};
use std::io;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

const MAX_RETRIES: u8 = 5;
const TIMEOUT: Duration = Duration::from_millis(200);

pub struct ReliableUdpSocket<T>
where
    T: Serialize + for<'de> Deserialize<'de> + Send + 'static,
{
    socket: Arc<UdpSocket>,
    pending_packets: Arc<DashMap<u64, PendingPacket>>,
    sequence_number: Arc<Mutex<u64>>,
    phantom_data: PhantomData<T>,
}

struct PendingPacket {
    data: Vec<u8>,
    destination: std::net::SocketAddr,
    timestamp: Instant,
    retries: u8,
}

/// Protocol packet
#[derive(Serialize, Deserialize, Debug)]
#[serde(bound(serialize = "T: Serialize", deserialize = "T: Deserialize<'de>"))]
enum ProtocolPacket<T> {
    Unreliable(T),
    Reliable {
        packet_id: u64,
        payload: T,
    },
    Ack {
        packet_id: u64,
    },
}

#[derive(Debug, Error)]
pub enum ReliableUdpSocketError {
    #[error("Failed to receive packet")]
    IoError(#[from] io::Error),
    #[error("Failed to serialize packet")]
    BincodeError(#[from] bincode::Error),
}

pub type Result<T> = std::result::Result<T, ReliableUdpSocketError>;

impl<T> ReliableUdpSocket<T>
where
    T: Serialize + for<'de> Deserialize<'de> + Send + 'static,
{
    pub fn new(socket: UdpSocket) -> Self {
        Self {
            socket: Arc::new(socket),
            pending_packets: Arc::new(DashMap::new()),
            sequence_number: Arc::new(Mutex::new(0)),
            phantom_data: PhantomData,
        }
    }

    pub async fn send_reliable(&self, payload: T, addr: std::net::SocketAddr) -> Result<()> {
        let mut seq_num = self.sequence_number.lock().await;
        *seq_num += 1;
        let packet_id = *seq_num;

        let packet = ProtocolPacket::Reliable {
            packet_id,
            payload,
        };

        let data = bincode::serialize(&packet)?;
        self.socket.send_to(&data, addr).await?;

        let pending_packet = PendingPacket {
            data: data.clone(),
            destination: addr,
            timestamp: Instant::now(),
            retries: 0,
        };

        self.pending_packets.insert(packet_id, pending_packet);

        self.handle_retransmission(packet_id);

        Ok(())
    }

    pub async fn send_unreliable(&self, payload: T, addr: std::net::SocketAddr) -> Result<()> {
        let packet = ProtocolPacket::Unreliable(payload);
        let data = bincode::serialize(&packet)?;
        self.socket.send_to(&data, addr).await?;

        Ok(())
    }

    pub async fn receive(&self) -> Result<Option<(T, std::net::SocketAddr)>> {
        let mut buf = [0u8; 65536];
        let (size, addr) = self.socket.recv_from(&mut buf).await?;
        let packet: ProtocolPacket<T> = bincode::deserialize(&buf[..size])?;

        match packet {
            ProtocolPacket::Ack { packet_id } => {
                self.pending_packets.remove(&packet_id);
                Ok(None)
            }
            ProtocolPacket::Reliable { packet_id, payload } => {
                let ack_packet = ProtocolPacket::<T>::Ack { packet_id };
                let ack_data = bincode::serialize(&ack_packet)?;
                self.socket.send_to(&ack_data, addr).await?;

                Ok(Some((payload, addr)))
            }
            ProtocolPacket::Unreliable(payload) => {
                Ok(Some((payload, addr)))
            }
        }
    }

    fn handle_retransmission(&self, packet_id: u64) {
        let pending_packets = self.pending_packets.clone();
        let socket = self.socket.clone();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(TIMEOUT).await;

                if let Some(mut pending_packet_entry) = pending_packets.get_mut(&packet_id) {
                    let pending_packet = pending_packet_entry.value_mut();

                    if pending_packet.retries >= MAX_RETRIES {
                        error!("Failed to deliver packet {} after {MAX_RETRIES} to {}.", packet_id, pending_packet.destination);
                        drop(pending_packet_entry);
                        pending_packets.remove(&packet_id);
                        break;
                    } else if pending_packet.timestamp.elapsed() >= TIMEOUT {
                        if let Err(e) = socket.send_to(&pending_packet.data, pending_packet.destination).await {
                            error!("Failed to deliver retransmission packet {} to {}: {}", packet_id, pending_packet.destination, e);
                            break;
                        } else {
                            pending_packet.timestamp = Instant::now();
                            pending_packet.retries += 1;
                        }
                    }
                } else {
                    break; // Packet was acknowledged
                }
            }
        });
    }
}