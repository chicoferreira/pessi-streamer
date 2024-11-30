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
    pub stream_id: u8,
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
    },

    /// Packets originated by the server
    FloodPacket(FloodPacket),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FloodPacket {
    /// The sequence number of the packet
    /// Each times a server sends a flood packet, this number is increased
    /// Used to ignore old packets that may be received after a long time
    pub sequence_number: u64,
    /// The number of hops this packet has done
    /// Used to calculate the best node to redirect a video
    pub hops: u8,
    /// The time the packet was created at the server
    /// Used to calculate the best node to redirect a video
    pub created_at_server_time: SystemTime,
    /// The videos available in this node
    pub videos_available: Vec<(u8, String)>,
    /// The nodes that have been visited by this packet
    /// Used to avoid loops
    pub visited_nodes: Vec<u64>,
    /// The fathers of the node that sent this packet
    /// Used to establish connection when there is only one node connecting to the rest of the network
    pub my_fathers: Vec<SocketAddr>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ServerPacket {
    /// Request to start a video
    RequestVideo(u8),
    /// Request to stop a video
    StopVideo(u8),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ClientPacket {
    /// Answer to a `Packet::NodePacket::ClientPing`
    VideoList(VideoListPacket),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct VideoListPacket {
    pub sequence_number: u64,
    pub videos: Vec<(u8, String)>,
    pub answer_creation_date: SystemTime,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum BootstrapperPacket {
    /// Request to get neighbours
    RequestNeighbours,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BootstrapperNeighboursResponse {
    pub neighbours: Vec<IpAddr>,
    pub id: u64,
}
