use dashmap::DashMap;
use log::error;
use serde::{Deserialize, Serialize};
use socket2::SockRef;
use std::io;
use std::net::SocketAddr;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::{Duration, Instant};
use sync::oneshot;
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::sync;

const MAX_RETRIES: u8 = 5;
const TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Clone)]
pub struct ReliableUdpSocket {
    socket: Arc<UdpSocket>,
    pending_packets: Arc<DashMap<u64, PendingPacket>>,
    sequence_number: Arc<AtomicU64>,
}

impl ReliableUdpSocket {
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }
}

struct PendingPacket {
    data: Vec<u8>,
    sender: oneshot::Sender<ReliablePacketResult>,
    destination: SocketAddr,
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

pub enum ReliablePacketResult {
    Acknowledged(Duration),
    Timeout,
}

pub type Result<T> = std::result::Result<T, ReliableUdpSocketError>;

impl ReliableUdpSocket {
    pub async fn new(addr: SocketAddr) -> io::Result<Self> {
        let socket = UdpSocket::bind(addr).await?;

        let sock_ref = SockRef::from(&socket);
        sock_ref.set_send_buffer_size(1_048_576)?;
        sock_ref.set_recv_buffer_size(1_048_576)?;

        Ok(Self {
            socket: Arc::new(socket),
            pending_packets: Arc::new(DashMap::new()),
            sequence_number: Arc::new(AtomicU64::new(0)),
        })
    }

    pub async fn send_reliable<S>(
        &self,
        payload: &S,
        addr: SocketAddr,
    ) -> Result<oneshot::Receiver<ReliablePacketResult>>
    where
        S: Serialize + for<'de> Deserialize<'de> + Send + 'static,
    {
        let packet_id = self
            .sequence_number
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let packet = UdpPacket::Reliable { packet_id, payload };

        let data = bincode::serialize(&packet)?;
        self.socket.send_to(&data, addr).await?;

        let (tx, rx) = oneshot::channel();

        let pending_packet = PendingPacket {
            data,
            sender: tx,
            destination: addr,
            timestamp: Instant::now(),
            retries: 0,
        };

        self.pending_packets.insert(packet_id, pending_packet);

        self.handle_retransmission(packet_id);

        Ok(rx)
    }

    pub async fn send_unreliable<S>(&self, payload: &S, addr: SocketAddr) -> Result<()>
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
        addrs: &[SocketAddr],
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

    async fn send_ack<S>(&self, packet_id: u64, addr: SocketAddr) -> Result<()>
    where
        S: Serialize + for<'de> Deserialize<'de> + Send + 'static,
    {
        let packet = UdpPacket::<S>::Ack { packet_id };
        let data = bincode::serialize(&packet)?;
        self.socket.send_to(&data, addr).await?;

        Ok(())
    }

    pub async fn receive<R>(&self, buf: &mut [u8]) -> Result<Option<(R, SocketAddr)>>
    where
        R: Serialize + for<'de> Deserialize<'de> + Send + 'static,
    {
        let (size, addr) = self.socket.recv_from(buf).await?;
        let packet: UdpPacket<R> = bincode::deserialize(&buf[..size])?;

        match packet {
            UdpPacket::Ack { packet_id } => {
                if let Some((_, packet)) = self.pending_packets.remove(&packet_id) {
                    let duration = packet.timestamp.elapsed();
                    let result = ReliablePacketResult::Acknowledged(duration);
                    let _ = packet.sender.send(result);
                }
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
                            "Failed to deliver packet {packet_id} after {MAX_RETRIES} tries to {}.",
                            pending_packet.destination
                        );
                        drop(pending_packet_entry);
                        
                        if let Some((_, packet)) = pending_packets.remove(&packet_id) {
                            let _ = packet.sender.send(ReliablePacketResult::Timeout);
                        }
                        
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
