use dashmap::DashMap;
use log::error;
use serde::{Deserialize, Serialize};
use std::io;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::net::UdpSocket;

const MAX_RETRIES: u8 = 5;
const TIMEOUT: Duration = Duration::from_millis(200);

#[derive(Clone)]
pub struct ReliableUdpSocket {
    socket: Arc<UdpSocket>,
    pending_packets: Arc<DashMap<u64, PendingPacket>>,
    sequence_number: Arc<AtomicU64>,
}

impl ReliableUdpSocket {
    pub fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.socket.local_addr()
    }
}

struct PendingPacket {
    data: Vec<u8>,
    destination: std::net::SocketAddr,
    timestamp: Instant,
    retries: u8,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(bound(serialize = "T: Serialize", deserialize = "T: Deserialize<'de>"))]
enum UdpPacket<T> {
    Unreliable(T),
    Reliable { packet_id: u64, payload: T },
    Ack { packet_id: u64 },
}

#[derive(Debug, Error)]
pub enum ReliableUdpSocketError {
    #[error(transparent)]
    IoError(#[from] io::Error),
    #[error(transparent)]
    BincodeError(#[from] bincode::Error),
}

pub type Result<T> = std::result::Result<T, ReliableUdpSocketError>;

impl ReliableUdpSocket {
    pub fn new(socket: UdpSocket) -> Self {
        Self {
            socket: Arc::new(socket),
            pending_packets: Arc::new(DashMap::new()),
            sequence_number: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn send_reliable<S>(&self, payload: &S, addr: std::net::SocketAddr) -> Result<()>
    where
        S: Serialize + for<'de> Deserialize<'de> + Send + 'static,
    {
        let packet_id = self
            .sequence_number
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let packet = UdpPacket::Reliable { packet_id, payload };

        let data = bincode::serialize(&packet)?;
        self.socket.send_to(&data, addr).await?;

        let pending_packet = PendingPacket {
            data,
            destination: addr,
            timestamp: Instant::now(),
            retries: 0,
        };

        self.pending_packets.insert(packet_id, pending_packet);

        self.handle_retransmission(packet_id);

        Ok(())
    }

    pub async fn send_unreliable<S>(&self, payload: &S, addr: std::net::SocketAddr) -> Result<()>
    where
        S: Serialize + for<'de> Deserialize<'de> + Send + 'static,
    {
        let packet = UdpPacket::Unreliable(payload);
        let data = bincode::serialize(&packet)?;
        self.socket.send_to(&data, addr).await?;

        Ok(())
    }

    pub async fn send_unreliable_broadcast<S>(
        &self,
        payload: &S,
        addrs: &[std::net::SocketAddr],
    ) -> Result<()>
    where
        S: Serialize + for<'de> Deserialize<'de> + Send + 'static,
    {
        let packet = UdpPacket::Unreliable(payload);
        let data = bincode::serialize(&packet)?;

        for addr in addrs {
            self.socket.send_to(&data, addr).await?;
        }

        Ok(())
    }

    async fn send_ack<S>(&self, packet_id: u64, addr: std::net::SocketAddr) -> Result<()>
    where
        S: Serialize + for<'de> Deserialize<'de> + Send + 'static,
    {
        let packet = UdpPacket::<S>::Ack { packet_id };
        let data = bincode::serialize(&packet)?;
        self.socket.send_to(&data, addr).await?;

        Ok(())
    }

    pub async fn receive<R>(&self, buf: &mut [u8]) -> Result<Option<(R, std::net::SocketAddr)>>
    where
        R: Serialize + for<'de> Deserialize<'de> + Send + 'static,
    {
        let (size, addr) = self.socket.recv_from(buf).await?;
        let packet: UdpPacket<R> = bincode::deserialize(&buf[..size])?;

        match packet {
            UdpPacket::Ack { packet_id } => {
                self.pending_packets.remove(&packet_id);
                Ok(None)
            }
            UdpPacket::Reliable { packet_id, payload } => {
                self.send_ack::<R>(packet_id, addr).await?;

                Ok(Some((payload, addr)))
            }
            UdpPacket::Unreliable(payload) => Ok(Some((payload, addr))),
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
                        error!(
                            "Failed to deliver packet {} after {MAX_RETRIES} tries to {}.",
                            packet_id, pending_packet.destination
                        );
                        drop(pending_packet_entry);
                        pending_packets.remove(&packet_id);
                        break;
                    } else if pending_packet.timestamp.elapsed() >= TIMEOUT {
                        if let Err(e) = socket
                            .send_to(&pending_packet.data, pending_packet.destination)
                            .await
                        {
                            error!(
                                "Failed to deliver retransmission packet {} to {}: {}",
                                packet_id, pending_packet.destination, e
                            );
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
