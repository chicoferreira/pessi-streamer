use crate::VideoId;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::time::SystemTime;

#[derive(Serialize, Deserialize, Debug)]
pub enum Packet {
    /// A video packet that contains video data
    VideoPacket(VideoPacket),
    /// Packets that only the node should receive
    NodePacket(NodePacket),
    /// Packets that only the server should receive
    ServerPacket(ServerPacket),
    /// Packets that only the client should receive
    ClientPacket(ClientPacket),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct VideoPacket {
    pub stream_id: VideoId,
    pub sequence_number: u64,
    pub stream_data: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum NodePacket {
    /// Ping packets sent by clients
    /// The node will answer with `Packet::ClientPacket::VideoList`
    ClientPing {
        /// The sequence number of the ping packet
        /// Used to match the response with the request
        sequence_number: u64,
        /// The list of currently requested videos
        /// Used to know which videos the client is interested in when the
        /// node dies and loses state, without having to send the `RequestVideo` packet again
        requested_videos: Vec<VideoId>,
    },

    /// Received when a node wants to connect to the node receiving this packet
    /// Used when a catastrophic node failure happens, and we can only connect to the node parent
    NewNeighbour,

    /// Packets originated by the server
    FloodPacket(FloodPacket),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FloodPacket {
    /// The time the packet was created at the server
    /// Used to calculate the best node to redirect a video
    pub created_at_server_time: SystemTime,
    /// The videos available in this node
    pub videos_available: Vec<(VideoId, String)>,
    /// The nodes that have been visited by this packet
    /// Used to avoid loops
    pub visited_nodes: Vec<u64>,
    /// The parents of the node that sent this packet
    /// Used to establish connection when there is only one node connecting to the rest of the network
    pub my_parents: Vec<SocketAddr>,
}

impl FloodPacket {
    pub fn hops(&self) -> usize {
        self.visited_nodes.len()
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ServerPacket {
    /// Request to start a video
    RequestVideo(VideoId),
    /// Request to stop a video
    StopVideo(VideoId),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ClientPacket {
    /// Answer to a `Packet::NodePacket::ClientPing`
    VideoList(VideoListPacket),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct VideoListPacket {
    pub sequence_number: u64,
    pub videos: Vec<(VideoId, String)>,
    pub answer_creation_date: SystemTime,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BootstrapperNeighboursResponse {
    pub neighbours: Vec<IpAddr>,
    pub id: u64,
}
