use crate::node::{RouteStatus, State};
use common::packet::{
    ClientPacket, FloodPacket, NodePacket, Packet, ServerPacket, VideoListPacket, VideoPacket,
};
use common::reliable;
use common::reliable::{ReliablePacketResult, ReliableUdpSocketError};
use log::{debug, error, info, trace, warn};
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::time::SystemTime;
use tokio::sync::oneshot;

impl State {
    pub async fn handle_client_ping(
        &self,
        sequence_number: u64,
        addr: SocketAddr,
    ) -> anyhow::Result<()> {
        debug!("Received client ping from {}", addr);
        let packet = Packet::ClientPacket(ClientPacket::VideoList(VideoListPacket {
            sequence_number,
            videos: self.get_videos(),
            answer_creation_date: SystemTime::now(),
        }));
        self.socket.send_reliable(&packet, addr).await?;
        Ok(())
    }

    pub async fn handle_request_video(&self, video_id: u8, addr: SocketAddr) -> anyhow::Result<()> {
        let mut interested = self.interested.entry(video_id).or_default();
        if !interested.value().contains(&addr) {
            interested.value_mut().push(addr);
        }

        if self.video_routes.contains_key(&video_id) {
            // We are already receiving this video
            return Ok(());
        }

        let best_node = self.get_best_node_to_redirect(video_id);
        if let Some(node) = best_node {
            info!("Selected best node to redirect: {:?}", best_node);
            let packet = Packet::ServerPacket(ServerPacket::RequestVideo(video_id));
            self.socket.send_reliable(&packet, node).await?;
            self.video_routes.insert(video_id, node);
            Ok(())
        } else {
            // TODO: Add to queue and wait for a better node
            anyhow::bail!("Couldn't find suitable servers for redirecting packet")
        }
    }

    pub async fn handle_stop_video(&self, video_id: u8, addr: SocketAddr) -> anyhow::Result<()> {
        if let Some(mut subscribers) = self.interested.get_mut(&video_id) {
            subscribers.retain(|&subscriber| subscriber != addr);
        }

        let remaining_subscribers = self
            .interested
            .get(&video_id)
            .map_or(0, |s| s.value().len());

        if remaining_subscribers == 0 {
            if let Some((_, node_addr)) = self.video_routes.remove(&video_id) {
                let packet = Packet::ServerPacket(ServerPacket::StopVideo(video_id));
                self.socket.send_reliable(&packet, node_addr).await?;
            }
        }

        Ok(())
    }

    pub async fn handle_flood_packet(&self, from_addr: SocketAddr, flood_packet: FloodPacket) {
        debug!("Received flood packet {flood_packet:?} from {from_addr}");

        self.update_video_names(flood_packet.videos_available.clone());

        {
            let mut node_route_info = self.available_routes.entry(from_addr).or_default();
            node_route_info.update_from_flood_packet(&flood_packet);
            // drop route info to release the lock
        }

        let last_flood_packet_sequence_number = self
            .last_flood_packet_sequence_number
            .load(Ordering::Relaxed);

        let sequence_number = flood_packet.sequence_number;
        if sequence_number < last_flood_packet_sequence_number {
            // old packet in the network, ignore
            return;
        }

        self.last_flood_packet_sequence_number
            .store(sequence_number, Ordering::Relaxed);

        let mut visited_nodes = flood_packet.visited_nodes.clone();
        if visited_nodes.contains(&self.id) {
            // loop detected, ignore
            return;
        }

        visited_nodes.push(self.id);

        let flood_packet = Packet::NodePacket(NodePacket::FloodPacket(FloodPacket {
            hops: flood_packet.hops + 1,
            my_fathers: self.get_fathers(),
            visited_nodes,
            ..flood_packet
        }));

        self.broadcast_flood_packet(from_addr, flood_packet).await;
    }

    async fn broadcast_flood_packet(&self, from_addr: SocketAddr, flood_packet: Packet) {
        let broadcast_to: Vec<SocketAddr> = self
            .neighbours
            .iter()
            .map(|addr| SocketAddr::new(*addr, common::PORT))
            .filter(|addr| *addr != from_addr)
            .collect();

        debug!("Broadcasting flood packet ({flood_packet:?}) to {broadcast_to:?}");

        for addr in broadcast_to {
            let result = self.socket.send_reliable(&flood_packet, addr).await;
            tokio::spawn(self.clone().handle_send_flood_packet_result(addr, result));
        }
    }

    async fn handle_send_flood_packet_result(
        self,
        target: SocketAddr,
        result: reliable::Result<oneshot::Receiver<ReliablePacketResult>>,
    ) {
        let result = match result {
            Ok(receiver) => Ok(receiver.await),
            Err(e) => Err(e),
        };

        match result {
            Err(ReliableUdpSocketError::IoError(_)) | Ok(Ok(ReliablePacketResult::Timeout)) => {
                warn!("Node {target} is unresponsive.");
                if let Some(mut route) = self.available_routes.get_mut(&target) {
                    route.status = RouteStatus::Unresponsive;
                }
            }
            Ok(_) => {}
            Err(e) => error!("Failed to send flood packet to {target}: {e}"),
        }
    }

    pub(crate) async fn handle_video_packet(
        &self,
        addr: SocketAddr,
        packet: VideoPacket,
    ) -> anyhow::Result<()> {
        let stream_id = packet.stream_id;
        let sequence_number = packet.sequence_number;
        let last_sequence_number = self
            .last_video_sequence_number
            .insert(stream_id, sequence_number)
            .unwrap_or(sequence_number);

        let stream_data_len = packet.stream_data.len();
        trace!("Received video packet (stream={stream_id}, seq={sequence_number}, n={stream_data_len}) from {addr}");

        let expected = last_sequence_number + 1;
        if expected != sequence_number {
            error!("Received out of order packet (expected {expected}, got {sequence_number})");
        }

        if let Some(subscribers) = self.interested.get(&stream_id) {
            let subscribers = subscribers.clone();

            trace!("Sending video packet (stream={stream_id}, seq={sequence_number}) to {subscribers:?}");

            let packet = Packet::VideoPacket(packet);

            self.socket
                .send_unreliable_broadcast(&packet, &subscribers)
                .await?;
        }

        Ok(())
    }
}
