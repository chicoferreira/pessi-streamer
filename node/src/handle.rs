use crate::node::{RouteStatus, State};
use common::packet::{
    ClientPacket, FloodPacket, NodePacket, Packet, ServerPacket, VideoListPacket, VideoPacket,
};
use common::reliable;
use common::reliable::{ReliablePacketResult, ReliableUdpSocketError};
use log::{debug, error, info, trace, warn};
use std::cmp;
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::time::SystemTime;
use tokio::sync::oneshot;

impl State {
    pub async fn handle_client_ping(
        &self,
        sequence_number: u64,
        requested_videos: Vec<u8>,
        addr: SocketAddr,
    ) -> anyhow::Result<()> {
        debug!("Received client ping from {}", addr);
        let packet = Packet::ClientPacket(ClientPacket::VideoList(VideoListPacket {
            sequence_number,
            videos: self.get_videos(),
            answer_creation_date: SystemTime::now(),
        }));

        self.last_pings.insert(addr, SystemTime::now());

        for video_id in requested_videos {
            self.handle_request_video(video_id, addr).await?;
        }

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
            info!("Selected best node to redirect: {:?}", node);
            self.request_video_to_node(video_id, node).await?
        } else {
            warn!("Couldn't find suitable servers for video {video_id}. Adding to queue.");
            self.pending_video_requests.write().unwrap().push(video_id);
        }

        Ok(())
    }

    pub async fn handle_stop_video(&self, video_id: u8, addr: SocketAddr) -> anyhow::Result<()> {
        if let Some(mut subscribers) = self.interested.get_mut(&video_id) {
            subscribers.retain(|&subscriber| subscriber != addr);
        }
        info!("Removing subscriber {addr} from video {video_id}.");

        let remaining_subscribers = self
            .interested
            .get(&video_id)
            .map_or(0, |s| s.value().len());

        if remaining_subscribers == 0 {
            if let Some((_, node_addr)) = self.video_routes.remove(&video_id) {
                info!("No more subscribers for video {video_id}. Requesting to stop video to {node_addr}.");
                let packet = Packet::ServerPacket(ServerPacket::StopVideo(video_id));
                self.socket.send_reliable(&packet, node_addr).await?;
            }
        }

        Ok(())
    }

    pub async fn handle_flood_packet(&self, from_addr: SocketAddr, flood_packet: FloodPacket) {
        let sequence_number = flood_packet.sequence_number;
        debug!("Received flood packet {sequence_number} from {from_addr}");

        // update video names
        for (id, video_name) in flood_packet.videos_available.clone() {
            self.video_names.insert(id, video_name);
        }

        {
            let mut node_route_info = self.available_routes.entry(from_addr).or_default();
            node_route_info.update_from_flood_packet(&flood_packet);
            // drop route info to release the lock
        }

        let last_flood_packet_sequence_number = self
            .last_flood_packet_sequence_number
            .load(Ordering::Relaxed);

        // if contains videos in pending videos, ask for them and remove them from queue
        self.check_pending_videos(from_addr, &flood_packet.videos_available)
            .await;

        if sequence_number < last_flood_packet_sequence_number {
            // old packet in the network, ignore
            return;
        }

        self.last_flood_packet_sequence_number
            .store(sequence_number, Ordering::Relaxed);

        if flood_packet.visited_nodes.contains(&self.id) {
            // loop detected, ignore
            return;
        }

        let mut visited_nodes = flood_packet.visited_nodes;
        visited_nodes.push(self.id);

        let flood_packet = Packet::NodePacket(NodePacket::FloodPacket(FloodPacket {
            hops: flood_packet.hops + 1,
            my_parents: self.get_parents(),
            visited_nodes,
            ..flood_packet
        }));

        self.broadcast_flood_packet(from_addr, flood_packet).await;
    }

    async fn broadcast_flood_packet(&self, from_addr: SocketAddr, flood_packet: Packet) {
        let broadcast_to: Vec<SocketAddr> = self
            .neighbours
            .read()
            .unwrap()
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
                warn!("Node {target} didn't ack our flood packet. Marking it as unresponsive.");
                if let Some(mut route) = self.available_routes.get_mut(&target) {
                    route.status = RouteStatus::Unresponsive;

                    let videos_to_stop_sending: Vec<u8> = self
                        .interested
                        .iter()
                        .filter(|entry| entry.contains(&target))
                        .map(|entry| *entry.key())
                        .collect();

                    if !videos_to_stop_sending.is_empty() {
                        for video_id in videos_to_stop_sending {
                            if let Err(e) = self.handle_stop_video(video_id, target).await {
                                error!("Failed to stop video {video_id} to {target}: {e}");
                            }
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(e) => error!("Failed to send flood packet to {target}: {e}"),
        }
    }

    pub async fn handle_video_packet(
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

        self.last_video_packet_time
            .insert(stream_id, SystemTime::now());

        let expected = last_sequence_number + 1;
        match expected.cmp(&sequence_number) {
            cmp::Ordering::Less => { // ignore older packets
                error!("Received old video packet (expected {expected}, got {sequence_number})");
                return Ok(());
            }
            cmp::Ordering::Greater => { // just notify
                error!("Received out of order packet (expected {expected}, got {sequence_number})");
            }
            cmp::Ordering::Equal => {} // perfect
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
